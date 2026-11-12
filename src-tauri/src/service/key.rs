use crate::state::KeyPairState;
use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signer, SigningKey};
use pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey, LineEnding};
use rand::rngs::OsRng;

pub fn generate_key_pair() -> Result<KeyPairState, String> {
    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key = signing_key.verifying_key();

    let priv_pem = signing_key
        .to_pkcs8_pem(LineEnding::LF)
        .map_err(|e| format!("Failed to encode private key PEM: {e}"))?
        .to_string();

    let pub_der = verifying_key
        .to_public_key_der()
        .map_err(|e| format!("Failed to encode public key SPKI: {e}"))?;
    let public_b64 = STANDARD.encode(pub_der.as_bytes());

    Ok(KeyPairState {
        public_key: Some(public_b64),
        private_key_pem: Some(priv_pem),
    })
}

pub fn sign_data(private_key_pem: &str, data: &str) -> Result<String, String> {
    let signing_key = SigningKey::from_pkcs8_pem(private_key_pem)
        .map_err(|e| format!("Invalid private key PEM: {e}"))?;
    let sig = signing_key.sign(data.as_bytes());
    Ok(STANDARD.encode(sig.to_bytes()))
}
