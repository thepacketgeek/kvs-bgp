use std::error::Error;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use bgp_rs::Update;
use bgpd::{
    config::{self, ServerConfig},
    rib::RIB,
    session::{SessionManager, SessionUpdate},
};
use log::{debug, trace};
use tokio::net::TcpListener;
use tokio::sync::{watch, RwLock};

pub struct BgpPeerings {
    pub sessions: Arc<RwLock<SessionManager>>,
    pub rib: Arc<RwLock<RIB>>,
}

impl BgpPeerings {
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

    pub async fn with_config(
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

    pub async fn serve(&mut self) -> Result<(), Box<dyn Error>> {
        while let Ok(update) = self
            .sessions
            .write()
            .await
            .get_update(self.rib.clone())
            .await
        {
            match update {
                Some(SessionUpdate::Learned((router_id, update))) => {
                    trace!("Incoming update from {}: {:?}", router_id, update);
                    // Some(update)
                }
                _ => (),
            }
        }
        Ok(())
    }
}
