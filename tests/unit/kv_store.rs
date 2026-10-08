//! test/memory-kv.test.js and test/redisKvAdapter.test.js, against the embedded store.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use sublink::storage::Store;

fn store_with_clock() -> (Store, Arc<AtomicU64>) {
    let now = Arc::new(AtomicU64::new(1_700_000_000_000));
    let handle = now.clone();
    (Store::in_memory().with_clock(Arc::new(move || handle.load(Ordering::SeqCst))), now)
}

fn advance(now: &AtomicU64, ms: u64) {
    now.fetch_add(ms, Ordering::SeqCst);
}

#[test]
fn keeps_values_with_ttls_beyond_the_set_timeout_limit() {
    let (kv, now) = store_with_clock();
    kv.put("k", "v", Some(60.0 * 60.0 * 24.0 * 30.0)).unwrap();
    advance(&now, 60 * 1000);
    assert_eq!(kv.get("k").unwrap().as_deref(), Some("v"));
}

#[test]
fn expires_values_after_their_ttl() {
    let (kv, now) = store_with_clock();
    kv.put("k", "v", Some(60.0)).unwrap();
    advance(&now, 61 * 1000);
    assert_eq!(kv.get("k").unwrap(), None);
}

#[test]
fn keeps_values_without_ttl_indefinitely() {
    let (kv, now) = store_with_clock();
    kv.put("k", "v", None).unwrap();
    advance(&now, 60 * 60 * 24 * 31 * 1000);
    assert_eq!(kv.get("k").unwrap().as_deref(), Some("v"));
}

#[test]
fn stores_and_retrieves_values() {
    let (kv, _) = store_with_clock();
    kv.put("greeting", "hello", None).unwrap();
    kv.put("farewell", "bye", None).unwrap();
    assert_eq!(kv.get("greeting").unwrap().as_deref(), Some("hello"));
    assert_eq!(kv.get("farewell").unwrap().as_deref(), Some("bye"));
}

#[test]
fn deletes_values() {
    let (kv, _) = store_with_clock();
    kv.put("temp", "value", None).unwrap();
    kv.delete("temp").unwrap();
    assert_eq!(kv.get("temp").unwrap(), None);
}

#[test]
fn applies_ttl_when_provided() {
    let (kv, now) = store_with_clock();
    kv.put("ttl-key", "value", Some(30.0)).unwrap();
    advance(&now, 29_999);
    assert_eq!(kv.get("ttl-key").unwrap().as_deref(), Some("value"));
    advance(&now, 1);
    assert_eq!(kv.get("ttl-key").unwrap(), None);
}
