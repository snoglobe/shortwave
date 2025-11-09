use std::{convert::Infallible, sync::Arc};
use std::net::SocketAddr;

 use axum::{
 	body::Body,
 	extract::{Path, Query, State},
 	http::{header, HeaderMap, HeaderValue, StatusCode},
 	response::{IntoResponse, Response, Sse},
 	Json,
 };
	use axum::response::sse::Event;
use axum::middleware::Next;
use axum::http::Request;
use axum::extract::connect_info::ConnectInfo;
 use serde::Deserialize;
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use tokio_stream::once;
use std::pin::Pin;
use futures_core::Stream;
use tracing::error;

use crate::state::{AppState};
use crate::types::{
    normalize_frequency_key, ErrorResponse, NodeInfo,
};
use bigdecimal::BigDecimal;
use std::str::FromStr;

 pub async fn healthz(State(state): State<Arc<AppState>>) -> impl IntoResponse {
 	let node = NodeInfo {
 		node_id: state.node_id,
 		api_base_url: state.public_url.clone(),
 		version: env!("CARGO_PKG_VERSION").to_string(),
 	};
 	Json(node)
 }

 pub async fn get_stations(State(state): State<Arc<AppState>>) -> impl IntoResponse {
 	let stations = state.snapshot_registry().await;
 	Json(stations)
 }

pub async fn get_station_by_frequency(State(state): State<Arc<AppState>>, Path(frequency): Path<String>) -> impl IntoResponse {
    let key = match BigDecimal::from_str(&frequency) {
        Ok(d) => normalize_frequency_key(&d),
        Err(_) => return (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: "invalid frequency".into() })).into_response(),
    };
    match state.get_assignment_by_key(&key).await {
        Some(a) => (StatusCode::OK, Json(a)).into_response(),
        None => (StatusCode::NOT_FOUND, Json(ErrorResponse { error: format!("frequency '{}' not found", frequency) })).into_response(),
    }
 }

 pub async fn events_sse(State(state): State<Arc<AppState>>) -> impl IntoResponse {
 	let rx = state.events_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|evt| {
        match evt {
            Ok(e) => {
                let json = serde_json::to_string(&e).unwrap_or_else(|_| "{}".into());
                Some(Ok::<Event, Infallible>(Event::default().data(json)))
            }
            Err(_) => None,
        }
    });
 	Sse::new(stream)
 }

pub async fn now_playing(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.get_now_playing().await {
        Some(np) => (StatusCode::OK, Json(np)).into_response(),
        None => (StatusCode::NO_CONTENT, Body::empty()).into_response(),
    }
}

pub async fn now_events_sse(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let rx = state.now_tx.subscribe();
    let broadcast_stream = BroadcastStream::new(rx).filter_map(|evt| {
        match evt {
            Ok(e) => {
                let json = serde_json::to_string(&e).unwrap_or_else(|_| "{}".into());
                Some(Ok::<Event, Infallible>(Event::default().data(json)))
            }
            Err(_) => None,
        }
    });
    // Send an initial event with current state if available, using boxed stream to unify types
    let stream: Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>> = if let Some(np) = state.get_now_playing().await {
        let json = serde_json::to_string(&np).unwrap_or_else(|_| "{}".into());
        let init = once(Ok::<Event, Infallible>(Event::default().data(json)));
        Box::pin(init.chain(broadcast_stream))
    } else {
        Box::pin(broadcast_stream)
    };
    Sse::new(stream)
}

#[derive(Debug, Deserialize)]
pub struct StreamQuery {
 	content_type: Option<String>,
 }

pub async fn stream_audio(State(state): State<Arc<AppState>>, Query(q): Query<StreamQuery>) -> impl IntoResponse {
 	let mime = q.content_type.unwrap_or_else(|| "audio/mpeg".to_string());
 	let rx = state.audio_tx.subscribe();
    let body_stream = BroadcastStream::new(rx)
        .filter_map(|item| item.ok())
        .map(|bytes| Ok::<bytes::Bytes, std::io::Error>(bytes));
    let content_type = HeaderValue::from_str(&mime).unwrap_or(HeaderValue::from_static("audio/mpeg"));
    let body = Body::from_stream(body_stream);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))
        .header("Cross-Origin-Resource-Policy", HeaderValue::from_static("cross-origin"))
        .body(body)
        .unwrap()
 }

pub async fn put_source(State(state): State<Arc<AppState>>, headers: HeaderMap, body: Body) -> Response {
 	if let Some(expected) = &state.source_token {
 		let Some(auth) = headers.get(header::AUTHORIZATION) else {
            return (StatusCode::UNAUTHORIZED, Json(ErrorResponse { error: "missing Authorization header".into() })).into_response();
 		};
 		let auth = auth.to_str().unwrap_or("");
 		if auth != format!("Bearer {}", expected) {
            return (StatusCode::UNAUTHORIZED, Json(ErrorResponse { error: "invalid Authorization token".into() })).into_response();
 		}
 	}

 	let mut stream = body.into_data_stream();
 	while let Some(chunk) = stream.next().await {
 		match chunk {
 			Ok(bytes) => {
 				let _ = state.audio_tx.send(bytes);
 			}
 			Err(err) => {
 				error!(error=%err, "error reading source stream");
 				break;
 			}
 		}
 	}
    StatusCode::NO_CONTENT.into_response()
 }



// Global middleware to enforce IP blocklist
pub async fn blocklist_middleware(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    // Prefer ConnectInfo if available
    if let Some(ci) = req.extensions().get::<ConnectInfo<SocketAddr>>() {
        let ip = ci.0.ip();
        if state.is_ip_blocked(&ip).await {
            return (
                StatusCode::FORBIDDEN,
                Json(ErrorResponse { error: "blocked".into() })
            ).into_response();
        }
    }
    next.run(req).await
}


