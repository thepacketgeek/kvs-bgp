use std::collections::hash_map::DefaultHasher;
use std::convert::{From, TryFrom};
use std::fmt::{self, Debug, Display};
use std::hash::{Hash, Hasher};
use std::net::Ipv6Addr;

use bincode;
use bytes::{BufMut, BytesMut};
use itertools::{chain, enumerate, Itertools};
use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

const ADDR_PREFIX: [u8; 2] = [0xbf, 0x51]; // BF51 IPv6 Prefix
const CHUNK_SIZE: usize = 96 / 8;

#[derive(Error, Debug)]
pub enum KvError {
    #[error("Could not decode: {0}")]
    DecodeError(String),
    #[error("Could not encode: {0}")]
    EncodeError(String),
}

#[derive(Debug)]
pub struct Key<K>
where
    K: Debug + Display + Hash + Serialize + DeserializeOwned,
{
    inner: K,
}

impl<K> Key<K>
where
    K: Debug + Display + Hash + Serialize + DeserializeOwned,
{
    pub fn new(key: K) -> Self {
        Self { inner: key }
    }

    pub fn len(&self) -> usize {
        self.as_bytes().len()
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        bincode::serialize(&self.inner).expect("Can encode")
    }

    pub fn get_hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.inner.hash(&mut hasher);
        hasher.finish()
    }
}

impl<K> Display for Key<K>
where
    K: Debug + Display + Hash + Serialize + DeserializeOwned,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.inner)
    }
}

#[derive(Debug)]
pub struct Value<V>(V)
where
    V: Debug + Display + Serialize + DeserializeOwned;

impl<V> Value<V>
where
    V: Debug + Display + Serialize + DeserializeOwned,
{
    pub fn new(value: V) -> Self {
        Self(value)
    }

    pub fn len(&self) -> usize {
        self.as_bytes().len()
    }

    pub fn value(&self) -> &V {
        &self.0
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        bincode::serialize(&self.0).expect("Can encode")
    }
}

impl<V> Display for Value<V>
where
    V: Debug + Display + Serialize + DeserializeOwned,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0,)
    }
}

#[derive(Debug)]
pub struct KeyValue<K, V>
where
    K: Debug + Display + Hash + Serialize + DeserializeOwned,
    V: Debug + Display + Serialize + DeserializeOwned,
{
    key: Key<K>,
    value: Value<V>,
    hash: u64,
    version: u16,
}

impl<K, V> KeyValue<K, V>
where
    K: Debug + Display + Hash + Serialize + DeserializeOwned,
    V: Debug + Display + Serialize + DeserializeOwned,
{
    pub fn new(key: K, value: V) -> Self {
        Self::with_version(key, value, 0)
    }

    pub fn with_version(key: K, value: V, version: u16) -> Self {
        let _key = Key::new(key);
        let hash = _key.get_hash();
        Self {
            key: _key,
            value: Value::new(value),
            hash,
            version,
        }
    }

    pub fn update(&mut self, value: V) {
        self.value = Value::new(value);
        self.version += 1;
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        [self.key.as_bytes(), self.value.as_bytes()].concat()
    }

    pub fn key_hash(&self) -> u64 {
        self.hash
    }

    pub fn version(&self) -> u16 {
        self.version
    }

    pub fn value(&self) -> &V {
        self.value.value()
    }
}

impl<K, V> Display for KeyValue<K, V>
where
    K: Debug + Display + Hash + Serialize + DeserializeOwned,
    V: Debug + Display + Serialize + DeserializeOwned,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} | {}", self.key, self.value)
    }
}

#[derive(Debug)]
pub struct Prefix(Ipv6Addr);

impl Prefix {
    pub fn sequence(&self) -> u16 {
        self.0.segments()[1]
    }

    // pub fn data(&self) -> &[u8] {
    //     &self.0.octets()[2..]
    // }
}

impl From<&BytesMut> for Prefix {
    fn from(bytes: &BytesMut) -> Self {
        let prefix = Ipv6Addr::from([
            bytes[0], bytes[1], bytes[3], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        ]);
        Self(prefix)
    }
}

#[derive(Debug)]
pub struct NextHop(Ipv6Addr);

impl NextHop {
    pub fn version(&self) -> u16 {
        self.0.segments()[1]
    }

    pub fn sequence(&self) -> u16 {
        self.0.segments()[2]
    }

    pub fn hash(&self) -> u64 {
        let data = &self.0.octets()[8..];
        u64::from_be_bytes([
            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
        ])
    }
}

impl From<&BytesMut> for NextHop {
    fn from(bytes: &BytesMut) -> Self {
        let next_hop = Ipv6Addr::from([
            bytes[0], bytes[1], bytes[3], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        ]);
        Self(next_hop)
    }
}

#[derive(Debug)]
pub struct Route {
    prefix: Prefix,
    next_hop: NextHop,
}

impl Route {
    pub fn sequence(&self) -> u16 {
        self.prefix.sequence()
    }
}

/// Represents one `KeyValue` as a collection of IPv6 Unicast Routes
///
/// Each `KeyValue` can represent 6_5535 * 96 bytes (6_291_360 bytes)
#[derive(Debug)]
pub struct RouteCollection(Vec<Route>);

impl RouteCollection {
    pub fn new(mut routes: Vec<Route>) -> Self {
        routes.sort_by_key(|r| r.sequence());
        Self(routes)
    }
}

