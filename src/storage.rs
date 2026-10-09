//! In-memory key-value store with per-key TTL, made durable by an append-only log.
//!
//! Every write is appended to the log as one JSON line and synced to disk
//! before it becomes visible; startup replays the log. Short links and saved
//! base configs share one namespace, exactly like the single KV binding of the
//! original deployment.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::js::{Value, json};

/// Below this size rewriting the log costs more than the space it reclaims.
const COMPACT_MIN_RECORDS: usize = 1024;

pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

#[derive(Clone)]
pub struct Store {
    inner: Arc<Mutex<Inner>>,
    clock: Clock,
}

struct Inner {
    entries: HashMap<String, Entry>,
    log: Option<Log>,
}

struct Entry {
    value: String,
    /// Milliseconds since the epoch; `0` means "never expires".
    expires_at: u64,
}

struct Log {
    path: PathBuf,
    file: File,
    /// Records in the file, live or not; drives compaction.
    records: usize,
    /// Held for the process lifetime so a second instance cannot interleave writes.
    _lock: File,
}

enum Record {
    Set { key: String, value: String, expires_at: u64 },
    Del { key: String },
}

fn system_clock() -> Clock {
    Arc::new(|| SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0))
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Redis `SET .. EX` semantics: fractional TTLs are floored and anything that
/// does not end up positive stores the key without expiry.
fn normalize_ttl(ttl_seconds: Option<f64>) -> Option<u64> {
    let ttl = ttl_seconds?.floor();
    (ttl.is_finite() && ttl > 0.0).then_some(ttl as u64)
}

fn is_live(entry: &Entry, now: u64) -> bool {
    entry.expires_at == 0 || now < entry.expires_at
}

fn encode_set(key: &str, entry: &Entry) -> String {
    let record = Value::array(vec![
        Value::str("set"),
        Value::str(key),
        Value::str(entry.value.as_str()),
        Value::Number(entry.expires_at as f64),
    ]);
    json::stringify(&record).unwrap_or_default() + "\n"
}

fn encode_del(key: &str) -> String {
    json::stringify(&Value::array(vec![Value::str("del"), Value::str(key)])).unwrap_or_default() + "\n"
}

fn decode(line: &[u8]) -> Option<Record> {
    let parsed = json::parse(std::str::from_utf8(line).ok()?).ok()?;
    let fields = parsed.as_array()?;
    let text = |i: usize| fields.get(i).and_then(Value::as_str).map(str::to_string);
    match text(0)?.as_str() {
        "set" => Some(Record::Set {
            key: text(1)?,
            value: text(2)?,
            expires_at: fields.get(3).and_then(Value::as_number).filter(|n| *n >= 0.0)? as u64,
        }),
        "del" => Some(Record::Del { key: text(1)? }),
        _ => None,
    }
}

impl Log {
    fn append(&mut self, line: &str) -> Result<(), String> {
        self.file.write_all(line.as_bytes()).map_err(err)?;
        self.file.sync_data().map_err(err)?;
        self.records += 1;
        Ok(())
    }
}

/// Replays log bytes into `entries`; returns the record count and the length of
/// the well-formed prefix (a crash can leave a torn final line behind).
fn replay(bytes: &[u8], entries: &mut HashMap<String, Entry>) -> (usize, usize) {
    let mut records = 0;
    let mut valid_len = 0;
    while let Some(end) = bytes[valid_len..].iter().position(|&b| b == b'\n') {
        let line = &bytes[valid_len..valid_len + end];
        match decode(line) {
            Some(Record::Set { key, value, expires_at }) => {
                entries.insert(key, Entry { value, expires_at });
            }
            Some(Record::Del { key }) => {
                entries.remove(&key);
            }
            None => eprintln!("Skipping unreadable store record at byte {valid_len}"),
        }
        records += 1;
        valid_len += end + 1;
    }
    (records, valid_len)
}

fn sync_parent(path: &Path) -> Result<(), String> {
    match path.parent().filter(|d| !d.as_os_str().is_empty()) {
        Some(dir) => File::open(dir).and_then(|d| d.sync_all()).map_err(err),
        None => File::open(".").and_then(|d| d.sync_all()).map_err(err),
    }
}

impl Store {
    /// Opens (or creates) the log at `path`, creating parent directories.
    pub fn open(path: impl AsRef<Path>) -> Result<Store, String> {
        let path = path.as_ref().to_path_buf();
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            fs::create_dir_all(dir).map_err(err)?;
        }
        let lock_path = PathBuf::from(format!("{}.lock", path.display()));
        let lock = OpenOptions::new().create(true).truncate(false).write(true).open(&lock_path).map_err(err)?;
        lock.try_lock().map_err(|_| format!("{} is in use by another process", path.display()))?;

