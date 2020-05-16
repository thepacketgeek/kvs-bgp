use std::sync::Arc;
use std::net::IpAddr;
use structopt::StructOpt;

use env_logger::Builder;
use log::{LevelFilter, info};
use tokio::sync::Mutex;
use warp::{self, Filter};

use kvs_bgp::{api, store::KvStore};


#[derive(StructOpt, Debug)]
#[structopt(
    name = "kvs_bgp",
    about = env!("CARGO_PKG_DESCRIPTION"),
    version = env!("CARGO_PKG_VERSION"),
    author = env!("CARGO_PKG_AUTHORS"),
    rename_all = "kebab-case")
]
/// KVS-BGP Server
pub struct Args {
    #[structopt(short, long, default_value = "127.0.0.1")]
    address: IpAddr,
    /// Host port to use for HTTP API
    #[structopt(short, long, default_value = "3030")]
    port: u16,
    /// Show debug logs (additive for trace logs)
    #[structopt(short, parse(from_occurrences))]
    pub verbose: u8,
}


#[tokio::main]
async fn main() {
    let args = Args::from_args();

    let (kvs_level, other_level) = match args.verbose {
        0 => (LevelFilter::Info, LevelFilter::Warn),
        1 => (LevelFilter::Debug, LevelFilter::Warn),
        2 => (LevelFilter::Trace, LevelFilter::Warn),
        3 | _ => (LevelFilter::Trace, LevelFilter::Trace),
    };
    Builder::new()
        .filter(Some("kvs_bgp"), kvs_level)
        .filter(None, other_level)
        .init();
    info!("Logging at levels {}/{}", kvs_level, other_level);

    let kv_store = Arc::new(Mutex::new(KvStore::new()));
    let state = warp::any().map(move || kv_store.clone());

    let status = warp::path!("status").map(|| "Alive!\n".to_owned());

    let get_key = warp::get()
        .and(warp::path!("get" / String))
        .and(warp::path::end())
        .and(state.clone())
        .and_then(api::get_key);

    let insert_key = warp::put()
        .and(warp::path!("insert" / String / String))
        .and(warp::path::end())
        .and(state.clone())
        .and_then(api::insert_pair);

    let remove = warp::delete()
        .and(warp::path!("remove" / String))
        .and(warp::path::end())
        .and(state.clone())
        .and_then(api::remove_pair);

    let routes = status.or(get_key).or(insert_key).or(remove);

    info!("Starting HTTP API on {}:{}", args.address, args.port);
    warp::serve(routes).run((args.address, args.port)).await;
}
