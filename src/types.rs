use chrono::{DateTime, Utc};
 use serde::{Deserialize, Serialize};
 use uuid::Uuid;
use bigdecimal::BigDecimal;
use std::str::FromStr;

// Serde helpers to accept numbers or strings for BigDecimal and serialize as string to preserve precision
mod serde_decimal {
    use super::*;
    use serde::{de, Deserializer, Serializer};

    pub fn serialize<S>(v: &BigDecimal, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        s.serialize_str(&v.to_string())
    }

    pub fn deserialize<'de, D>(d: D) -> Result<BigDecimal, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct V;
        impl<'de> de::Visitor<'de> for V {
            type Value = BigDecimal;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "a decimal number or string")
            }
            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                BigDecimal::from_str(v).map_err(|e| E::custom(format!("invalid decimal: {}", e)))
            }
            fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                BigDecimal::from_str(v).map_err(|e| E::custom(format!("invalid decimal: {}", e)))
            }
            fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(BigDecimal::from_str(&format!("{v}"))
                    .map_err(|e| E::custom(format!("invalid decimal: {}", e)))?)
            }
            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(BigDecimal::from(v))
            }
            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(BigDecimal::from(v))
            }
        }
        d.deserialize_any(V)
    }
}

pub fn normalize_frequency_key(f: &BigDecimal) -> String {
    let mut s = f.to_string();
    if s.contains('.') {
        // Trim trailing zeros
        while s.ends_with('0') { s.pop(); }
        if s.ends_with('.') { s.pop(); }
    }
    if s == "-0" { s = "0".to_string(); }
    s
}

 #[derive(Debug, Clone, Serialize, Deserialize)]
 pub struct NodeInfo {
 	pub node_id: Uuid,
 	pub api_base_url: String,
 	pub version: String,
 }

 #[derive(Debug, Clone, Serialize, Deserialize)]
 pub struct PeerInfo {
 	pub node_id: Uuid,
 	pub api_base_url: String,
 	pub last_seen: DateTime<Utc>,
 }

 #[derive(Debug, Clone, Serialize, Deserialize)]
 pub struct StationAdvertisement {
 	pub message_id: Uuid,
 	pub station_id: Uuid,
	#[serde(with = "serde_decimal")]
	pub frequency: BigDecimal,
 	pub name: String,
 	pub stream_url: String,
 	pub advertised_at: DateTime<Utc>,
 	pub ttl_seconds: u32,
    /// Base64 Ed25519 public key of owner (the broadcaster)
    pub owner_public_key: String,
    /// Signature over canonical advertisement bytes
    pub signature: String,
 }

 #[derive(Debug, Clone, Serialize, Deserialize)]
 pub struct StationAssignment {
 	pub station_id: Uuid,
	#[serde(with = "serde_decimal")]
	pub frequency: BigDecimal,
 	pub name: String,
 	pub stream_url: String,
 	pub created_at: DateTime<Utc>,
 	pub last_seen: DateTime<Utc>,
 	pub expires_at: DateTime<Utc>,
    pub owner_public_key: String,
 }

 #[derive(Debug, Clone, Serialize, Deserialize)]
 #[serde(rename_all = "lowercase")]
 pub enum AdvertiseResponseStatus {
 	Accepted,
 	Conflict,
 }

 #[derive(Debug, Clone, Serialize, Deserialize)]
 pub struct AdvertiseResponse {
 	pub status: AdvertiseResponseStatus,
 	pub assigned_to: Option<StationAssignment>,
 	pub reason: Option<String>,
 }

 #[derive(Debug, Clone, Serialize, Deserialize)]
 pub struct RegisterPeerRequest {
 	pub node: NodeInfo,
 }

 #[derive(Debug, Clone, Serialize, Deserialize)]
 pub struct RegisterPeerResponse {
 	pub node: NodeInfo,
 	pub peers: Vec<PeerInfo>,
 	pub registry: Vec<StationAssignment>,
 }

 #[derive(Debug, Clone, Serialize, Deserialize)]
 pub struct ReleaseRequest {
 	pub station_id: Uuid,
	#[serde(with = "serde_decimal")]
	pub frequency: BigDecimal,
 	pub reason: Option<String>,
    /// Signature by owner over canonical release bytes
    pub signature: String,
 }

 #[derive(Debug, Clone, Serialize, Deserialize)]
 pub struct ReleaseResponse {
 	pub released: bool,
 }

 #[derive(Debug, Clone, Serialize, Deserialize)]
 pub struct ErrorResponse {
 	pub error: String,
 }

 #[derive(Debug, Clone, Serialize, Deserialize)]
 pub struct RegistryEvent {
 	/// "upsert" or "delete"
 	pub event: String,
 	pub assignment: StationAssignment,
 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NowPlaying {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub cover_url: Option<String>,
    pub updated_at: DateTime<Utc>,
}


