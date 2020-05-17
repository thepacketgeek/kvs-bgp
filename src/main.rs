use std::error::Error;
use std::net::IpAddr;
use std::sync::Arc;
use structopt::StructOpt;

use env_logger::Builder;
use log::{info, LevelFilter};
use tokio::sync::{mpsc, RwLock};

use kvs_bgp::{api, peering::BgpPeerings, store::KvStore};

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
    /// BGPd config file for peering details
    config_path: String,
    /// Host address to use for HTTP API
    #[structopt(long, default_value = "127.0.0.1")]
    api_address: IpAddr,
    /// Host port to use for HTTP API
    #[structopt(long, default_value = "3030")]
    api_port: u16,
    /// Host address to use for BGPd
    #[structopt(long, default_value = "127.0.0.1")]
    bgp_address: IpAddr,
    /// Host port to use for BGPd
    #[structopt(long, default_value = "179")]
    bgp_port: u16,
    /// Log verbosity (additive [-vv] for debug, trace, etc.)
    #[structopt(short, parse(from_occurrences))]
    pub verbose: u8,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::from_args();

    let (kvs_level, other_level) = match args.verbose {
        0 => (LevelFilter::Info, LevelFilter::Warn),
        1 => (LevelFilter::Debug, LevelFilter::Warn),
        2 => (LevelFilter::Trace, LevelFilter::Warn),
        _ => (LevelFilter::Trace, LevelFilter::Trace),
    };
    Builder::new()
        .filter(Some("kvs_bgp"), kvs_level)
        .filter(None, other_level)
        .init();
    info!("Logging at levels {}/{}", kvs_level, other_level);

    let kv_store = Arc::new(RwLock::new(KvStore::new()));
    let (outbound_tx, outbound_rx) = mpsc::unbounded_channel();

    let mut bgp_server =
        BgpPeerings::from_config(&args.config_path, args.bgp_address, args.bgp_port).await?;

    // Start the HTTP API server in a thread, updating the KvStore
    let api_routes = api::get_routes(kv_store.clone(), outbound_tx);
    tokio::spawn(async move {
        info!(
            "Starting HTTP API on {}:{}",
            args.api_address, args.api_port
        );
        warp::serve(api_routes)
            .run((args.api_address, args.api_port))
            .await;
    });

    // Run the BGP daemon
    // Injecting inbound updates into KvStore and outbound updates to peers
    bgp_server.run(kv_store, outbound_rx).await?;
    Ok(())
}
