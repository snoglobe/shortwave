use clap::{ArgAction, Parser};
 use uuid::Uuid;
use bigdecimal::BigDecimal;
use std::str::FromStr;
use ed25519_dalek::SigningKey;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use serde::Deserialize;

 #[derive(Clone, Debug)]
 pub struct LocalStationConfig {
 	pub station_id: Uuid,
 	pub name: String,
	pub frequency: BigDecimal,
 	pub stream_url: String,
 }

 #[derive(Clone, Debug)]
 pub struct Config {
 	pub node_id: Uuid,
 	pub bind: String,
 	pub public_url: String,
 	pub peers: Vec<String>,
 	pub source_token: Option<String>,
 	pub local_station: Option<LocalStationConfig>,
 	pub advertise_ttl_secs: u32,
 	pub owner_signing_key: Option<SigningKey>,
 	pub max_frequencies_per_owner: u32,
	pub ipc_socket: Option<String>,
	pub audio_ipc_socket: Option<String>,
	pub blocklist_url: Option<String>,
	pub blocklist_refresh_secs: u32,
 	pub p2p_listen: Vec<String>,
 	pub p2p_bootstrap: Vec<String>,
 	pub p2p_mdns: bool,
	pub p2p_key_path: Option<String>,
 }

 #[derive(Parser, Debug, Clone)]
 #[command(author, version, about = "Shortwave P2P Internet Radio Node", long_about = None)]
 pub struct Cli {
	/// Path to YAML config file (if provided, overrides CLI)
	#[arg(long = "config", env = "SHORTWAVE_CONFIG")]
	pub config_path: Option<String>,
 	/// Bind address for the HTTP API (e.g. 0.0.0.0:8080)
 	#[arg(long, env = "SHORTWAVE_BIND", default_value = "0.0.0.0:8080")]
 	pub bind: String,

 	/// Public base URL of this node (e.g. https://radio.example.com)
 	#[arg(long, env = "SHORTWAVE_PUBLIC_URL")]
 	pub public_url: String,

 	/// Optional node ID. If omitted, a random UUID v4 is generated each start.
 	#[arg(long, env = "SHORTWAVE_NODE_ID")]
 	pub node_id: Option<String>,

 	/// Peer API base URL(s) to initially register with (repeat flag for multiple peers)
 	#[arg(long = "peer", env = "SHORTWAVE_PEERS", action = ArgAction::Append)]
 	pub peers: Vec<String>,

 	/// Token required to PUT /api/v1/source for ingest; omit to disable authentication
 	#[arg(long, env = "SHORTWAVE_SOURCE_TOKEN")]
 	pub source_token: Option<String>,

 	/// Station display name (enable station mode when set)
 	#[arg(long, env = "SHORTWAVE_STATION_NAME")]
 	pub name: Option<String>,

 	/// Frequency ID to advertise (enable station mode when set)
 	#[arg(long, env = "SHORTWAVE_FREQUENCY")]
	pub frequency: Option<String>,

 	/// Explicit station ID for persistence; omit to autogenerate
 	#[arg(long, env = "SHORTWAVE_STATION_ID")]
 	pub station_id: Option<String>,

 	/// TTL in seconds for station advertisements
 	#[arg(long, env = "SHORTWAVE_TTL_SECS", default_value_t = 60)]
 	pub ttl_secs: u32,

 	/// Base64-encoded 32-byte Ed25519 secret key for signing station ads/releases
 	#[arg(long, env = "SHORTWAVE_OWNER_SECRET_KEY")]
 	pub owner_secret_key: Option<String>,

 	/// Maximum concurrent frequencies per owner public key
 	#[arg(long, env = "SHORTWAVE_MAX_FREQS_PER_OWNER", default_value_t = 3)]
 	pub max_freqs_per_owner: u32,

 	/// Unix domain socket path to receive NowPlaying JSON lines
 	#[arg(long, env = "SHORTWAVE_IPC_SOCKET")]
 	pub ipc_socket: Option<String>,

	/// Unix domain socket path to receive raw audio bytes (MPEG/OGG/Opus)
	#[arg(long, env = "SHORTWAVE_AUDIO_IPC_SOCKET")]
	pub audio_ipc_socket: Option<String>,

	/// URL to fetch IP blocklist (one IP or CIDR per line, '#' comments allowed)
	#[arg(long, env = "SHORTWAVE_BLOCKLIST_URL")]
	pub blocklist_url: Option<String>,

	/// Refresh interval in seconds for blocklist
	#[arg(long, env = "SHORTWAVE_BLOCKLIST_REFRESH_SECS", default_value_t = 600)]
	pub blocklist_refresh_secs: u32,

 	/// libp2p listen multiaddrs (repeatable)
 	#[arg(long = "p2p-listen", env = "SHORTWAVE_P2P_LISTEN", action = ArgAction::Append)]
 	pub p2p_listen: Vec<String>,

 	/// libp2p bootstrap peer multiaddrs (repeatable)
 	#[arg(long = "p2p-bootstrap", env = "SHORTWAVE_P2P_BOOTSTRAP", action = ArgAction::Append)]
 	pub p2p_bootstrap: Vec<String>,

 	/// Enable mDNS discovery
 	#[arg(long = "p2p-mdns", env = "SHORTWAVE_P2P_MDNS", default_value_t = true)]
 	pub p2p_mdns: bool,

	/// Path to persist libp2p Ed25519 private key (stable PeerId)
	#[arg(long = "p2p-key-path", env = "SHORTWAVE_P2P_KEY_PATH")]
	pub p2p_key_path: Option<String>,
 }

 impl Cli {
 	pub fn parse() -> Self {
 		<Self as Parser>::parse()
 	}

 	pub fn into_config(self) -> anyhow::Result<Config> {
		// If a config file is provided, prefer loading from it.
		if let Some(path) = self.config_path.clone() {
			return load_config_file(&path);
		}
 		let node_id = match self.node_id {
 			Some(s) => Uuid::from_str(&s)?,
 			None => Uuid::new_v4(),
 		};

		let local_station = match (self.name.clone(), self.frequency.clone()) {
 			(Some(name), Some(frequency)) => {
				let freq = BigDecimal::from_str(&frequency)?;
 				let station_id = match self.station_id {
 					Some(id) => Uuid::from_str(&id)?,
 					None => Uuid::new_v4(),
 				};
 				let stream_url = format!("{}/stream", self.public_url.trim_end_matches('/'));
				Some(LocalStationConfig { station_id, name, frequency: freq, stream_url })
 			}
 			_ => None,
 		};

 		let owner_signing_key = match self.owner_secret_key {
 			Some(sk_b64) => {
 				let bytes = B64.decode(sk_b64)?;
 				let sk = SigningKey::from_bytes(bytes.as_slice().try_into()?);
 				Some(sk)
 			}
 			None => None,
 		};

 		Ok(Config {
 			node_id,
 			bind: self.bind,
 			public_url: self.public_url,
 			peers: self.peers,
 			source_token: self.source_token,
 			local_station,
 			advertise_ttl_secs: self.ttl_secs.max(10),
 			owner_signing_key,
 			max_frequencies_per_owner: self.max_freqs_per_owner.max(1),
 			ipc_socket: self.ipc_socket,
			audio_ipc_socket: self.audio_ipc_socket,
			blocklist_url: self.blocklist_url,
			blocklist_refresh_secs: self.blocklist_refresh_secs.max(30),
 			p2p_listen: self.p2p_listen,
 			p2p_bootstrap: self.p2p_bootstrap,
 			p2p_mdns: self.p2p_mdns,
			p2p_key_path: self.p2p_key_path,
 		})
 	}
 }

