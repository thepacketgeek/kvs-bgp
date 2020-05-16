use std::collections::HashMap;

use crate::kv::KeyValue;

/// Front-end Key/Value store for [KeyValue](struct.KeyValue.html) pairs that can be encoded/decoded as
/// BGP Update announcements
///
/// As contained [KeyValue](struct.KeyValue.html)s are added/updated/removed, serialization
pub struct KvStore {
    inner: HashMap<String, KeyValue<String, String>>,
}

impl KvStore {
    /// Create a new, empty KvStore
    pub fn new() -> Self {
        Self::with_capacity(8)
    }

    /// Number of unique [Key](struct.Key.html)s in this store
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Is this store empty?
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Create with an initial capacity (default is 8)
    pub fn with_capacity(size: usize) -> Self {
        Self {
            inner: HashMap::with_capacity(size),
        }
    }

    /// Insert a new [Key](struct.Key.html) / [Value](struct.Value.html) pair
    ///
    /// Will construct a [KeyValue](struct.KeyValue.html) to store and queue for BGP synchronization
    pub fn insert(&mut self, key: String, value: String) {
        if let Some(ref mut existing) = self.inner.get_mut(&key) {
            existing.update(value);
        } else {
            let kv = KeyValue::new(key.clone(), value);
            self.inner.insert(key, kv);
        }
    }

    /// Retrieve a [Value](struct.Value.html) by a given &[Key](struct.Key.html)
    pub fn get(&mut self, key: &str) -> Option<String> {
        self.inner.get(key).map(|kv| kv.as_ref().clone())
    }

    /// Remove a [KeyValue](struct.KeyValue.html) by a given &[Key](struct.Key.html)
    pub fn remove(&mut self, key: &str) {
        self.inner.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store() {
        let mut store = KvStore::new();
        assert!(store.is_empty());

        store.insert("Key".to_owned(), "Value".to_owned());
        assert_eq!(store.get("Key"), Some("Value".to_owned()));

        store.insert("Key".to_owned(), "42".to_owned());
        assert_eq!(store.get("Key"), Some("42".to_owned()));
        assert_eq!(store.inner.get("Key").unwrap().version(), 1);

        store.remove("Key");
        assert_eq!(store.len(), 0);
        assert_eq!(store.get("Key"), None);
    }
}