        let mut file = OpenOptions::new().read(true).append(true).create(true).open(&path).map_err(err)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(err)?;
        let mut entries = HashMap::new();
        let (records, valid_len) = replay(&bytes, &mut entries);
        if valid_len < bytes.len() {
            eprintln!("Discarding {} bytes of an incomplete store record", bytes.len() - valid_len);
            file.set_len(valid_len as u64).map_err(err)?;
            file.sync_data().map_err(err)?;
        }

        let store = Store {
            inner: Arc::new(Mutex::new(Inner { entries, log: Some(Log { path, file, records, _lock: lock }) })),
            clock: system_clock(),
        };
        store.sweep()?;
        Ok(store)
    }

    /// Volatile store for tests.
    pub fn in_memory() -> Store {
        Store { inner: Arc::new(Mutex::new(Inner { entries: HashMap::new(), log: None })), clock: system_clock() }
    }

    /// Replaces the wall clock (milliseconds since the epoch), for TTL tests.
    pub fn with_clock(mut self, clock: Clock) -> Store {
        self.clock = clock;
        self
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A panic while holding the lock cannot leave the map half-updated.
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn get(&self, key: &str) -> Result<Option<String>, String> {
        let now = (self.clock)();
        let mut inner = self.lock();
        match inner.entries.get(key) {
            Some(entry) if is_live(entry, now) => Ok(Some(entry.value.clone())),
            Some(_) => {
                inner.entries.remove(key);
                Ok(None)
            }
            None => Ok(None),
        }
    }

    pub fn put(&self, key: &str, value: &str, ttl_seconds: Option<f64>) -> Result<(), String> {
        let expires_at = normalize_ttl(ttl_seconds).map_or(0, |ttl| (self.clock)().saturating_add(ttl * 1000));
        let entry = Entry { value: value.to_string(), expires_at };
        let mut inner = self.lock();
        if let Some(log) = inner.log.as_mut() {
            log.append(&encode_set(key, &entry))?;
        }
        inner.entries.insert(key.to_string(), entry);
        Ok(())
    }

    pub fn delete(&self, key: &str) -> Result<(), String> {
        let mut inner = self.lock();
        if !inner.entries.contains_key(key) {
            return Ok(());
        }
        if let Some(log) = inner.log.as_mut() {
            log.append(&encode_del(key))?;
        }
        inner.entries.remove(key);
        Ok(())
    }

    /// Drops expired entries and compacts the log once most of it is dead;
    /// returns how many entries expired.
    pub fn sweep(&self) -> Result<usize, String> {
        let now = (self.clock)();
        let mut inner = self.lock();
        let before = inner.entries.len();
        inner.entries.retain(|_, entry| is_live(entry, now));
        let removed = before - inner.entries.len();
        let live = inner.entries.len();
        if inner.log.as_ref().is_some_and(|log| log.records > COMPACT_MIN_RECORDS && log.records > 2 * live) {
            compact(&mut inner)?;
        }
        Ok(removed)
    }

    /// Rewrites the log so it holds exactly one record per live entry.
    pub fn compact(&self) -> Result<(), String> {
        let now = (self.clock)();
        let mut inner = self.lock();
        inner.entries.retain(|_, entry| is_live(entry, now));
        compact(&mut inner)
    }
}

