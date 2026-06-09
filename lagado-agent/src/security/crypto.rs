//! crypto.rs — AES-256-GCM encryption for vault data.
//!
//! Key derivation: Argon2id from passphrase + salt (salt stored alongside ciphertext).
//! Wire format: [16-byte salt][12-byte nonce][ciphertext+16-byte tag]
//! Phase 1: passphrase is the machine ID (no user password UI yet).

use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use argon2::Argon2;
use rand::RngCore;

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;

/// Derive a 256-bit key from a passphrase + raw salt bytes using Argon2id.
fn derive_key(passphrase: &[u8], salt: &[u8; SALT_LEN]) -> Result<[u8; 32], String> {
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(passphrase, salt, &mut key)
        .map_err(|e| format!("key derivation failed: {e}"))?;
    Ok(key)
}

/// Encrypt plaintext. Returns wire-format bytes: [salt][nonce][ciphertext+tag].
pub fn encrypt(plaintext: &[u8], passphrase: &[u8]) -> Result<Vec<u8>, String> {
    // Generate random salt and nonce
    let mut salt = [0u8; SALT_LEN];
    let mut nonce_bytes = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce_bytes);

    let key_bytes = derive_key(passphrase, &salt)?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| format!("encryption failed: {e}"))?;

    // Wire format: salt + nonce + ciphertext
    let mut out = Vec::with_capacity(SALT_LEN + NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypt wire-format bytes back to plaintext.
pub fn decrypt(data: &[u8], passphrase: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < SALT_LEN + NONCE_LEN + 16 {
        return Err("data too short to be valid ciphertext".to_string());
    }
    let salt: [u8; SALT_LEN] = data[..SALT_LEN].try_into().unwrap();
    let nonce_bytes: [u8; NONCE_LEN] = data[SALT_LEN..SALT_LEN + NONCE_LEN].try_into().unwrap();
    let ciphertext = &data[SALT_LEN + NONCE_LEN..];

    let key_bytes = derive_key(passphrase, &salt)?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(&nonce_bytes);

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| format!("decryption failed: {e}"))
}

/// Public re-export of key derivation for use by auth module.
pub fn derive_key_pub(passphrase: &[u8], salt: &[u8; 16]) -> Result<[u8; 32], String> {
    derive_key(passphrase, salt)
}

/// Encrypt plaintext with a pre-derived key. Wire format: [12-byte nonce][ciphertext+tag].
pub fn encrypt_with_key(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>, String> {
    use aes_gcm::aead::KeyInit;
    let mut nonce_bytes = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher.encrypt(nonce, plaintext)
        .map_err(|e| format!("encryption failed: {e}"))?;
    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypt data encrypted with encrypt_with_key.
pub fn decrypt_with_key(key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>, String> {
    use aes_gcm::aead::KeyInit;
    if data.len() < NONCE_LEN + 16 {
        return Err("data too short".to_string());
    }
    let nonce_bytes: [u8; NONCE_LEN] = data[..NONCE_LEN].try_into().unwrap();
    let ciphertext = &data[NONCE_LEN..];
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let nonce = Nonce::from_slice(&nonce_bytes);
    cipher.decrypt(nonce, ciphertext)
        .map_err(|e| format!("decryption failed: {e}"))
}

/// Get the machine-derived passphrase (Phase 1: uses machine ID or hostname).
/// Phase 2: user-provided password via vault unlock UI.
pub fn machine_passphrase() -> Vec<u8> {
    // Try /etc/machine-id (Linux), fall back to hostname
    if let Ok(id) = std::fs::read_to_string("/etc/machine-id") {
        return id.trim().as_bytes().to_vec();
    }
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "lagado-default".to_string())
        .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let plaintext = b"hello lagado vault";
        let pass = b"test-passphrase";
        let encrypted = encrypt(plaintext, pass).unwrap();
        let decrypted = decrypt(&encrypted, pass).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn wrong_passphrase_fails() {
        let encrypted = encrypt(b"secret", b"correct").unwrap();
        assert!(decrypt(&encrypted, b"wrong").is_err());
    }
}
