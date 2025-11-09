use std::sync::Arc;
use std::time::Duration;

use libp2p::{
    gossipsub::{self, IdentTopic as Topic, MessageAuthenticity, ConfigBuilder as GossipsubConfigBuilder, ValidationMode, Event as GossipEvent},
    identity,
    mdns,
    swarm::{SwarmEvent},
    SwarmBuilder,
    tcp,
    Multiaddr, PeerId,
    noise, yamux,
};
use libp2p::swarm::{behaviour::toggle::Toggle, NetworkBehaviour};
use tokio::fs;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use futures_util::StreamExt;

use crate::state::AppState;
use crate::types::{ReleaseRequest, StationAdvertisement};

#[derive(NetworkBehaviour)]
struct NodeBehaviour {
    pub gossipsub: gossipsub::Behaviour<gossipsub::IdentityTransform, gossipsub::AllowAllSubscriptionFilter>,
    pub mdns: Toggle<mdns::tokio::Behaviour>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
enum GossipMessage {
    Advertise(StationAdvertisement),
    Release(ReleaseRequest),
}

pub struct P2PHandle {
    tx: mpsc::Sender<GossipMessage>,
}

impl P2PHandle {
    pub async fn publish_advertisement(&self, ad: StationAdvertisement) {
        let _ = self.tx.send(GossipMessage::Advertise(ad)).await;
    }
    pub async fn publish_release(&self, rel: ReleaseRequest) {
        let _ = self.tx.send(GossipMessage::Release(rel)).await;
    }
}

pub async fn run_libp2p(
    state: Arc<AppState>,
    listen_addrs: Vec<String>,
    bootstrap: Vec<String>,
    enable_mdns: bool,
    key_path: Option<String>,
) -> anyhow::Result<P2PHandle> {
    // Load or generate a persistent libp2p identity key
    let local_key = if let Some(path) = key_path {
        match fs::read(&path).await {
            Ok(bytes) => {
                // First try protobuf-encoded Keypair
                if let Ok(kp) = identity::Keypair::from_protobuf_encoding(&bytes) {
                    kp
                } else {
                    // Fallback to raw/base64 32-byte ed25519 secret
                    let mut raw = bytes;
                    if raw.len() != 32 {
                        if let Ok(s) = std::str::from_utf8(&raw) {
                            if let Ok(decoded) = B64.decode(s.trim()) {
                                raw = decoded;
                            }
                        }
                    }
                    let mut arr: [u8; 32] = raw.as_slice().try_into().map_err(|_| anyhow::anyhow!("invalid p2p key length"))?;
                    let secret = libp2p::identity::ed25519::SecretKey::try_from_bytes(&mut arr)
                        .map_err(|_| anyhow::anyhow!("invalid p2p key file (expect 32-byte ed25519 secret)"))?;
                    let ed = libp2p::identity::ed25519::Keypair::from(secret);
                    identity::Keypair::from(ed)
                }
            }
            Err(_) => {
                let kp = identity::Keypair::generate_ed25519();
                if let Ok(bytes) = kp.to_protobuf_encoding() {
                    let _ = fs::write(&path, bytes).await;
                }
                kp
            }
        }
    } else {
        identity::Keypair::generate_ed25519()
    };
    let local_peer_id = PeerId::from(local_key.public());
    info!(%local_peer_id, "libp2p starting");

    let mut swarm = SwarmBuilder::with_existing_identity(local_key.clone())
        .with_tokio()
        .with_tcp(
            tcp::Config::default().nodelay(true),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(move |keys| {
            let gossipsub_config = GossipsubConfigBuilder::default()
                .validation_mode(ValidationMode::Strict)
                .heartbeat_interval(Duration::from_secs(5))
                .max_transmit_size(1024 * 128)
                .build()
                .expect("gossipsub config");
            let mut gs = gossipsub::Behaviour::<gossipsub::IdentityTransform, gossipsub::AllowAllSubscriptionFilter>::new(
                MessageAuthenticity::Signed(keys.clone()),
                gossipsub_config,
            )
            .expect("gossipsub behaviour");
            let _ = gs.subscribe(&Topic::new("shortwave/advertise/v1"));
            let _ = gs.subscribe(&Topic::new("shortwave/release/v1"));
            let mdns_behaviour = if enable_mdns {
                Toggle::from(Some(mdns::tokio::Behaviour::new(mdns::Config::default(), PeerId::from(keys.public())).expect("mdns")))
            } else {
                Toggle::from(None)
            };
            NodeBehaviour { gossipsub: gs, mdns: mdns_behaviour }
        })?
        .build();

    // Listen addresses
    if listen_addrs.is_empty() {
        swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;
    } else {
        for la in listen_addrs {
            match la.parse::<Multiaddr>() {
                Ok(ma) => { swarm.listen_on(ma)?; },
                Err(err) => warn!(error=%err, addr=%la, "invalid listen multiaddr"),
            }
        }
    }
    for b in bootstrap {
        match b.parse::<Multiaddr>() {
            Ok(ma) => { if let Err(err) = swarm.dial(ma.clone()) { warn!(error=%err, addr=%ma, "bootstrap dial failed"); } },
            Err(err) => warn!(error=%err, addr=%b, "invalid bootstrap multiaddr"),
        }
    }

    let (tx, mut rx) = mpsc::channel::<GossipMessage>(128);
    let handle = P2PHandle { tx: tx.clone() };

    let st = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(cmd) = rx.recv() => {
                    match cmd {
                        GossipMessage::Advertise(ad) => {
                            if let Ok(bytes) = serde_json::to_vec(&GossipMessage::Advertise(ad)) {
                                if let Err(err) = swarm.behaviour_mut().gossipsub.publish(Topic::new("shortwave/advertise/v1"), bytes) { warn!(error=%err, "gossip publish advertise failed"); }
                            }
                        }
                        GossipMessage::Release(rel) => {
                            if let Ok(bytes) = serde_json::to_vec(&GossipMessage::Release(rel)) {
                                if let Err(err) = swarm.behaviour_mut().gossipsub.publish(Topic::new("shortwave/release/v1"), bytes) { warn!(error=%err, "gossip publish release failed"); }
                            }
                        }
                    }
                }
                event = swarm.next() => {
                    let Some(event) = event else { continue };
                    match event {
                        SwarmEvent::Behaviour(NodeBehaviourEvent::Gossipsub(GossipEvent::Message { message, .. })) => {
                            if let Ok(g) = serde_json::from_slice::<GossipMessage>(&message.data) {
                                match g {
                                    GossipMessage::Advertise(ad) => {
                                        let _ = st.accept_advertisement(&ad).await;
                                    }
                                    GossipMessage::Release(rel) => {
                                        let key = crate::types::normalize_frequency_key(&rel.frequency);
                                        let _ = st.release_assignment(&key, rel.station_id, &rel.signature).await;
                                    }
                                }
                            }
                        }
                        SwarmEvent::Behaviour(NodeBehaviourEvent::Mdns(mdns::Event::Discovered(list))) => {
                            for (_peer, addr) in list {
                                if let Err(err) = swarm.dial(addr.clone()) {
                                    warn!(error=%err, addr=%addr, "mdns dial failed");
                                }
                            }
                        }
                        SwarmEvent::NewListenAddr { address, .. } => {
                            info!(%address, "libp2p listening");
                        }
                        SwarmEvent::ConnectionEstablished { peer_id, .. } => { debug!(%peer_id, "connected"); }
                        SwarmEvent::ConnectionClosed { peer_id, .. } => { debug!(%peer_id, "disconnected"); }
                        _ => {}
                    }
                }
            }
        }
    });

    Ok(handle)
}


