use std::collections::HashMap;

mod kv;
use kv::KeyValue;

struct KvStore {
    inner: HashMap<String, KeyValue<String, String>>,
}

impl KvStore {
    pub fn new() -> Self {
        Self::with_capacity(8)
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn with_capacity(size: usize) -> Self {
        Self {
            inner: HashMap::with_capacity(size),
        }
    }

    pub fn insert(&mut self, key: String, value: String) {
        if let Some(ref mut existing) = self.inner.get_mut(&key) {
            existing.update(value);
        } else {
            let kv = KeyValue::new(key.clone(), value);
            self.inner.insert(key, kv);
        }
    }

    pub fn get(&mut self, key: &str) -> Option<String> {
        self.inner.get(key).map(|kv| kv.value().clone())
    }

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
