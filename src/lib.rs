use std::collections::hash_map::DefaultHasher;
use std::convert::{From, TryFrom};
use std::fmt::{self, Display};
use std::hash::{Hash, Hasher};
use std::net::Ipv6Addr;

use bytes::{BufMut, BytesMut};
use itertools::{chain, enumerate, Itertools};
use thiserror::Error;

const ADDR_PREFIX: [u8; 2] = [0xbf, 0x51]; // BF51 IPv6 Prefix
const CHUNK_SIZE: usize = 96 / 8;

#[derive(Error, Debug)]
pub enum KvsError {
    #[error("Could not decode: {0}")]
    DecodeError(String),
    #[error("Could not encode: {0}")]
    EncodeError(String),
}

#[derive(Debug)]
struct Key {
    value: Vec<u8>,
    hash: u64,
}

impl Key {
    pub fn new(value: Vec<u8>) -> Self {
        let hash = Self::get_hash(&value);
        Self { value, hash }
    }

    pub fn len(&self) -> usize {
        self.value.len()
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.value
    }

    pub fn hash(&self) -> u64 {
        self.hash
    }

    pub fn get_hash(val: &[u8]) -> u64 {
        let mut hasher = DefaultHasher::new();
        val.hash(&mut hasher);
        hasher.finish()
    }
}

impl Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            std::str::from_utf8(&self.value).expect("Invalid key"),
        )
    }
}

impl From<&str> for Key {
    fn from(val: &str) -> Self {
        Self::new(val.as_bytes().into())
    }
}

impl From<&[u8]> for Key {
    fn from(val: &[u8]) -> Self {
        Self::new(val.to_owned())
    }
}

#[derive(Debug)]
struct Value(Vec<u8>);

impl Value {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            std::str::from_utf8(&self.0).expect("Invalid value"),
        )
    }
}

impl From<&str> for Value {
    fn from(val: &str) -> Self {
        Self(val.as_bytes().into())
    }
}

impl From<&[u8]> for Value {
    fn from(val: &[u8]) -> Self {
        Self(val.to_owned())
    }
}

#[derive(Debug)]
struct KeyValue {
    key: Key,
    value: Value,
    version: u16,
}

impl KeyValue {
    pub fn new(key: Key, value: Value) -> Self {
        Self::with_version(key, value, 0)
    }

    pub fn with_version(key: Key, value: Value, version: u16) -> Self {
        Self {
            key,
            value,
            version,
        }
    }

    pub fn update(&mut self, value: Value) {
        self.value = value;
        self.version += 1;
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        [self.key.as_bytes(), self.value.as_bytes()].concat()
    }
}

impl Display for KeyValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} | {}", self.key, self.value)
    }
}

#[derive(Debug)]
struct Prefix(Ipv6Addr);

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
struct NextHop(Ipv6Addr);

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
struct Route {
    prefix: Prefix,
    next_hop: NextHop,
}

impl Route {
    pub fn sequence(&self) -> u16 {
        self.prefix.sequence()
    }
}

#[derive(Debug)]
struct RouteCollection(Vec<Route>);

impl RouteCollection {
    pub fn new(mut routes: Vec<Route>) -> Self {
        routes.sort_by_key(|r| r.sequence());
        Self(routes)
    }
}

impl TryFrom<&KeyValue> for RouteCollection {
    type Error = KvsError;

    fn try_from(kv: &KeyValue) -> Result<Self, Self::Error> {
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
            next_hop_buf.put_u64(kv.key.hash());
            let next_hop: NextHop = (&next_hop_buf).into();
            next_hop_buf.clear();

            routes.push(Route { prefix, next_hop });
        }
        Ok(RouteCollection(routes))
    }
}

impl TryFrom<&RouteCollection> for KeyValue {
    type Error = KvsError;

    fn try_from(routes: &RouteCollection) -> Result<Self, Self::Error> {
        let first = routes
            .0
            .first()
            .ok_or_else(|| KvsError::DecodeError(format!("At least one route should exist")))?;

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
        let kv = Self::with_version(
            key.into(),
            value.into(),
            version.expect("Missing a version"),
        );
        Ok(kv)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::TryInto;

    #[test]
    fn round_trip() {
        let kv = KeyValue::new("MyKey".into(), "This is a pretty long value".into());
        let routes: RouteCollection = (&kv).try_into().unwrap();

        let kv2: KeyValue = (&routes).try_into().unwrap();
        assert_eq!(kv.key.hash(), kv2.key.hash());
        assert_eq!(kv.key.to_string(), kv2.key.to_string());
        assert_eq!(kv.value.to_string(), kv2.value.to_string());
    }
}
