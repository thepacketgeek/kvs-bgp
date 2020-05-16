//! # Internal representation of [KeyValue](struct.KeyValue.html) pairs
//!
//! Supports encoding/decoding pairs as BGP update messages using IPv6 Unicast [Prefix](struct.Prefix.html) & [NextHop](struct.NextHop.html)
//!
//! ## [KeyValue](struct.KeyValue.html) Pairs
//! Each [KeyValue](struct.KeyValue.html) is allowed ~**768 Kbytes** (65,535 * 96 bits). Data
//! is serialized as [Prefix](struct.Prefix.html)es with sorted sequence numbers.
//!
//! ## [Prefix](struct.Prefix.html) encoding is as follows:
//!
//! First prefix for a [KeyValue](struct.KeyValue.html) pair:
//! ```ignore
//! bits: | 16 :  16    :    16      :     16       :      64        |
//! addr: |BF51: seq #  : key length : value length :     data       | /128
//! ```
//!
//! Subsequent prefixes for a [KeyValue](struct.KeyValue.html) pair:
//!
//! ```ignore
//! bits: | 16 :  16    :                   96                       |
//! addr: |BF51: seq #  :                  data                      | /128
//! ```
//!
//! ### Notes:
//! - BF51 Prefix
//!   - Used for easy identification and to make sure this
//!     doesn't clobber public routes
//! - Sequence Number
//!   - Provides ordering for data decoding and creates unique routes
//!     so best-path selection doesn't filter prefixes
//!   - Allows for 65_535 prefixes per [KeyValue](struct.KeyValue.html) pair, and given 12 bytes per prefix
//!     provides ~768 Kb per [KeyValue](struct.KeyValue.html) pair
//! - Data
//!   - Serialized to bytes using [Serde](https://github.com/serde-rs/serde) with [bincode](https://github.com/servo/bincode) serialization
//!
//!
//! ## [NextHop](struct.NextHop.html) encoding is as follows:
//!
//! ```ignore
//! bits: | 16 :   16    :  16   :  16  :          64                |
//! addr: |BF51: version : seq # : rsvd :       key hash             | /128
//! ```
//!
//! ### Notes:
//! - BF51 Prefix
//!   - Used for easy identification and to make sure this
//!     doesn't clobber public routes
//! - Version
//!   - Encoding of the [KeyValue](struct.KeyValue.html) version number
//!   - During convergence of an updated [KeyValue](struct.KeyValue.html) pair, will provide unique Prefix/NextHop route
//!     so bytes of different versions aren't interlaced together
//! - Sequence Number
//!   - Provides ordering for data decoding and creates unique routes
//!     so best-path selection doesn't filter prefixes
//! - Reserved
//!   - Not currently used
//! - Key Hash
//!   - Hash of the [KeyValue](struct.KeyValue.html) [Key](struct.Key.html), to differentiate this [NextHop](struct.NextHop.html) from other [KeyValue](struct.KeyValue.html) [NextHop](struct.NextHop.html)s
//!
//! ## Example
//! The [KeyValue](struct.KeyValue.html) pair "MyKey" : "Some Value" would be represented as:
//! ```ignore
//! | Seq # | Prefix                                   | NextHop                              |
//! | 0     | BF51:0:D:12:500::                   /128 | BF51::7911:E0FA:7BEA:920B       /128 |
//! | 1     | BF51:1:4D79:4B65:790A::             /128 | BF51:0:1:0:7911:E0FA:7BEA:920B  /128 |
//! | 2     | BF51:2:53:6F6D:6520:5661:6C75:6500  /128 | BF51:0:2:0:7911:E0FA:7BEA:920B  /128 |
//! ```

/// Internal `KeyValue` representations for Encoding/Decoding as BGP Updates
pub mod kv;
#[warn(missing_docs)]

/// In-memory Key/Value store that stores `KeyValue` pairs and synchronizes with BGP peers
pub mod store;

use thiserror::Error;

/// Main error for Kvs library
#[derive(Error, Debug)]
pub enum KvsError {
    #[error("Could not decode: {0}")]
    DecodeError(String),
    #[error("Could not encode: {0}")]
    EncodeError(String),
}
