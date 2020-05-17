use std::collections::HashMap;
use std::convert::TryInto;

use crate::kv::{KeyValue, RouteCollection};
use crate::KvsError;

/// Front-end Key/Value store for [KeyValue](struct.KeyValue.html) pairs that can be encoded/decoded as
/// BGP Update announcements
///
/// As contained [KeyValue](struct.KeyValue.html)s are added/updated/removed, serialization
pub struct KvStore {
    /// Internal storage of [Key](struct.Key.html) -> [KeyValue](struct.KeyValue.html) pairs
    inner: HashMap<String, KeyValue<String, String>>,
}

impl KvStore {
    /// Create a new, empty KvStore
    pub fn new() -> Self {
        Self {
            inner: HashMap::with_capacity(16),
        }
    }

    /// Number of unique [Key](struct.Key.html)s in this store
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Is this store empty?
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Insert a new [Key](struct.Key.html) / [Value](struct.Value.html) pair
    ///
    /// Will construct a [KeyValue](struct.KeyValue.html) to store and queue for BGP synchronization.
    /// If the key already exists in this KvStore, will updated the existing value and also queue
    /// a BGP withdraw for the old [KeyValue](struct.KeyValue.html)
    pub fn insert(&mut self, key: String, value: String) -> Result<Update, KvsError> {
        if let Some(existing) = self.inner.get_mut(&key) {
            // Temporarily cast away mut for TryFrom<&KeyValue> to match
            let withdraw: RouteCollection = (&*existing).try_into().map_err(|_| {
                KvsError::EncodeError(format!("Could not encode: {}", existing.to_string()))
            })?;
            existing.update(value);
            let announce: RouteCollection = (&*existing).try_into().map_err(|_| {
                KvsError::EncodeError(format!("Could not encode: {}", existing.to_string()))
            })?;
            Ok(Update::with_both(announce, withdraw))
        } else {
            let kv = KeyValue::new(key.clone(), value);
            let announce: RouteCollection = (&kv).try_into()?;
            self.inner.insert(key, kv);
            Ok(Update::with_announce(announce))
        }
    }

    /// Retrieve a [Value](struct.Value.html) by a given &[Key](struct.Key.html)
    pub fn get(&self, key: &str) -> Option<String> {
        self.inner.get(key).map(|kv| kv.as_ref().clone())
    }

    /// Remove a [KeyValue](struct.KeyValue.html) by a given &[Key](struct.Key.html)
    pub fn remove(&mut self, key: &str) -> Result<Option<Update>, KvsError> {
        if let Some(removed) = self.inner.remove(key) {
            let withdraw: RouteCollection = (&removed).try_into().map_err(|_| {
                KvsError::EncodeError(format!("Could not encode: {}", removed.to_string()))
            })?;
            Ok(Some(Update::with_withdraw(withdraw)))
        } else {
            Ok(None)
        }
    }
}

/// A Pending update to be sent to BGP Peers
///
/// - A new [KeyValue](struct.KeyValue.html) will only have an announcement
/// - An updated [KeyValue](struct.KeyValue.html) will announce the new value (and version), will withdraw the old value
/// - A removed [KeyValue](struct.KeyValue.html) will only have a withdraw
#[derive(Debug)]
pub struct Update {
    pub announce: Option<RouteCollection>,
    pub withdraw: Option<RouteCollection>,
}

impl Update {
    fn with_announce(announce: RouteCollection) -> Self {
        Self {
            announce: Some(announce),
            withdraw: None,
        }
    }

    fn with_withdraw(withdraw: RouteCollection) -> Self {
        Self {
            announce: None,
            withdraw: Some(withdraw),
        }
    }

    fn with_both(announce: RouteCollection, withdraw: RouteCollection) -> Self {
        Self {
            announce: Some(announce),
            withdraw: Some(withdraw),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_new_insert() {
        let mut store = KvStore::new();
        assert!(store.is_empty());

        store.insert("Key".to_owned(), "Value".to_owned()).unwrap();
        assert_eq!(store.len(), 1);
        assert_eq!(store.get("Key"), Some("Value".to_owned()));
    }

    #[test]
    fn store_insert_and_update() {
        let mut store = KvStore::new();

        let update = store.insert("Key".to_owned(), "Value".to_owned()).unwrap();
        assert!(update.announce.is_some());
        assert!(update.withdraw.is_none());

        let routes: Vec<_> = update.announce.unwrap().iter().cloned().collect();
        assert_eq!(routes[0].next_hop.version(), 0);

        let update = store.insert("Key".to_owned(), "42".to_owned()).unwrap();
        assert!(update.announce.is_some());
        assert!(update.withdraw.is_some());

        let a_routes: Vec<_> = update.announce.unwrap().iter().cloned().collect();
        assert_eq!(a_routes[0].next_hop.version(), 1);

        let w_routes: Vec<_> = update.withdraw.unwrap().iter().cloned().collect();
        assert_eq!(w_routes[0].next_hop.version(), 0);
    }

    #[test]
    fn store_remove() {
        let mut store = KvStore::new();
        store.insert("Key".to_owned(), "Value".to_owned()).unwrap();

        let update = store.remove("Key").unwrap();
        assert_eq!(store.get("Key"), None);
        assert!(&update.is_some());
        let update = update.unwrap();
        assert!(update.announce.is_none());
        assert!(update.withdraw.is_some());
    }
}
