use std::collections::{HashMap, HashSet};

use chrono::{Duration, Utc};
 use tokio::sync::{broadcast, RwLock};
 use uuid::Uuid;

use crate::types::{normalize_frequency_key, PeerInfo, RegistryEvent, StationAdvertisement, StationAssignment, NowPlaying};
use crate::crypto::{parse_public_key_b64, parse_sig_b64, verify_bytes, canonicalize_ad_bytes, canonicalize_release_bytes};

use std::net::IpAddr;

 #[derive(thiserror::Error, Debug)]
 pub enum RegistryError {
 	#[error("frequency '{0}' already assigned to {1}")]
 	FrequencyConflict(String, Uuid),
    #[error("invalid signature")]
    InvalidSignature,
    #[error("owner public key mismatch")]
    OwnerMismatch,
    #[error("owner cap exceeded")]
    OwnerCapExceeded,
 }

 pub struct AppState {
 	pub node_id: Uuid,
 	pub public_url: String,
 	pub source_token: Option<String>,
	pub max_frequencies_per_owner: u32,

 	pub peers: RwLock<HashMap<String, PeerInfo>>, // key: api_base_url
    pub registry: RwLock<HashMap<String, StationAssignment>>, // key: normalized frequency string
 	pub seen_messages: RwLock<HashSet<Uuid>>, // message dedupe

    pub events_tx: broadcast::Sender<RegistryEvent>,
    pub audio_tx: broadcast::Sender<bytes::Bytes>,
    pub now_tx: broadcast::Sender<NowPlaying>,
    pub now_playing: RwLock<Option<NowPlaying>>,
	pub blocklist: RwLock<std::collections::HashSet<IpAddr>>,
 }

 impl AppState {
 	pub fn new(
 		node_id: Uuid,
 		public_url: String,
 		source_token: Option<String>,
		max_frequencies_per_owner: u32,
 	) -> Self {
        let (events_tx, _events_rx) = broadcast::channel(1024);
        let (audio_tx, _audio_rx) = broadcast::channel(256);
        let (now_tx, _now_rx) = broadcast::channel(128);

 		Self {
 			node_id,
 			public_url,
 			source_token,
			max_frequencies_per_owner,
 			peers: RwLock::new(HashMap::new()),
 			registry: RwLock::new(HashMap::new()),
 			seen_messages: RwLock::new(HashSet::new()),
            events_tx,
            audio_tx,
            now_tx,
            now_playing: RwLock::new(None),
			blocklist: RwLock::new(std::collections::HashSet::new()),
 		}
 	}

   pub async fn accept_advertisement(&self, ad: &StationAdvertisement) -> Result<StationAssignment, RegistryError> {
        let key = normalize_frequency_key(&ad.frequency);
        {
            let mut seen = self.seen_messages.write().await;
            if !seen.insert(ad.message_id) {
                // already processed
                if let Some(existing) = self.registry.read().await.get(&key).cloned() {
                    return Ok(existing);
                }
            }
        }
       // Verify signature for advertisement
       let vk = parse_public_key_b64(&ad.owner_public_key).map_err(|_| RegistryError::InvalidSignature)?;
        let msg = canonicalize_ad_bytes(
            "advertise",
            &key,
            &ad.station_id.to_string(),
            &ad.stream_url,
            &ad.advertised_at.to_rfc3339(),
            ad.ttl_seconds,
        );
       let sig = parse_sig_b64(&ad.signature).map_err(|_| RegistryError::InvalidSignature)?;
        verify_bytes(&vk, &msg, &sig).map_err(|_| RegistryError::InvalidSignature)?;
        let mut reg = self.registry.write().await;
        if let Some(existing) = reg.get(&key) {
 			if existing.station_id != ad.station_id {
                return Err(RegistryError::FrequencyConflict(key, existing.station_id));
 			}
            if existing.owner_public_key != ad.owner_public_key {
                return Err(RegistryError::OwnerMismatch);
            }
 		}
        if !reg.contains_key(&key) {
            let owner = &ad.owner_public_key;
            let count = reg.values().filter(|a| &a.owner_public_key == owner).count() as u32;
            if count >= self.max_frequencies_per_owner {
                return Err(RegistryError::OwnerCapExceeded);
            }
        }

 		let created_at = Utc::now();
 		let expires_at = ad.advertised_at + Duration::seconds(ad.ttl_seconds as i64);
        let assignment = StationAssignment {
 			station_id: ad.station_id,
            frequency: ad.frequency.clone(),
 			name: ad.name.clone(),
 			stream_url: ad.stream_url.clone(),
 			created_at,
 			last_seen: ad.advertised_at,
 			expires_at,
            owner_public_key: ad.owner_public_key.clone(),
 		};
        reg.insert(key, assignment.clone());
 		drop(reg);
 		let _ = self.events_tx.send(RegistryEvent { event: "upsert".into(), assignment: assignment.clone() });
 		Ok(assignment)
 	}

  pub async fn release_assignment(&self, frequency_key: &str, station_id: Uuid, signature_b64: &str) -> bool {
       // First, read to verify
       let maybe_owner_pk = {
           let reg = self.registry.read().await;
           match reg.get(frequency_key) {
               Some(a) if a.station_id == station_id => Some(a.owner_public_key.clone()),
               _ => None,
           }
       };
       let Some(owner_pk) = maybe_owner_pk else { return false };
       let vk = match parse_public_key_b64(&owner_pk) { Ok(v) => v, Err(_) => return false };
       let msg = canonicalize_release_bytes("release", frequency_key, &station_id.to_string());
       let sig = match parse_sig_b64(signature_b64) { Ok(s) => s, Err(_) => return false };
       if verify_bytes(&vk, &msg, &sig).is_err() { return false; }
       // Verified; proceed to remove
       let mut reg = self.registry.write().await;
       if let Some(a) = reg.get(frequency_key) {
           if a.station_id != station_id { return false; }
       } else {
           return false;
       }
       let removed = reg.remove(frequency_key).unwrap();
       drop(reg);
       let _ = self.events_tx.send(RegistryEvent { event: "delete".into(), assignment: removed });
       true
   }

 	pub async fn expire_assignments(&self) -> anyhow::Result<()> {
 		let now = Utc::now();
 		let mut to_remove: Vec<String> = Vec::new();
 		{
 			let reg = self.registry.read().await;
 			for (freq, a) in reg.iter() {
 				if a.expires_at <= now {
 					to_remove.push(freq.clone());
 				}
 			}
 		}
 		if !to_remove.is_empty() {
 			let mut reg = self.registry.write().await;
 			for freq in to_remove {
 				if let Some(removed) = reg.remove(&freq) {
 					let _ = self.events_tx.send(RegistryEvent { event: "delete".into(), assignment: removed });
 				}
 			}
 		}
 		Ok(())
 	}

 	pub async fn snapshot_registry(&self) -> Vec<StationAssignment> {
 		let now = Utc::now();
 		let reg = self.registry.read().await;
 		reg.values().filter(|a| a.expires_at > now).cloned().collect()
 	}

    pub async fn get_assignment_by_key(&self, frequency_key: &str) -> Option<StationAssignment> {
        self.registry.read().await.get(frequency_key).cloned()
 	}

 	pub async fn add_or_update_peer(&self, base_url: String, info: PeerInfo) {
 		self.peers.write().await.insert(base_url, info);
 	}

	pub async fn set_blocklist(&self, ips: std::collections::HashSet<IpAddr>) {
		let mut bl = self.blocklist.write().await;
		*bl = ips;
	}

	pub async fn is_ip_blocked(&self, ip: &IpAddr) -> bool {
		self.blocklist.read().await.contains(ip)
	}

 	pub async fn list_peers(&self) -> Vec<PeerInfo> {
 		self.peers.read().await.values().cloned().collect()
 	}

 	pub async fn merge_peer_register_response(&self, peer_base: &str, resp: crate::types::RegisterPeerResponse) {
 		self.add_or_update_peer(peer_base.to_string(), PeerInfo { node_id: resp.node.node_id, api_base_url: peer_base.to_string(), last_seen: Utc::now() }).await;
 		for p in resp.peers {
 			self.add_or_update_peer(p.api_base_url.clone(), p).await;
 		}
		for a in resp.registry {
			self.import_assignment(a).await;
		}
 	}

	pub async fn import_assignment(&self, assignment: StationAssignment) {
		let key = normalize_frequency_key(&assignment.frequency);
		let mut reg = self.registry.write().await;
		match reg.get(&key) {
			Some(existing) => {
				// If owner matches, update; if owner differs, adopt incoming to converge
				if existing.owner_public_key == assignment.owner_public_key {
					reg.insert(key, assignment.clone());
				} else {
					reg.insert(key, assignment.clone());
				}
			}
			None => {
				reg.insert(key, assignment.clone());
			}
		}
		let _ = self.events_tx.send(RegistryEvent { event: "upsert".into(), assignment });
	}

    pub async fn set_now_playing(&self, np: NowPlaying) {
        {
            let mut guard = self.now_playing.write().await;
            *guard = Some(np.clone());
        }
        let _ = self.now_tx.send(np);
    }

    pub async fn get_now_playing(&self) -> Option<NowPlaying> {
        self.now_playing.read().await.clone()
    }
 }