fn compact(inner: &mut Inner) -> Result<(), String> {
    let Inner { entries, log } = inner;
    let Some(log) = log.as_mut() else { return Ok(()) };
    let tmp = PathBuf::from(format!("{}.tmp", log.path.display()));
    {
        let mut out = File::create(&tmp).map_err(err)?;
        let mut buf = String::new();
        for (key, entry) in entries.iter() {
            buf.push_str(&encode_set(key, entry));
        }
        out.write_all(buf.as_bytes()).map_err(err)?;
        out.sync_all().map_err(err)?;
    }
    // rename() is atomic: a crash leaves either the old or the new log intact.
    fs::rename(&tmp, &log.path).map_err(err)?;
    sync_parent(&log.path)?;
    log.file = OpenOptions::new().append(true).open(&log.path).map_err(err)?;
    log.records = entries.len();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn fake_clock() -> (Arc<AtomicU64>, Clock) {
        let now = Arc::new(AtomicU64::new(1_000_000));
        let handle = now.clone();
        (now, Arc::new(move || handle.load(Ordering::SeqCst)))
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> TempDir {
            TempDir(std::env::temp_dir().join(format!("sublink-store-{}", fastrand::u64(..))))
        }

        fn log(&self) -> PathBuf {
            self.0.join("nested").join("kv.aof")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn ttl_normalization_follows_redis() {
        assert_eq!(normalize_ttl(None), None);
        assert_eq!(normalize_ttl(Some(0.0)), None);
        assert_eq!(normalize_ttl(Some(-5.0)), None);
        assert_eq!(normalize_ttl(Some(0.9)), None);
        assert_eq!(normalize_ttl(Some(1.9)), Some(1));
        assert_eq!(normalize_ttl(Some(f64::NAN)), None);
        assert_eq!(normalize_ttl(Some(f64::INFINITY)), None);
    }

    #[test]
    fn sweep_drops_only_expired_entries() {
        let (now, clock) = fake_clock();
        let store = Store::in_memory().with_clock(clock);
        store.put("a", "1", Some(10.0)).unwrap();
        store.put("b", "2", None).unwrap();
        now.fetch_add(10_000, Ordering::SeqCst);
        assert_eq!(store.sweep().unwrap(), 1);
        assert_eq!(store.get("a").unwrap(), None);
        assert_eq!(store.get("b").unwrap().as_deref(), Some("2"));
        store.delete("b").unwrap();
        assert_eq!(store.get("b").unwrap(), None);
    }

    #[test]
    fn overwriting_clears_previous_ttl() {
        let (now, clock) = fake_clock();
        let store = Store::in_memory().with_clock(clock);
        store.put("k", "v1", Some(5.0)).unwrap();
        store.put("k", "v2", None).unwrap();
        now.fetch_add(60_000, Ordering::SeqCst);
        assert_eq!(store.get("k").unwrap().as_deref(), Some("v2"));
    }

    #[test]
    fn log_replays_writes_deletes_and_ttls_after_reopen() {
        let dir = TempDir::new();
        {
            let store = Store::open(dir.log()).unwrap();
            store.put("kept", "line1\nline2 \"quoted\" 中文", None).unwrap();
            store.put("deleted", "x", None).unwrap();
            store.delete("deleted").unwrap();
            store.put("overwritten", "old", None).unwrap();
            store.put("overwritten", "new", Some(3600.0)).unwrap();
            store.put("expired", "gone", Some(1.0)).unwrap();
        }
        let (now, clock) = fake_clock();
        now.store(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64 + 2_000, Ordering::SeqCst);
        let store = Store::open(dir.log()).unwrap().with_clock(clock);
        assert_eq!(store.get("kept").unwrap().as_deref(), Some("line1\nline2 \"quoted\" 中文"));
        assert_eq!(store.get("deleted").unwrap(), None);
        assert_eq!(store.get("overwritten").unwrap().as_deref(), Some("new"));
        assert_eq!(store.get("expired").unwrap(), None);
    }

    #[test]
    fn torn_final_record_is_discarded() {
        let dir = TempDir::new();
        Store::open(dir.log()).unwrap().put("a", "1", None).unwrap();
        let mut file = OpenOptions::new().append(true).open(dir.log()).unwrap();
        file.write_all(br#"["set","b","par"#).unwrap();
        drop(file);

        let store = Store::open(dir.log()).unwrap();
        assert_eq!(store.get("a").unwrap().as_deref(), Some("1"));
        assert_eq!(store.get("b").unwrap(), None);
        store.put("c", "3", None).unwrap();
        drop(store);
        let store = Store::open(dir.log()).unwrap();
        assert_eq!(store.get("c").unwrap().as_deref(), Some("3"));
    }

    #[test]
    fn compaction_keeps_live_entries_and_shrinks_the_log() {
        let dir = TempDir::new();
        {
            let store = Store::open(dir.log()).unwrap();
            for i in 0..50 {
                store.put("hot", &i.to_string(), None).unwrap();
            }
            store.put("cold", "c", None).unwrap();
            let before = fs::metadata(dir.log()).unwrap().len();
            store.compact().unwrap();
            assert!(fs::metadata(dir.log()).unwrap().len() < before / 10);
            store.put("after", "a", None).unwrap();
        }
        let store = Store::open(dir.log()).unwrap();
        assert_eq!(store.get("hot").unwrap().as_deref(), Some("49"));
        assert_eq!(store.get("cold").unwrap().as_deref(), Some("c"));
        assert_eq!(store.get("after").unwrap().as_deref(), Some("a"));
    }

    #[test]
    fn a_second_process_cannot_open_the_same_log() {
        let dir = TempDir::new();
        let _first = Store::open(dir.log()).unwrap();
        let second = Store::open(dir.log()).err().expect("second open must fail");
        assert!(second.contains("in use"), "{second}");
    }
}
