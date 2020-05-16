# KVS-BGP
A Key/Value store that allows for eventually consistent, distributed synchronization using BGP

## Why BGP?
- BGP is a reliable way to distribute bytes over an arbitrary number of participating peers
  - Using eBGP, we can guarantee every peer will have a consistent view of the data
  - As long as one peer remains online, your data will be around
- Community support for adding categories to `KeyValue` pairs
  - Allows participating nodes to Pub/Sub to specific categories
  - Use BGP Policy to filter inbound/outbound synchronization of categories
- Many tools exist to inject the BGP routes into a peer mesh
  - E.g. [exabgp](https://github.com/Exa-Networks/exabgp), [gobgp](https://github.com/osrg/gobgp), [bgpd-rs](https://github.com/thepacketgeek/bgpd-rs)

## HTTP API for KeyValue CRUD
Run the HTTP API with:
```
$ cargo run -- --port 8179
```

Then use your favorite HTTP client to make requests:
```
$ curl http://localhost:8179/insert/name/Mat --request PUT
$ curl http://localhost:8179/get/name
Mat
$ curl http://localhost:8179/insert/favorite::protocol/BGP --request PUT
$ curl http://localhost:8179/insert/favorite::food/Pizza --request PUT
$ curl http://localhost:8179/insert/favorite::drink/Scotch --request PUT
$ curl http://localhost:8179/get/favorite::drink
Scotch
$ curl http://localhost:8179/get/favorite::protocol
BGP
$ curl http://localhost:8179/get/favorite::food
Pizza
```

## Key/Value API
The current API is just PoC and should likely replicate other successful KeyValue APIs to work with existing clients (Eg. Redis)

# Internal representation of `KeyValue` pairs

Supports encoding/decoding pairs as BGP update messages using IPv6 Unicast `Prefix` & `NextHop`

## `KeyValue` Pairs
Each `KeyValue` is allowed ~**768 Kbytes** (65,535 * 96 bits). Data
is serialized as `Prefix`es with sorted sequence numbers.

## `Prefix` encoding is as follows:

First prefix for a `KeyValue` pair:
```ignore
bits: | 16 :  16    :    16      :     16       :      64        |
addr: |BF51: seq #  : key length : value length :     data       | /128
```

Subsequent prefixes for a `KeyValue` pair:

```ignore
bits: | 16 :  16    :                   96                       |
addr: |BF51: seq #  :                  data                      | /128
```

### Notes:
- BF51 Prefix
- Used for easy identification and to make sure this
    doesn't clobber public routes
- Sequence Number
- Provides ordering for data decoding and creates unique routes
    so best-path selection doesn't filter prefixes
- Allows for 65_535 prefixes per `KeyValue` pair, and given 12 bytes per prefix
    provides ~768 Kb per `KeyValue` pair
- Data
- Serialized to bytes using [Serde](https://github.com/serde-rs/serde) with [bincode](https://github.com/servo/bincode) serialization


## `NextHop` encoding is as follows:

```ignore
bits: | 16 :   16    :  16   :  16  :          64                |
addr: |BF51: version : seq # : rsvd :       key hash             | /128
```

### Notes:
- BF51 Prefix
- Used for easy identification and to make sure this
    doesn't clobber public routes
- Version
- Encoding of the `KeyValue` version number
- During convergence of an updated `KeyValue` pair, will provide unique Prefix/NextHop route
    so bytes of different versions aren't interlaced together
- Sequence Number
- Provides ordering for data decoding and creates unique routes
    so best-path selection doesn't filter prefixes
- Reserved
- Not currently used
- Key Hash
- Hash of the `KeyValue` `Key`, to differentiate this `NextHop` from other `KeyValue` `NextHop`s

## Example
The `KeyValue` pair "MyKey" : "Some Value" would be represented as:
```ignore
| Seq # | Prefix                                   | NextHop                              |
| 0     | BF51:0:D:12:500::                   /128 | BF51::7911:E0FA:7BEA:920B       /128 |
| 1     | BF51:1:4D79:4B65:790A::             /128 | BF51:0:1:0:7911:E0FA:7BEA:920B  /128 |
| 2     | BF51:2:53:6F6D:6520:5661:6C75:6500  /128 | BF51:0:2:0:7911:E0FA:7BEA:920B  /128 |
```