impl<K, V> TryFrom<&KeyValue<K, V>> for RouteCollection
where
    K: Debug + Display + Hash + Serialize + DeserializeOwned,
    V: Debug + Display + Serialize + DeserializeOwned,
{
    type Error = KvError;

    fn try_from(kv: &KeyValue<K, V>) -> Result<Self, Self::Error> {
        let num_routes =
            f32::from(((kv.key.len() + kv.value.len()) as f32) / 96f32).ceil() as usize;
        let mut routes: Vec<Route> = Vec::with_capacity(num_routes);

        // Encode the K/V lengths for the first prefix
        let lengths = [
            (kv.key.len() as u16).to_be_bytes(),
            (kv.value.len() as u16).to_be_bytes(),
        ]
        .concat();

        let mut prefix_buf = BytesMut::with_capacity(128);
        let mut next_hop_buf = BytesMut::with_capacity(128);

        for (i, chunk) in enumerate(&chain(lengths.iter(), kv.as_bytes().iter()).chunks(CHUNK_SIZE))
        {
            prefix_buf.put(&ADDR_PREFIX[..]);
            prefix_buf.put_u16(i as u16);
            let mut remaining = CHUNK_SIZE;
            for byte in chunk {
                prefix_buf.put_u8(*byte);
                remaining -= 1;
            }
            for _ in 0..remaining {
                // Pad remaining bytes
                prefix_buf.put_u8(0);
            }
            let prefix: Prefix = (&prefix_buf).into();
            prefix_buf.clear();

            next_hop_buf.put(&ADDR_PREFIX[..]);
            next_hop_buf.put_u16(kv.version);
            next_hop_buf.put_u16(i as u16);
            next_hop_buf.put_u16(0u16); // Reserved
            next_hop_buf.put_u64(kv.key_hash());
            let next_hop: NextHop = (&next_hop_buf).into();
            next_hop_buf.clear();

            routes.push(Route { prefix, next_hop });
        }
        Ok(RouteCollection(routes))
    }
}

impl<K, V> TryFrom<&RouteCollection> for KeyValue<K, V>
where
    K: Debug + Display + Hash + Serialize + DeserializeOwned,
    V: Debug + Display + Serialize + DeserializeOwned,
{
    type Error = KvError;

    fn try_from(routes: &RouteCollection) -> Result<Self, Self::Error> {
        let first = routes
            .0
            .first()
            .ok_or_else(|| KvError::DecodeError(format!("At least one route should exist")))?;

        let key_length = first.prefix.0.segments()[2];
        let val_length = first.prefix.0.segments()[3];
        let mut bytes: Vec<u8> = Vec::with_capacity((key_length + val_length) as usize);

        let mut version: Option<u16> = None;
        let mut hash: Option<u64> = None;

        for (i, route) in routes.0.iter().enumerate() {
            if i == 0 {
                version.replace(route.next_hop.version());
                hash.replace(route.next_hop.hash());
                bytes.extend_from_slice(&route.prefix.0.octets()[8..]);
            } else {
                bytes.extend_from_slice(&route.prefix.0.octets()[4..]);
            }
        }

        let (key, bytes) = bytes.split_at(key_length as usize);
        let (value, _) = bytes.split_at(val_length as usize);
        let version = version.ok_or_else(|| KvError::DecodeError(format!("Missing version")))?;
        let key = bincode::deserialize(&key)
            .map_err(|_e| KvError::DecodeError(format!("Couldn't decode key")))?;
        let value = bincode::deserialize(&value)
            .map_err(|_e| KvError::DecodeError(format!("Couldn't decode value")))?;
        let kv = Self::with_version(key, value, version);
        Ok(kv)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::TryInto;

    #[test]
    fn test_key() {
        let k1 = Key::new("Test".to_owned());
        assert_eq!(
            k1.as_bytes(),
            vec![4, 0, 0, 0, 0, 0, 0, 0, 84, 101, 115, 116]
        );
        let k2 = Key::new(42);
        assert_eq!(k2.as_bytes(), vec![42, 0, 0, 0]);
        assert_eq!(&k2.to_string(), "42");
    }

    #[test]
    fn test_value() {
        let v1 = Value::new("Test".to_owned());
        assert_eq!(
            v1.as_bytes(),
            vec![4, 0, 0, 0, 0, 0, 0, 0, 84, 101, 115, 116]
        );
        let v2 = Value::new(42);
        assert_eq!(v2.as_bytes(), vec![42, 0, 0, 0]);
        assert_eq!(&v2.to_string(), "42");
    }

    #[test]
    fn test_key_value() {
        let kv1 = KeyValue::new("myKey".to_owned(), 42);
        assert_eq!(
            kv1.as_bytes(),
            vec![5, 0, 0, 0, 0, 0, 0, 0, 109, 121, 75, 101, 121, 42, 0, 0, 0]
        );
        assert_eq!(&kv1.to_string(), "myKey | 42");
    }

    #[test]
    fn test_key_value_update() {
        let mut kv = KeyValue::new("myKey".to_owned(), 42);
        assert_eq!(kv.version(), 0);

        kv.update(24);
        assert_eq!(kv.version(), 1);
        // assert_eq!(kv.value, 24);
    }

    #[test]
    fn round_trip() {
        let kv = KeyValue::new("MyKey".to_owned(), "This is a pretty long value".to_owned());
        let routes: RouteCollection = (&kv).try_into().unwrap();

        let kv2: KeyValue<String, String> = (&routes).try_into().unwrap();
        assert_eq!(kv.key_hash(), kv2.key_hash());
        assert_eq!(kv.key.to_string(), kv2.key.to_string());
        assert_eq!(kv.value.to_string(), kv2.value.to_string());
    }
}