#[derive(Debug, Deserialize, Clone)]
struct FileStation {
	pub name: String,
	pub frequency: BigDecimal,
	pub station_id: Option<Uuid>,
}

#[derive(Debug, Deserialize, Clone)]
struct FileP2P {
	pub listen: Option<Vec<String>>,
	pub bootstrap: Option<Vec<String>>,
	pub mdns: Option<bool>,
	pub key_path: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct FileConfig {
	pub bind: Option<String>,
	pub public_url: String,
	pub node_id: Option<Uuid>,
	pub source_token: Option<String>,
	pub station: Option<FileStation>,
	pub advertise_ttl_secs: Option<u32>,
	pub owner_secret_key: Option<String>,
	pub max_frequencies_per_owner: Option<u32>,
	pub ipc_socket: Option<String>,
	pub audio_ipc_socket: Option<String>,
	pub blocklist_url: Option<String>,
	pub blocklist_refresh_secs: Option<u32>,
	pub p2p: Option<FileP2P>,
}

fn load_config_file(path: &str) -> anyhow::Result<Config> {
	let text = std::fs::read_to_string(path)?;
	let cfg: FileConfig = serde_yaml::from_str(&text)?;
	let node_id = cfg.node_id.unwrap_or_else(Uuid::new_v4);
	let bind = cfg.bind.unwrap_or_else(|| "0.0.0.0:8080".to_string());
	let public_url = cfg.public_url;
	let local_station = match cfg.station {
		Some(fs) => {
			let station_id = fs.station_id.unwrap_or_else(Uuid::new_v4);
			let stream_url = format!("{}/stream", public_url.trim_end_matches('/'));
			Some(LocalStationConfig {
				station_id,
				name: fs.name,
				frequency: fs.frequency,
				stream_url,
			})
		}
		None => None,
	};
	let owner_signing_key = match cfg.owner_secret_key {
		Some(sk_b64) => {
			let bytes = B64.decode(sk_b64)?;
			let sk = SigningKey::from_bytes(bytes.as_slice().try_into()?);
			Some(sk)
		}
		None => None,
	};
	let p2p_listen = cfg.p2p.as_ref().and_then(|p| p.listen.clone()).unwrap_or_default();
	let p2p_bootstrap = cfg.p2p.as_ref().and_then(|p| p.bootstrap.clone()).unwrap_or_default();
	let p2p_mdns = cfg.p2p.as_ref().and_then(|p| p.mdns).unwrap_or(true);
	let p2p_key_path = cfg.p2p.and_then(|p| p.key_path);
	Ok(Config {
		node_id,
		bind,
		public_url,
		peers: Vec::new(),
		source_token: cfg.source_token,
		local_station,
		advertise_ttl_secs: cfg.advertise_ttl_secs.unwrap_or(60).max(10),
		owner_signing_key,
		max_frequencies_per_owner: cfg.max_frequencies_per_owner.unwrap_or(3).max(1),
		ipc_socket: cfg.ipc_socket,
		audio_ipc_socket: cfg.audio_ipc_socket,
		blocklist_url: cfg.blocklist_url,
		blocklist_refresh_secs: cfg.blocklist_refresh_secs.unwrap_or(600).max(30),
		p2p_listen,
		p2p_bootstrap,
		p2p_mdns,
		p2p_key_path,
	})
}


