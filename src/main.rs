 use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{
	routing::{get, put},
	Router,
};
 use chrono::{DateTime, Utc};
use tracing::{info, warn};

 mod config;
 mod http;
mod p2p;
 mod state;
 mod types;
mod crypto;
mod ipc;

 use crate::config::Cli;
 use crate::state::AppState;
use crate::types::{StationAdvertisement};
use crate::types::normalize_frequency_key;
use crate::crypto::{encode_public_key_b64, encode_signature_b64, sign_bytes, canonicalize_ad_bytes};
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use rand::RngCore;
use tower_http::cors::CorsLayer;
use axum::middleware;

 #[tokio::main]
 async fn main() -> anyhow::Result<()> {
 	// Initialize logging
 	tracing_subscriber::fmt()
 		.with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
 		.with_target(false)
 		.init();

 	let cli = Cli::parse();
		let config = cli.into_config()?;

 	let addr: SocketAddr = config.bind.parse()?;

	let state = Arc::new(AppState::new(
 		config.node_id,
 		config.public_url.clone(),
 		config.source_token.clone(),
		config.max_frequencies_per_owner,
 	));

 	// Build router
 	let app = Router::new()
 		.route("/api/v1/healthz", get(http::healthz))
 		.route("/api/v1/stations", get(http::get_stations))
 		.route("/api/v1/stations/:frequency", get(http::get_station_by_frequency))
 		.route("/api/v1/events", get(http::events_sse))
		.route("/api/v1/now", get(http::now_playing))
		.route("/api/v1/now/events", get(http::now_events_sse))
 		.route("/stream", get(http::stream_audio))
 		.route("/api/v1/source", put(http::put_source))
		// P2P HTTP routes removed (libp2p in use)
		.with_state(state.clone())
		.layer(middleware::from_fn_with_state(state.clone(), http::blocklist_middleware))
		.layer(CorsLayer::permissive());

 	let listener = tokio::net::TcpListener::bind(addr).await?;
 	info!("listening on http://{}", addr);

   // Start libp2p gossip
   let p2p_handle = p2p::run_libp2p(
        state.clone(),
        config.p2p_listen.clone(),
        config.p2p_bootstrap.clone(),
       config.p2p_mdns,
       config.p2p_key_path.clone(),
    ).await?;

    // Background: station advertisement (heartbeat)
    let state_for_boot = state.clone();
    let advertise_ttl = config.advertise_ttl_secs;
    let local_station = config.local_station.clone();
	let signing_key: SigningKey = match config.owner_signing_key.clone() {
		Some(sk) => sk,
		None => {
			let mut seed = [0u8; 32];
			OsRng.fill_bytes(&mut seed);
			SigningKey::from_bytes(&seed)
		}
	};
    let signing_key = std::sync::Arc::new(signing_key);
    let owner_public_key_b64 = encode_public_key_b64(&signing_key.verifying_key());
    tokio::spawn(async move {
 		// If we're a station, advertise now and periodically
		if let Some(ls) = local_station {
 			let mut interval = tokio::time::interval(Duration::from_secs((advertise_ttl / 2).max(10) as u64));
 			loop {
 				let now: DateTime<Utc> = Utc::now();
				let freq_key = normalize_frequency_key(&ls.frequency);
                // Offload CPU-heavy signing to blocking pool to avoid impacting audio streaming.
                let sk = signing_key.clone();
                let station_id_str = ls.station_id.to_string();
                let stream_url = ls.stream_url.clone();
                let now_str = now.to_rfc3339();
                let sig_b64 = tokio::task::spawn_blocking(move || {
                    let msg = canonicalize_ad_bytes(
                        "advertise",
                        &freq_key,
                        &station_id_str,
                        &stream_url,
                        &now_str,
                        advertise_ttl,
                    );
                    encode_signature_b64(&sign_bytes(&sk, &msg))
                }).await.unwrap_or_else(|_| "".to_string());
				let ad = StationAdvertisement {
 					message_id: uuid::Uuid::new_v4(),
 					station_id: ls.station_id,
					frequency: ls.frequency.clone(),
 					name: ls.name.clone(),
 					stream_url: ls.stream_url.clone(),
 					advertised_at: now,
 					ttl_seconds: advertise_ttl,
					owner_public_key: owner_public_key_b64.clone(),
					signature: sig_b64,
 				};
                match state_for_boot.accept_advertisement(&ad).await {
                    Ok(assignment) => {
                        p2p_handle.publish_advertisement(ad.clone()).await;
                        info!(frequency=%assignment.frequency, station_id=%assignment.station_id, "advertised station");
                    }
                    Err(err) => {
                        warn!(error=%err, "local advertisement conflicted; will retry later");
                    }
                }
 				interval.tick().await;
 			}
 		}
 	});

	// Background: IPC listener for NowPlaying
	if let Some(sock) = config.ipc_socket.clone() {
		let st = state.clone();
		tokio::spawn(async move {
			if let Err(err) = crate::ipc::run_ipc_listener(st, sock).await {
				warn!(error=%err, "ipc listener exited");
			}
		});
	}
	// Background: Audio IPC listener (raw bytes)
	if let Some(sock) = config.audio_ipc_socket.clone() {
		let st = state.clone();
		tokio::spawn(async move {
			if let Err(err) = crate::ipc::run_audio_ipc_listener(st, sock).await {
				warn!(error=%err, "audio ipc listener exited");
			}
		});
	}

 	// Background: periodic expiry cleanup
 	let expiry_state = state.clone();
 	tokio::spawn(async move {
 		let mut interval = tokio::time::interval(Duration::from_secs(15));
 		loop {
 			interval.tick().await;
 			if let Err(err) = expiry_state.expire_assignments().await {
 				warn!(error=%err, "expiry task error");
 			}
 		}
 	});

	// Background: blocklist fetcher
	if let Some(url) = config.blocklist_url.clone() {
		let st = state.clone();
		let refresh = config.blocklist_refresh_secs;
		tokio::spawn(async move {
			let client = reqwest::Client::builder().no_proxy().build();
			let mut interval = tokio::time::interval(Duration::from_secs(refresh as u64));
			loop {
				match &client {
					Ok(c) => {
						match c.get(&url).send().await {
							Ok(resp) => {
								if resp.status().is_success() {
									if let Ok(body) = resp.text().await {
										let mut set = std::collections::HashSet::new();
										for line in body.lines() {
											let mut s = line.trim();
											if s.is_empty() || s.starts_with('#') { continue; }
											if let Some((left, _)) = s.split_once('#') { s = left.trim(); }
											if let Ok(ip) = s.parse::<std::net::IpAddr>() {
												set.insert(ip);
											}
										}
										st.set_blocklist(set).await;
									}
								}
							}
							Err(err) => {
								warn!(error=%err, "blocklist fetch failed");
							}
						}
					}
					Err(err) => {
						warn!(error=%err, "failed to build http client for blocklist");
					}
				}
				interval.tick().await;
			}
		});
	}

	axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;
 	Ok(())
 }


