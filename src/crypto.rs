 use ed25519_dalek::{Signature, SigningKey, VerifyingKey, Signer, Verifier};
 use base64::{engine::general_purpose::STANDARD as B64, Engine as _};

pub fn parse_public_key_b64(b64: &str) -> anyhow::Result<VerifyingKey> {
	let bytes = B64.decode(b64)?;
	let vk = VerifyingKey::from_bytes(bytes.as_slice().try_into()?)
		.map_err(|e| anyhow::anyhow!("invalid public key bytes: {}", e))?;
	Ok(vk)
}

pub fn parse_sig_b64(b64: &str) -> anyhow::Result<Signature> {
	let bytes = B64.decode(b64)?;
	let arr: [u8; 64] = bytes.as_slice().try_into()?;
	Ok(Signature::from_bytes(&arr))
}

 pub fn encode_public_key_b64(vk: &VerifyingKey) -> String {
 	B64.encode(vk.to_bytes())
 }

 pub fn encode_signature_b64(sig: &Signature) -> String {
 	B64.encode(sig.to_bytes())
 }

 pub fn sign_bytes(sk: &SigningKey, data: &[u8]) -> Signature {
 	sk.sign(data)
 }

 pub fn verify_bytes(vk: &VerifyingKey, data: &[u8], sig: &Signature) -> anyhow::Result<()> {
 	vk.verify(data, sig).map_err(|e| anyhow::anyhow!("signature verification failed: {}", e))
 }

 pub fn canonicalize_ad_bytes(
 	namespace: &str,
 	frequency_key: &str,
 	station_id: &str,
 	stream_url: &str,
 	advertised_at_rfc3339: &str,
 	ttl_seconds: u32,
 ) -> Vec<u8> {
 	format!(
 		"shortwave:{namespace}:freq={frequency_key};station={station_id};url={stream_url};at={advertised_at_rfc3339};ttl={ttl_seconds}"
 	)
 	.into_bytes()
 }

 pub fn canonicalize_release_bytes(namespace: &str, frequency_key: &str, station_id: &str) -> Vec<u8> {
 	format!("shortwave:{namespace}:freq={frequency_key};station={station_id}").into_bytes()
 }


