use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use thiserror::Error;

pub const KEY_LEN: usize = 32;
pub const NONCE_LEN: usize = 12;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("shared secret must not be empty")]
    EmptySecret,
    #[error("key derivation failed")]
    KeyDerivation,
    #[error("malformed ciphertext")]
    MalformedCiphertext,
    #[error("decryption failed (bad key or tampered message)")]
    Decryption,
    #[error("encryption failed")]
    Encryption,
}

pub fn derive_key(psk: impl AsRef<[u8]>) -> Result<[u8; KEY_LEN], CryptoError> {
    let psk = psk.as_ref();
    if psk.is_empty() {
        return Err(CryptoError::EmptySecret);
    }
    let hkdf = Hkdf::<Sha256>::new(Some(b"eef-node-salt"), psk);
    let mut key = [0_u8; KEY_LEN];
    hkdf.expand(b"eef-node-v1", &mut key)
        .map_err(|_| CryptoError::KeyDerivation)?;
    Ok(key)
}

#[derive(Clone)]
pub struct NodeCrypto {
    key: [u8; KEY_LEN],
}

impl NodeCrypto {
    pub fn new(psk: impl AsRef<[u8]>) -> Result<Self, CryptoError> {
        Ok(Self {
            key: derive_key(psk)?,
        })
    }

    pub fn encrypt(&self, plaintext: &[u8], aad: &[u8]) -> Result<String, CryptoError> {
        let nonce_bytes: [u8; NONCE_LEN] = rand::random();
        let cipher = Aes256Gcm::new_from_slice(&self.key).map_err(|_| CryptoError::Encryption)?;
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce_bytes),
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .map_err(|_| CryptoError::Encryption)?;
        let mut wire = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        wire.extend_from_slice(&nonce_bytes);
        wire.extend_from_slice(&ciphertext);
        Ok(STANDARD.encode(wire))
    }

    pub fn decrypt(&self, token: &str, aad: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let wire = STANDARD
            .decode(token)
            .map_err(|_| CryptoError::MalformedCiphertext)?;
        if wire.len() < NONCE_LEN + 16 {
            return Err(CryptoError::MalformedCiphertext);
        }
        let (nonce, ciphertext) = wire.split_at(NONCE_LEN);
        let cipher = Aes256Gcm::new_from_slice(&self.key).map_err(|_| CryptoError::Decryption)?;
        cipher
            .decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad,
                },
            )
            .map_err(|_| CryptoError::Decryption)
    }

    pub fn sign(&self, data: &[u8]) -> String {
        let mut mac =
            <Hmac<Sha256> as Mac>::new_from_slice(&self.key).expect("HMAC accepts a 256-bit key");
        mac.update(data);
        STANDARD.encode(mac.finalize().into_bytes())
    }

    pub fn verify(&self, data: &[u8], signature: &str) -> bool {
        let Ok(signature) = STANDARD.decode(signature) else {
            return false;
        };
        let Ok(mut mac) = <Hmac<Sha256> as Mac>::new_from_slice(&self.key) else {
            return false;
        };
        mac.update(data);
        mac.verify_slice(&signature).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crypto_round_trip_and_tamper() {
        let crypto = NodeCrypto::new("test-shared-secret").unwrap();
        let token = crypto.encrypt(b"hello", b"eef").unwrap();
        assert_eq!(crypto.decrypt(&token, b"eef").unwrap(), b"hello");
        assert!(crypto.decrypt(&token, b"wrong").is_err());
        assert!(
            NodeCrypto::new("bad")
                .unwrap()
                .decrypt(&token, b"eef")
                .is_err()
        );
    }

    #[test]
    fn crypto_sign_and_verify() {
        let crypto = NodeCrypto::new("test-shared-secret").unwrap();
        let signature = crypto.sign(b"payload");
        assert!(crypto.verify(b"payload", &signature));
        assert!(!crypto.verify(b"other", &signature));
    }
}
