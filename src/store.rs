use crate::kv::{KeyValue, RouteCollection};
use crate::KvsError;
use std::collections::{HashMap, VecDeque};
use std::convert::TryInto;

/// Front-end Key/Value store for [KeyValue](struct.KeyValue.html) pairs that can be encoded/decoded as
/// BGP Update announcements
///
/// As contained [KeyValue](struct.KeyValue.html)s are added/updated/removed, serialization
pub struct KvStore {
    /// Internal storage of [Key](struct.Key.html) -> [KeyValue](struct.KeyValue.html) pairs
    inner: HashMap<String, KeyValue<String, String>>,
    /// Queued outbound updates (to be sent to remote peers)
    outbound: VecDeque<Update>,
    /// Queued inbound updates (received from remote peers)
    inbound: VecDeque<Update>,
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
            outbound: VecDeque::with_capacity(size),
            inbound: VecDeque::with_capacity(size),
        }
    }

    /// Insert a new [Key](struct.Key.html) / [Value](struct.Value.html) pair
    ///
    /// Will construct a [KeyValue](struct.KeyValue.html) to store and queue for BGP synchronization.
    /// If the key already exists in this KvStore, will updated the existing value and also queue
    /// a BGP withdraw for the old [KeyValue](struct.KeyValue.html)
    pub fn insert(&mut self, key: String, value: String) -> Result<(), KvsError> {
        if let Some(existing) = self.inner.get_mut(&key) {
            // Temporarily cast away mut for TryFrom<&KeyValue> to match
            let withdraw: RouteCollection = (&*existing).try_into().map_err(|_| {
                KvsError::EncodeError(format!("Could not encode: {}", existing.to_string()))
            })?;
            existing.update(value);
            let announce: RouteCollection = (&*existing).try_into().map_err(|_| {
                KvsError::EncodeError(format!("Could not encode: {}", existing.to_string()))
            })?;
            self.outbound
                .push_back(Update::with_both(announce, withdraw));
        } else {
            let kv = KeyValue::new(key.clone(), value);
            let announce: RouteCollection = (&kv).try_into()?;
            self.inner.insert(key, kv);
            self.outbound.push_back(Update::with_announce(announce));
        }
        Ok(())
    }

    /// Retrieve a [Value](struct.Value.html) by a given &[Key](struct.Key.html)
    pub fn get(&mut self, key: &str) -> Option<String> {
        self.inner.get(key).map(|kv| kv.as_ref().clone())
    }

    /// Remove a [KeyValue](struct.KeyValue.html) by a given &[Key](struct.Key.html)
    pub fn remove(&mut self, key: &str) -> Result<Option<String>, KvsError> {
        if let Some(removed) = self.inner.remove(key) {
            let withdraw: RouteCollection = (&removed).try_into().map_err(|_| {
                KvsError::EncodeError(format!("Could not encode: {}", removed.to_string()))
            })?;
            self.outbound.push_back(Update::with_withdraw(withdraw));
            Ok(Some(removed.into_value()))
        } else {
            Ok(None)
        }
    }

    /*
    /// Process inbound updates (without triggering outbound updates)
    ///
    /// Ordering of [KeyValue](struct.KeyValue.html) updates is important, we should
    /// process the events for matching [Key](struct.Key.html)s in order of the `version` to make sure
    /// the store has the newest [Value](struct.Value.html)
    fn sync(&mut self) -> Result<(), KvsError> {
        let mut updates: HashMap<String, Vec<KeyValue<String, String>>> = HashMap::new();
        while let Some(incoming) = self.inbound.pop_front() {

        }
    }
    */
}

/// A Pending update to be sent to BGP Peers
///
/// - A new [KeyValue](struct.KeyValue.html) will only have an announcement
/// - An updated [KeyValue](struct.KeyValue.html) will announce the new value (and version), will withdraw the old value
/// - A removed [KeyValue](struct.KeyValue.html) will only have a withdraw
struct Update {
    announce: Option<RouteCollection>,
    withdraw: Option<RouteCollection>,
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
    fn test_store_updates() {
        let mut store = KvStore::new();
        assert!(store.is_empty());
        assert!(store.outbound.is_empty());

        store.insert("Key".to_owned(), "Value".to_owned()).unwrap();
        assert_eq!(store.get("Key"), Some("Value".to_owned()));
        assert_eq!(store.outbound.len(), 1);
        assert!(store.outbound.front().unwrap().announce.is_some());
        assert!(store.outbound.front().unwrap().withdraw.is_none());

        store.insert("Key".to_owned(), "42".to_owned()).unwrap();
        assert_eq!(store.get("Key"), Some("42".to_owned()));
        assert_eq!(store.inner.get("Key").unwrap().version(), 1);
        assert_eq!(store.outbound.len(), 2);
        assert!(store.outbound.back().unwrap().announce.is_some());
        assert!(store.outbound.back().unwrap().withdraw.is_some());

        store.remove("Key").unwrap();
        assert_eq!(store.len(), 0);
        assert_eq!(store.get("Key"), None);
        assert_eq!(store.outbound.len(), 3);
        assert!(store.outbound.back().unwrap().announce.is_none());
        assert!(store.outbound.back().unwrap().withdraw.is_some());
    }

    #[test]
    fn test_store_inbound_announce() {
        let mut store = KvStore::new();

        let kv = KeyValue::new("MyKey".to_owned(), "Some Value".to_owned());
        let routes: RouteCollection = (&kv).try_into().unwrap();

        store.inbound.push_back(Update::with_announce(routes));
    }
}
