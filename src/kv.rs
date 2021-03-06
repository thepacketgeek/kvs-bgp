use std::collections::hash_map::DefaultHasher;
use std::convert::{AsRef, From, TryFrom};
use std::fmt::{self, Debug, Display};
use std::hash::{Hash, Hasher};
use std::net::{IpAddr, Ipv6Addr};

use bgp_rs::{Identifier, NLRIEncoding, PathAttribute, Update};
use bytes::{BufMut, BytesMut};
use itertools::{chain, enumerate, Itertools};
use serde::{de::DeserializeOwned, Serialize};

use crate::KvsError;

const ADDR_PREFIX: [u8; 2] = [0xbf, 0x51]; // BF51 IPv6 Prefix
const CHUNK_SIZE: usize = 96 / 8;

/// `Key` ID for the Key/Value Store
///
/// Must be Hashable as it's used as a key in HashTable
/// and (De)Serializable for sending/receiving on the wire
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
    /// Create a new [Key](struct.Key.html) with the given key item
    pub fn new(key: K) -> Self {
        Self { inner: key }
    }

    fn len(&self) -> usize {
        self.as_bytes().len()
    }

    fn as_bytes(&self) -> Vec<u8> {
        bincode::serialize(&self.inner).expect("Can encode")
    }

    fn get_hash(&self) -> u64 {
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

/// `Value` of a Key/Value pair
///
/// Must be (De)Serializable for sending/receiving on the wire
#[derive(Debug)]
pub struct Value<V>(V)
where
    V: Debug + Display + Serialize + DeserializeOwned;

impl<V> Value<V>
where
    V: Debug + Display + Serialize + DeserializeOwned,
{
    /// Create a new [Key](struct.Key.html) with the given key item
    pub fn new(value: V) -> Self {
        Self(value)
    }

    fn len(&self) -> usize {
        self.as_bytes().len()
    }

    fn as_bytes(&self) -> Vec<u8> {
        bincode::serialize(&self.0).expect("Can encode")
    }

    fn into_value(self) -> V {
        self.0
    }
}

impl<V> AsRef<V> for Value<V>
where
    V: Debug + Display + Serialize + DeserializeOwned,
{
    #[inline]
    fn as_ref(&self) -> &V {
        &self.0
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

/// A [KeyValue](struct.KeyValue.html) pair, stored internally as a value in the [KvStore](struct.KvStore.html) HashMap
///
/// Keeps track of the key hash for checksum & comparison, along with a version
/// that increments each time the value is updated
///    (for evicting aged out versions locally and syncing remote peers)
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
    /// Create a new [KeyValue](struct.KeyValue.html) pair by values for K, V
    pub fn new(key: K, value: V) -> Self {
        Self::with_version(key, value, 0)
    }

    /// Get a ref to the `KeyValue` `Key`
    pub fn key(&self) -> &K {
        &self.key.inner
    }

    fn with_version(key: K, value: V, version: u16) -> Self {
        let _key = Key::new(key);
        let hash = _key.get_hash();
        Self {
            key: _key,
            value: Value::new(value),
            hash,
            version,
        }
    }

    /// Replace the current `Value` and increment the [KeyValue](struct.KeyValue.html) version
    pub fn update(&mut self, value: V) {
        self.value = Value::new(value);
        self.version += 1;
    }

    fn as_bytes(&self) -> Vec<u8> {
        [self.key.as_bytes(), self.value.as_bytes()].concat()
    }

    /// Calculate the number of [Route](struct.Route.html)s needed to encode
    /// this `KeyValue` pair
    pub fn number_of_routes(&self) -> usize {
        // Sum the length fields and the length of key & value,
        // divided by 96 bits per `Prefix`
        ((self.key.len() + self.value.len() + 4) as f32 / CHUNK_SIZE as f32).ceil() as usize
    }

    fn key_hash(&self) -> u64 {
        self.hash
    }

    /// The version of this `KeyValue` (incremented for every update)
    pub fn version(&self) -> u16 {
        self.version
    }

    /// Extract the value from this `KeyValue`, consuming this struct
    pub fn into_value(self) -> V {
        self.value.into_value()
    }
}

impl<K, V> AsRef<V> for KeyValue<K, V>
where
    K: Debug + Display + Hash + Serialize + DeserializeOwned,
    V: Debug + Display + Serialize + DeserializeOwned,
{
    #[inline]
    fn as_ref(&self) -> &V {
        self.value.as_ref()
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

/// An IPv6 Unicast Prefix to encode a portion of a [KeyValue](struct.KeyValue.html) pair
#[derive(Clone, Debug)]
pub struct Prefix(Ipv6Addr);

impl Prefix {
    /// [Route](struct.Route.html) sequence for this [KeyValue](struct.KeyValue.html)
    fn sequence(&self) -> u16 {
        self.0.segments()[1]
    }

    // fn data(&self) -> &[u8] {
    //     &self.0.octets()[2..]
    // }
}

impl AsRef<Ipv6Addr> for Prefix {
    #[inline]
    fn as_ref(&self) -> &Ipv6Addr {
        &self.0
    }
}

impl From<&BytesMut> for Prefix {
    fn from(bytes: &BytesMut) -> Self {
        Self(octets_to_ip(&bytes[..16]))
    }
}

impl From<&Prefix> for IpAddr {
    fn from(prefix: &Prefix) -> Self {
        IpAddr::V6(prefix.0)
    }
}

/// An IPv6 Unicast Next Hop to encode details about a [KeyValue](struct.KeyValue.html) pair
#[derive(Clone, Debug)]
pub struct NextHop(Ipv6Addr);

impl NextHop {
    /// The version of this `KeyValue` (incremented for every update)
    pub fn version(&self) -> u16 {
        self.0.segments()[1]
    }

    /// [Route](struct.Route.html) sequence for this [KeyValue](struct.KeyValue.html)
    pub fn sequence(&self) -> u16 {
        self.0.segments()[2]
    }

    /// The encoded number of routes for the encoded `KeyValue`
    fn collection_length(&self) -> u16 {
        self.0.segments()[3]
    }

    /// The `Key` hash for this [KeyValue](struct.KeyValue.html)
    fn hash(&self) -> u64 {
        let mut data = [0u8; 8];
        data.copy_from_slice(&self.0.octets()[8..]);
        u64::from_be_bytes(data)
    }
}

impl AsRef<Ipv6Addr> for NextHop {
    #[inline]
    fn as_ref(&self) -> &Ipv6Addr {
        &self.0
    }
}

impl From<&BytesMut> for NextHop {
    fn from(bytes: &BytesMut) -> Self {
        Self(octets_to_ip(&bytes[..16]))
    }
}

impl From<&NextHop> for IpAddr {
    fn from(next_hop: &NextHop) -> Self {
        IpAddr::V6(next_hop.0)
    }
}

/// One of many [Prefix](struct.Prefix.html)/[NextHop](struct.NextHop.html) pairs used to encode a [KeyValue](struct.KeyValue.html) pair in BGP Messages
///
/// Collected in sequential order as a `RouteCollection` for encoding & decoding
#[derive(Clone, Debug)]
pub struct Route {
    /// BGP Update IPv6 Prefix to advertise (assumed /128 mask)
    pub prefix: Prefix,
    /// BGP Update IPv6 NextHop to advertise
    pub next_hop: NextHop,
}

impl Route {
    /// Create a `Route` object from prefix & next_hop `Ipv6Addr`s
    pub fn from_addrs(prefix: Ipv6Addr, next_hop: Ipv6Addr) -> Self {
        Self {
            prefix: Prefix(prefix),
            next_hop: NextHop(next_hop),
        }
    }

    /// Determine if this has a BF51 prefix
    fn has_valid_prefix(&self) -> bool {
        ADDR_PREFIX[..] == self.prefix.0.octets()[..2]
            && ADDR_PREFIX[..] == self.next_hop.0.octets()[..2]
    }

    pub fn hash(&self) -> u64 {
        self.next_hop.hash()
    }

    pub fn collection_length(&self) -> usize {
        self.next_hop.collection_length() as usize
    }

    fn sequence(&self) -> u16 {
        self.prefix.sequence()
    }
}

// This needs some major cleanup, it's a pain to do all the matching for BGP Update PathAttributes
impl TryFrom<&Update> for Route {
    type Error = KvsError;

    fn try_from(update: &Update) -> Result<Self, Self::Error> {
        if let Some(PathAttribute::MP_REACH_NLRI(mp_reach)) = update.get(Identifier::MP_REACH_NLRI)
        {
            if let Some(nlri) = mp_reach.announced_routes.first() {
                if let NLRIEncoding::IP(prefix) = nlri {
                    let addr: IpAddr = prefix.into();
                    if let IpAddr::V6(v6) = addr {
                        let next_hop = octets_to_ip(&mp_reach.next_hop);
                        let route = Route::from_addrs(v6, next_hop);
                        if route.has_valid_prefix() {
                            return Ok(route);
                        }
                    }
                }
            }
            return Err(KvsError::NotAKvsRoute);
        } else if let Some(PathAttribute::MP_UNREACH_NLRI(mp_unreach)) =
            update.get(Identifier::MP_UNREACH_NLRI)
        {
            // These are KeyValue pairs removed from remote servers
            // Collect and remove from local store
            let next_hop: Option<Ipv6Addr> =
                if let Some(PathAttribute::NEXT_HOP(next_hop)) = update.get(Identifier::NEXT_HOP) {
                    if let IpAddr::V6(v6) = next_hop {
                        Some(*v6)
                    } else {
                        None
                    }
                } else {
                    None
                };
            let next_hop = if let Some(next_hop) = next_hop {
                next_hop
            } else {
                return Err(KvsError::NotAKvsRoute);
            };
            if let Some(nlri) = mp_unreach.withdrawn_routes.first() {
                if let NLRIEncoding::IP(prefix) = nlri {
                    let addr: IpAddr = prefix.into();
                    if let IpAddr::V6(v6) = addr {
                        let route = Route::from_addrs(v6, next_hop);
                        if route.has_valid_prefix() {
                            return Ok(route);
                        }
                    }
                }
            }
            return Err(KvsError::NotAKvsRoute);
        }
        Err(KvsError::NotAKvsRoute)
    }
}

/// Represents one [KeyValue](struct.KeyValue.html) as a collection of IPv6 Unicast Routes
#[derive(Debug)]
pub struct RouteCollection(Vec<Route>);

impl RouteCollection {
    /// Construct a `RouteCollection` from a vec of `Route`s
    pub fn from_routes(mut routes: Vec<Route>) -> Self {
        routes.sort_by_key(|r| r.sequence());
        Self(routes)
    }

    /// Iterate through contained routes in sorted order (by sequence number)
    pub fn iter(&self) -> impl Iterator<Item = &Route> {
        self.0.iter()
    }
}

impl<K, V> TryFrom<&KeyValue<K, V>> for RouteCollection
where
    K: Debug + Display + Hash + Serialize + DeserializeOwned,
    V: Debug + Display + Serialize + DeserializeOwned,
{
    type Error = KvsError;

    fn try_from(kv: &KeyValue<K, V>) -> Result<Self, Self::Error> {
        let num_routes = kv.number_of_routes();
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
            next_hop_buf.put_u16(num_routes as u16);
            next_hop_buf.put_u64(kv.key_hash());
            let next_hop: NextHop = (&next_hop_buf).into();
            next_hop_buf.clear();

            routes.push(Route { prefix, next_hop });
        }
        Ok(RouteCollection::from_routes(routes))
    }
}

impl<K, V> TryFrom<&RouteCollection> for KeyValue<K, V>
where
    K: Debug + Display + Hash + Serialize + DeserializeOwned,
    V: Debug + Display + Serialize + DeserializeOwned,
{
    type Error = KvsError;

    fn try_from(routes: &RouteCollection) -> Result<Self, Self::Error> {
        let first = routes
            .0
            .first()
            .ok_or_else(|| KvsError::DecodeError("At least one route should exist".to_owned()))?;

        let key_length = first.prefix.0.segments()[2];
        let val_length = first.prefix.0.segments()[3];
        let mut bytes: Vec<u8> = Vec::with_capacity((key_length + val_length) as usize);

        let mut version: Option<u16> = None;
        let mut hash: Option<u64> = None;

        for (i, route) in routes.0.iter().enumerate() {
            if !route.has_valid_prefix() {
                return Err(KvsError::DecodeError("Not a KVS-BGP Prefix".to_owned()));
            }
            if route.sequence() != i as u16 {
                return Err(KvsError::DecodeError(format!(
                    "Missing route sequence # {}",
                    i
                )));
            }
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
        let version = version.ok_or_else(|| KvsError::DecodeError("Missing version".to_owned()))?;
        let key = bincode::deserialize(&key)
            .map_err(|_e| KvsError::DecodeError("Couldn't decode key".to_owned()))?;
        let value = bincode::deserialize(&value)
            .map_err(|_e| KvsError::DecodeError("Couldn't decode value".to_owned()))?;
        let kv = Self::with_version(key, value, version);
        Ok(kv)
    }
}

/// Convert a [u8] slice (with at least 16 x u8) into an Ipv6 addr
#[inline]
fn octets_to_ip(bytes: &[u8]) -> Ipv6Addr {
    let mut octets = [0u8; 16];
    octets.copy_from_slice(&bytes[..16]);
    Ipv6Addr::from(octets)
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
        assert_eq!(kv1.number_of_routes(), 2);

        let kv2 = KeyValue::new(
            "myKey".to_owned(),
            "This is a really long value that should use a few more routes than the last"
                .to_owned(),
        );
        assert_eq!(kv2.number_of_routes(), 9);
    }

    #[test]
    fn test_key_value_update() {
        let mut kv = KeyValue::new("myKey".to_owned(), 42);
        assert_eq!(kv.version(), 0);

        kv.update(24);
        assert_eq!(kv.version(), 1);
        assert_eq!(kv.value.as_ref(), &24);
    }

    #[test]
    fn round_trip() {
        let kv = KeyValue::new("MyKey".to_owned(), "Some Value".to_owned());
        let routes: RouteCollection = (&kv).try_into().unwrap();
        let kv2: KeyValue<String, String> = (&routes).try_into().unwrap();
        assert_eq!(kv.key_hash(), kv2.key_hash());
        assert_eq!(kv.key.to_string(), kv2.key.to_string());
        assert_eq!(kv.value.to_string(), kv2.value.to_string());
    }

    #[test]
    fn has_valid_prefix() {
        let route = Route {
            prefix: Prefix("BF51:10::2".parse().unwrap()),
            next_hop: NextHop("bf51:A::2".parse().unwrap()),
        };
        assert!(route.has_valid_prefix());
        let route = Route {
            prefix: Prefix("2001:10::2".parse().unwrap()),
            next_hop: NextHop("bf51:A::2".parse().unwrap()),
        };
        assert!(!route.has_valid_prefix());
    }

    #[test]
    fn missing_route() {
        let kv = KeyValue::new(
            "MyKey".to_owned(),
            "Something longer that needs multiple routes".to_owned(),
        );
        let routes: Vec<_> = {
            let rc: RouteCollection = (&kv).try_into().unwrap();
            rc.0
        };
        let missing_rc = RouteCollection::from_routes([&routes[0..3], &routes[4..]].concat());
        let kv2: Result<KeyValue<String, String>, _> = (&missing_rc).try_into();
        assert!(kv2.is_err());
    }
}
