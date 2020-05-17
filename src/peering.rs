//! Module for injecting and receiving BGP update messages
//!
//! Uses [bgpd-rs](https://github.com/thepacketgeek/bgpd-rs) for session management
//! and RIB storage of pending updates

use std::collections::HashMap;
use std::convert::TryInto;
use std::error::Error;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use bgp_rs::{MPUnreachNLRI, NLRIEncoding, PathAttribute, AFI, SAFI};
use bgpd::{
    config::{self, ServerConfig},
    rib::{Family, RIB},
    session::{SessionManager, SessionUpdate},
};
use log::{debug, trace};
use tokio::{
    self,
    net::TcpListener,
    sync::{mpsc, watch, RwLock},
};

use crate::{
    kv::{KeyValue, Route, RouteCollection},
    store::{KvStore, Update as KvUpdate},
};

/// Struct for interacting with BGP Peers
///
/// Keeps sessions and an RIB for storing inbound/outbound updates for `KeyValue` pair routes
pub struct BgpPeerings {
    pub sessions: Arc<RwLock<SessionManager>>,
    pub rib: Arc<RwLock<RIB>>,
}

impl BgpPeerings {
    /// Construct a new `BGPPeerings` struct from a config, BGP TcpListener, and config_rx
    pub fn new(
        config: Arc<ServerConfig>,
        listener: TcpListener,
        config_rx: watch::Receiver<Arc<ServerConfig>>,
    ) -> Result<Self, Box<dyn Error>> {
        let manager = SessionManager::new(config, listener, config_rx);
        Ok(Self {
            sessions: Arc::new(RwLock::new(manager)),
            rib: Arc::new(RwLock::new(RIB::new())),
        })
    }

    /// Construct a new `BGPPeerings` struct from a given bgpd-rs config file
    pub async fn from_config(
        config_path: &str,
        addr: IpAddr,
        port: u16,
    ) -> Result<Self, Box<dyn Error>> {
        let config = Arc::new(config::from_file(&config_path)?);
        debug!("Found {} peers in {}", config.peers.len(), config_path);
        trace!("Using config: {:#?}", &config);
        let (config_tx, config_rx) = watch::channel(config.clone());
        config_tx.broadcast(config.clone())?;

        let socket = SocketAddr::from((addr, port));
        let bgp_listener = TcpListener::bind(&socket).await?;
        Self::new(config, bgp_listener, config_rx)
    }

    /// Process BGP sessions & updates, listening for KvStore updates from the HTTP API and
    /// announcing routes out to peers
    pub async fn run(
        &mut self,
        kv_store: Arc<RwLock<KvStore>>,
        mut outbound_updates: mpsc::UnboundedReceiver<KvUpdate>,
    ) -> Result<(), Box<dyn Error>> {
        // BGP Updates from peers may come in multiple messages
        // Keep any routes that have come in a HashMap, keyed by file hash
        // and only decode once all messages for a KeyValue version are received
        let mut pending_routes: HashMap<u64, Vec<Route>> = HashMap::new();

        loop {
            let mut sessions = self.sessions.write().await;
            tokio::select! {
                update = sessions.get_update(self.rib.clone()) => {
                    if let Ok(Some(SessionUpdate::Learned((_, update)))) = update {
                        if let Ok(route) = TryInto::<Route>::try_into(&update) {
                            let hash = route.hash();
                            let kv_length = route.collection_length();
                            trace!("Bgp update: {} {:?}", hash, route);
                            let routes = pending_routes.entry(hash).or_insert_with(|| vec![]);
                            routes.push(route);
                            trace!("Bgp update: {} [{}/{}]", hash, routes.len(), kv_length);

                            if routes.len() == kv_length {
                                let full_routes = pending_routes.remove(&hash).expect("Hash is in map");
                                let collection = RouteCollection::from_routes(full_routes);
                                if let Ok(kv) = TryInto::<KeyValue<String, String>>::try_into(&collection) {
                                    kv_store.write().await.insert_from_peer(kv);
                                }
                            }
                        }
                    }
                },
                outbound_update = outbound_updates.recv() => {
                    if let Some(update) = outbound_update {
                        // New/updated `KeyValue` pairs need to be announced to peers
                        if let Some(announce) = update.announce {
                            for route in announce.iter() {
                                self.rib.write().await.insert_from_api(
                                    Family::new(AFI::IPV6, SAFI::Unicast),
                                    vec![
                                        PathAttribute::NEXT_HOP((&route.next_hop).into()),
                                    ],
                                    NLRIEncoding::IP(((&route.prefix).into(), 128).into()),
                                );
                            }
                        }
                        if let Some(withdraw) = update.withdraw {
                            for route in withdraw.iter() {
                                self.rib.write().await.insert_from_api(
                                    Family::new(AFI::IPV6, SAFI::Unicast),
                                    vec![
                                        PathAttribute::MP_UNREACH_NLRI(MPUnreachNLRI {
                                            afi: AFI::IPV6,
                                            safi: SAFI::Unicast,
                                            withdrawn_routes: vec![
                                                NLRIEncoding::IP(((&route.prefix).into(), 128).into()),
                                            ],
                                        }),
                                        PathAttribute::NEXT_HOP((&route.next_hop).into()),
                                    ],
                                    NLRIEncoding::IP(((&route.prefix).into(), 128).into()),
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
