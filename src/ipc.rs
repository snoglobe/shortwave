 use std::{path::Path, sync::Arc};

 use tokio::{
    io::{AsyncBufReadExt, BufReader, AsyncReadExt},
     net::{UnixListener, UnixStream},
 };
use tracing::{info, warn};

 use crate::{state::AppState, types::NowPlaying};
 use chrono::Utc;

 async fn handle_ipc_stream(state: Arc<AppState>, stream: UnixStream) {
     let reader = BufReader::new(stream);
     let mut lines = reader.lines();
     while let Ok(Some(line)) = lines.next_line().await {
         let line = line.trim();
         if line.is_empty() { continue; }
         match serde_json::from_str::<serde_json::Value>(line) {
             Ok(v) => {
                 let np = NowPlaying {
                     title: v.get("title").and_then(|x| x.as_str()).map(|s| s.to_string()),
                     artist: v.get("artist").and_then(|x| x.as_str()).map(|s| s.to_string()),
                     album: v.get("album").and_then(|x| x.as_str()).map(|s| s.to_string()),
                     cover_url: v.get("cover_url").and_then(|x| x.as_str()).map(|s| s.to_string()),
                     updated_at: Utc::now(),
                 };
                 state.set_now_playing(np).await;
             }
             Err(err) => warn!(error=%err, "invalid IPC JSON"),
         }
     }
 }

 pub async fn run_ipc_listener(state: Arc<AppState>, socket_path: String) -> anyhow::Result<()> {
     let p = Path::new(&socket_path);
     if p.exists() {
         // best effort unlink
         let _ = tokio::fs::remove_file(p).await;
     }
     let listener = UnixListener::bind(p)?;
     info!(path=%socket_path, "IPC socket listening");
     loop {
         match listener.accept().await {
             Ok((stream, _addr)) => {
                 let st = state.clone();
                 tokio::spawn(async move { handle_ipc_stream(st, stream).await });
             }
             Err(err) => {
                 warn!(error=%err, "IPC accept error");
             }
         }
     }
 }

pub async fn run_audio_ipc_listener(state: Arc<AppState>, socket_path: String) -> anyhow::Result<()> {
    let p = Path::new(&socket_path);
    if p.exists() {
        let _ = tokio::fs::remove_file(p).await;
    }
    let listener = UnixListener::bind(p)?;
    info!(path=%socket_path, "Audio IPC socket listening");
    loop {
        match listener.accept().await {
            Ok((mut stream, _addr)) => {
                let st = state.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 16 * 1024];
                    loop {
                        match stream.read(&mut buf).await {
                            Ok(0) => break,
                            Ok(n) => {
                                let _ = st.audio_tx.send(bytes::Bytes::copy_from_slice(&buf[..n]));
                            }
                            Err(err) => {
                                warn!(error=%err, "audio IPC read error");
                                break;
                            }
                        }
                    }
                });
            }
            Err(err) => {
                warn!(error=%err, "audio IPC accept error");
            }
        }
    }
}


