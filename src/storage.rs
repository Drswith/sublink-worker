//! Embedded key-value store with per-key TTL, replacing Redis / Cloudflare KV.
//!
//! Short links and saved base configs share one namespace, exactly like the
//! single KV binding of the original deployment.

use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use redb::backends::InMemoryBackend;
use redb::{Database, ReadableDatabase, TableDefinition};

/// key -> (expires_at_ms, value); `0` means "never expires".
const TABLE: TableDefinition<&str, (u64, &str)> = TableDefinition::new("kv");

/// Keeps the page cache small: the working set is a handful of short strings.
const CACHE_BYTES: usize = 16 * 1024 * 1024;

pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

#[derive(Clone)]
pub struct Store {
    db: Arc<Database>,
    clock: Clock,
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

impl Store {
    /// Opens (or creates) the database file, creating parent directories.
    pub fn open(path: impl AsRef<Path>) -> Result<Store, String> {
        let path = path.as_ref();
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(err)?;
        }
        let db = Database::builder().set_cache_size(CACHE_BYTES).create(path).map_err(err)?;
        Store::init(db)
    }

    /// Volatile store, used by tests and `DB_PATH=:memory:`.
    pub fn in_memory() -> Store {
        let db = Database::builder()
            .set_cache_size(CACHE_BYTES)
            .create_with_backend(InMemoryBackend::new())
            .expect("in-memory database");
        Store::init(db).expect("in-memory table")
    }

    fn init(db: Database) -> Result<Store, String> {
        let txn = db.begin_write().map_err(err)?;
        txn.open_table(TABLE).map_err(err)?;
        txn.commit().map_err(err)?;
        Ok(Store { db: Arc::new(db), clock: system_clock() })
    }

    /// Replaces the wall clock (milliseconds since the epoch), for TTL tests.
    pub fn with_clock(mut self, clock: Clock) -> Store {
        self.clock = clock;
        self
    }

    pub fn get(&self, key: &str) -> Result<Option<String>, String> {
        let txn = self.db.begin_read().map_err(err)?;
        let table = txn.open_table(TABLE).map_err(err)?;
        let Some(entry) = table.get(key).map_err(err)? else { return Ok(None) };
        let (expires_at, value) = entry.value();
        // Expired entries stay on disk until the next sweep; reads must not see them.
        if expires_at != 0 && (self.clock)() >= expires_at {
            return Ok(None);
        }
        Ok(Some(value.to_string()))
    }

    pub fn put(&self, key: &str, value: &str, ttl_seconds: Option<f64>) -> Result<(), String> {
        let expires_at = normalize_ttl(ttl_seconds).map_or(0, |ttl| (self.clock)().saturating_add(ttl * 1000));
        let txn = self.db.begin_write().map_err(err)?;
        txn.open_table(TABLE).map_err(err)?.insert(key, (expires_at, value)).map_err(err)?;
        txn.commit().map_err(err)
    }

    pub fn delete(&self, key: &str) -> Result<(), String> {
        let txn = self.db.begin_write().map_err(err)?;
        txn.open_table(TABLE).map_err(err)?.remove(key).map_err(err)?;
        txn.commit().map_err(err)
    }

    /// Physically removes expired entries; returns how many were dropped.
    pub fn sweep(&self) -> Result<usize, String> {
        let now = (self.clock)();
        let txn = self.db.begin_write().map_err(err)?;
        let mut removed = 0;
        txn.open_table(TABLE)
            .map_err(err)?
            .retain(|_, (expires_at, _)| {
                let keep = expires_at == 0 || now < expires_at;
                removed += usize::from(!keep);
                keep
            })
            .map_err(err)?;
        txn.commit().map_err(err)?;
        Ok(removed)
    }
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
        assert_eq!(store.get("a").unwrap(), None);
        assert_eq!(store.sweep().unwrap(), 1);
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
    fn file_store_persists_across_reopen() {
        let dir = std::env::temp_dir().join(format!("sublink-store-{}", fastrand::u64(..)));
        let path = dir.join("nested").join("kv.redb");
        {
            let store = Store::open(&path).unwrap();
            store.put("k", "v", None).unwrap();
        }
        let store = Store::open(&path).unwrap();
        assert_eq!(store.get("k").unwrap().as_deref(), Some("v"));
        drop(store);
        let _ = std::fs::remove_dir_all(dir);
    }
}
