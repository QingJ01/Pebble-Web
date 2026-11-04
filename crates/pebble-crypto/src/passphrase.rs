use pebble_core::{PebbleError, Result};
use rand::RngCore;
use ring::pbkdf2;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use zeroize::Zeroizing;

const SALT_SIZE: usize = 16;
const AES_GCM_MIN_PAYLOAD_SIZE: usize = 12 + 16;
const DEFAULT_PBKDF2_ITERATIONS: u32 = 210_000;
const ALGORITHM: &str = "aes-256-gcm";
const KDF: &str = "pbkdf2-hmac-sha256";
const BACKUP_SECRET_AAD: &[u8] = b"pebble.backup.secrets.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PassphraseEncryptedBlob {
    pub algorithm: String,
    pub kdf: String,
    pub iterations: u32,
    pub salt_hex: String,
    pub ciphertext_hex: String,
}

pub fn encrypt_with_passphrase(
    plaintext: &[u8],
    passphrase: &str,
) -> Result<PassphraseEncryptedBlob> {
    validate_passphrase(passphrase)?;

    let mut salt = [0u8; SALT_SIZE];
    rand::thread_rng().fill_bytes(&mut salt);
    let key = derive_key(passphrase, &salt, DEFAULT_PBKDF2_ITERATIONS)?;
    let ciphertext = crate::aes::encrypt_enveloped(&key, plaintext, BACKUP_SECRET_AAD)?;

    Ok(PassphraseEncryptedBlob {
        algorithm: ALGORITHM.to_string(),
        kdf: KDF.to_string(),
        iterations: DEFAULT_PBKDF2_ITERATIONS,
        salt_hex: hex_encode(&salt),
        ciphertext_hex: hex_encode(&ciphertext),
    })
}

pub fn decrypt_with_passphrase(
    blob: &PassphraseEncryptedBlob,
    passphrase: &str,
) -> Result<Vec<u8>> {
    validate_passphrase(passphrase)?;
    validate_blob(blob)?;

    let salt = hex_decode(&blob.salt_hex)?;
    if salt.len() != SALT_SIZE {
        return Err(PebbleError::Validation(format!(
            "Invalid backup secret salt length: expected {SALT_SIZE} bytes, got {}",
            salt.len()
        )));
    }
    let ciphertext = hex_decode(&blob.ciphertext_hex)?;
    if ciphertext.len() < AES_GCM_MIN_PAYLOAD_SIZE {
        return Err(PebbleError::Validation(
            "Invalid backup secret ciphertext length".to_string(),
        ));
    }
    let key = derive_key(passphrase, &salt, blob.iterations)?;

    Ok(crate::aes::decrypt_enveloped(&key, &ciphertext, BACKUP_SECRET_AAD)?.plaintext)
}

fn validate_passphrase(passphrase: &str) -> Result<()> {
    if passphrase.trim().is_empty() {
        return Err(PebbleError::Validation(
            "Backup encryption passphrase is required".to_string(),
        ));
    }
    Ok(())
}

fn validate_blob(blob: &PassphraseEncryptedBlob) -> Result<()> {
    if blob.algorithm != ALGORITHM {
        return Err(PebbleError::Validation(format!(
            "Unsupported backup secret encryption algorithm: {}",
            blob.algorithm
        )));
    }
    if blob.kdf != KDF {
        return Err(PebbleError::Validation(format!(
            "Unsupported backup secret KDF: {}",
            blob.kdf
        )));
    }
    if blob.iterations != DEFAULT_PBKDF2_ITERATIONS {
        return Err(PebbleError::Validation(format!(
            "Unsupported backup secret KDF iterations: {}",
            blob.iterations
        )));
    }
    Ok(())
}

fn derive_key(passphrase: &str, salt: &[u8], iterations: u32) -> Result<Zeroizing<[u8; 32]>> {
    let iterations = NonZeroU32::new(iterations).ok_or_else(|| {
        PebbleError::Validation(
            "Backup secret KDF iterations must be greater than zero".to_string(),
        )
    })?;
    let mut key = Zeroizing::new([0u8; 32]);
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        iterations,
        salt,
        passphrase.as_bytes(),
        key.as_mut(),
    );
    Ok(key)
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode(s: &str) -> Result<Vec<u8>> {
    hex::decode(s)
        .map_err(|error| PebbleError::Validation(format!("Invalid backup secret hex: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passphrase_encryption_emits_v1_envelope() {
        let encrypted = encrypt_with_passphrase(b"backup secrets", "passphrase").unwrap();
        let ciphertext = hex_decode(&encrypted.ciphertext_hex).unwrap();

        assert!(ciphertext.starts_with(b"PEBBLE\0\x01"));
    }

    #[test]
    fn passphrase_decryption_reads_legacy_ciphertext() {
        let mut encrypted = encrypt_with_passphrase(b"placeholder", "passphrase").unwrap();
        let salt = hex_decode(&encrypted.salt_hex).unwrap();
        let key = derive_key("passphrase", &salt, encrypted.iterations).unwrap();
        let legacy = crate::aes::encrypt(&key, b"legacy backup secrets").unwrap();
        encrypted.ciphertext_hex = hex_encode(&legacy);

        assert_eq!(
            decrypt_with_passphrase(&encrypted, "passphrase").unwrap(),
            b"legacy backup secrets"
        );
    }
}
