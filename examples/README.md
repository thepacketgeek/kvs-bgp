Here's a quick demo of getting up & running with Kvs-BGP (using [ExaBGP](https://github.com/Exa-Networks/exabgp) as a remote peer)

## Setting up ExaBGP
I have Exabgp setup locally running on **127.0.0.2:1179** with [this config](./exabgp.ini) and the following:

```sh
$ env exabgp.tcp.port=1179 exabgp.tcp.bind="127.0.0.2" exabgp ./exabgp.ini
```

## Running Kvs-Bgp
Using [this config](./kvs_bgp.toml) and running locally:

```sh
$ cargo run --release -- ./kvs_bgp.toml --bgp-port 1179 -v
```

## Make HTTP API Calls to Kvs-Bgp
Now in another terminal I can use `curl` to test out calls to the HTTP API:

```sh
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

Once these routes have been advertised to ExaBGP (which happens automatically on **insert** and **remove**), you can kill the local `kvs-bgp` service, restart, and your keys will be re-advertised by ExaBGP and `kvs-bgp` will **have your data again** :D

## Simulate incoming updates
For testing, you can simulate incoming updates from another `kvs-bgp` speaker using `exabgpcli` to inject routes into `kvs-bgp`:

These routes will decode to the key "MyKey" and value "Value"
```sh
$ exabgpcli announce route bf51:0:d:12:500::/128 next-hop bf51::3:7911:e0fa:7bea:920b
$ exabgpcli announce route bf51:1:4d79:4b65:790a::/128 next-hop bf51:0:1:3:7911:e0fa:7bea:920b
$ exabgpcli announce route bf51:2:53:6f6d:6520:5661:6c75:6500/128 next-hop bf51:0:2:3:7911:e0fa:7bea:920b
```