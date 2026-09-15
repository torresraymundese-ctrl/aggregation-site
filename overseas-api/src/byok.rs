use aes_gcm::{
    aead::{rand_core::RngCore, Aead, OsRng},
    Aes256Gcm, KeyInit, Nonce,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Serialize;
use sha2::{Digest, Sha256};

const MASTER_KEY_ENV: &str = "BYOK_MASTER_KEY_B64";

#[derive(Debug, thiserror::Error)]
pub enum ByokError {
    #[error("BYOK_MASTER_KEY_B64 must be set")]
    MissingMasterKey,
    #[error("BYOK_MASTER_KEY_B64 must be valid Base64")]
    InvalidBase64,
    #[error("BYOK_MASTER_KEY_B64 must decode to exactly 32 bytes")]
    InvalidKeyLength,
    #[error("credential encryption failed")]
    EncryptionFailed,
    #[error("credential decryption failed")]
    DecryptionFailed,
    #[error("decrypted credential is not valid UTF-8")]
    InvalidPlaintext,
}

#[derive(Clone)]
pub struct ByokCipher {
    key: [u8; 32],
    key_id: String,
}

#[derive(Debug)]
pub struct EncryptedCredential {
    pub ciphertext: Vec<u8>,
    pub nonce: [u8; 12],
}

#[derive(Debug, Serialize)]
pub struct ProviderCredentialMetadata {
    pub uid: String,
    pub provider_id: i64,
    pub provider_name: String,
    pub name: String,
    pub key_prefix: String,
    pub encryption_key_id: String,
    pub status: i16,
    pub last_used_at: Option<chrono::NaiveDateTime>,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

impl ByokCipher {
    pub fn from_env() -> Result<Self, ByokError> {
        let encoded = std::env::var(MASTER_KEY_ENV).map_err(|_| ByokError::MissingMasterKey)?;
        Self::from_base64(&encoded)
    }

    pub fn from_base64(encoded: &str) -> Result<Self, ByokError> {
        let decoded = STANDARD
            .decode(encoded.trim())
            .map_err(|_| ByokError::InvalidBase64)?;
        let key: [u8; 32] = decoded
            .try_into()
            .map_err(|_| ByokError::InvalidKeyLength)?;
        let digest = Sha256::digest(key);
        let key_id = format!("mk_{}", hex::encode(&digest[..8]));
        Ok(Self { key, key_id })
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<EncryptedCredential, ByokError> {
        let cipher =
            Aes256Gcm::new_from_slice(&self.key).map_err(|_| ByokError::EncryptionFailed)?;
        let mut nonce = [0_u8; 12];
        OsRng.fill_bytes(&mut nonce);
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce), plaintext.as_bytes())
            .map_err(|_| ByokError::EncryptionFailed)?;
        Ok(EncryptedCredential { ciphertext, nonce })
    }

    pub fn decrypt(&self, ciphertext: &[u8], nonce: &[u8]) -> Result<String, ByokError> {
        if nonce.len() != 12 {
            return Err(ByokError::DecryptionFailed);
        }
        let cipher =
            Aes256Gcm::new_from_slice(&self.key).map_err(|_| ByokError::DecryptionFailed)?;
        let plaintext = cipher
            .decrypt(Nonce::from_slice(nonce), ciphertext)
            .map_err(|_| ByokError::DecryptionFailed)?;
        String::from_utf8(plaintext).map_err(|_| ByokError::InvalidPlaintext)
    }
}

pub fn fingerprint(api_key: &str) -> String {
    hex::encode(Sha256::digest(api_key.as_bytes()))
}

pub fn prefix(api_key: &str) -> String {
    api_key.chars().take(8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD;

    fn cipher(byte: u8) -> ByokCipher {
        ByokCipher::from_base64(&STANDARD.encode([byte; 32])).expect("valid test key")
    }

    #[test]
    fn credential_encrypts_and_decrypts() {
        let cipher = cipher(7);
        let secret = "sk-live-secret-value";
        let encrypted = cipher.encrypt(secret).expect("encryption succeeds");

        assert_ne!(encrypted.ciphertext, secret.as_bytes());
        assert_eq!(encrypted.nonce.len(), 12);
        assert_eq!(
            cipher
                .decrypt(&encrypted.ciphertext, &encrypted.nonce)
                .expect("decryption succeeds"),
            secret
        );
    }

    #[test]
    fn wrong_master_key_cannot_decrypt() {
        let encrypted = cipher(7).encrypt("sk-live-secret-value").unwrap();
        let error = cipher(8).decrypt(&encrypted.ciphertext, &encrypted.nonce);

        assert!(matches!(error, Err(ByokError::DecryptionFailed)));
    }

    #[test]
    fn credential_metadata_never_contains_plaintext() {
        let secret = "sk-live-secret-value";
        let metadata = ProviderCredentialMetadata {
            uid: "PCR_TEST".to_string(),
            provider_id: 1,
            provider_name: "OpenAI".to_string(),
            name: "production".to_string(),
            key_prefix: prefix(secret),
            encryption_key_id: cipher(7).key_id().to_string(),
            status: 0,
            last_used_at: None,
            created_at: chrono::NaiveDateTime::default(),
            updated_at: chrono::NaiveDateTime::default(),
        };

        let response = serde_json::to_string(&metadata).unwrap();
        assert!(!response.contains(secret));
        assert!(!response.contains("api_key"));
        assert!(!response.contains("ciphertext"));
        assert!(!response.contains("nonce"));
    }
}
