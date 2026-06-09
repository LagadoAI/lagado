use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};
use rand::RngCore;
use aes_gcm::aead::OsRng;

const MAX_FAILURES: u32 = 3;
const LOCKOUT_SECS: u64 = 600;

fn data_dir() -> PathBuf {
    std::env::var("LAGADO_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_default();
            PathBuf::from(format!("{home}/.laputa-secure"))
        })
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

#[derive(Serialize, Deserialize)]
struct KeychainFile {
    password_salt: String,
    password_blob: String,
    recovery_salt: String,
    recovery_blob: String,
}

#[derive(Serialize, Deserialize, Default)]
struct LockoutFile {
    failures: u32,
    locked_until: Option<u64>,
}

fn keychain_path() -> PathBuf { data_dir().join("config/keychain.json") }
fn lockout_path() -> PathBuf { data_dir().join("config/lockout.json") }

fn derive(passphrase: &[u8], salt_hex: &str) -> Result<[u8; 32], String> {
    let salt_bytes = hex::decode(salt_hex).map_err(|_| "corrupt salt".to_string())?;
    let salt: [u8; 16] = salt_bytes.try_into().map_err(|_| "corrupt salt".to_string())?;
    crate::security::crypto::derive_key_pub(passphrase, &salt)
}

fn wrap(dek: &[u8], key: &[u8; 32]) -> Result<String, String> {
    let blob = crate::security::crypto::encrypt_with_key(key, dek)?;
    Ok(hex::encode(blob))
}

fn unwrap(blob_hex: &str, key: &[u8; 32]) -> Result<Vec<u8>, String> {
    let blob = hex::decode(blob_hex).map_err(|_| "corrupt blob".to_string())?;
    crate::security::crypto::decrypt_with_key(key, &blob)
}

/// Check and update lockout state. Returns seconds remaining if locked, 0 if clear.
pub fn lockout_check() -> u64 {
    let path = lockout_path();
    let file: LockoutFile = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    if let Some(until) = file.locked_until {
        let now = now_secs();
        if now < until { return until - now; }
    }
    0
}

fn lockout_record_failure() {
    let path = lockout_path();
    let mut file: LockoutFile = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    file.failures += 1;
    if file.failures >= MAX_FAILURES {
        file.locked_until = Some(now_secs() + LOCKOUT_SECS);
    }
    if let Ok(json) = serde_json::to_string(&file) {
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        let _ = std::fs::write(&path, json);
    }
}

fn lockout_reset() {
    let path = lockout_path();
    let file = LockoutFile::default();
    if let Ok(json) = serde_json::to_string(&file) {
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        let _ = std::fs::write(&path, json);
    }
}

pub fn lockout_failures() -> u32 {
    let path = lockout_path();
    let file: LockoutFile = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    file.failures
}

/// Returns true if a keychain file exists on disk.
pub fn keychain_exists() -> bool { keychain_path().exists() }

/// Create a new keychain (signup). Returns the DEK on success.
pub fn keychain_create(password: &str, recovery_phrase: &str) -> Result<Vec<u8>, String> {
    let mut dek = [0u8; 32];
    OsRng.fill_bytes(&mut dek);

    let mut pass_salt = [0u8; 16];
    let mut rec_salt = [0u8; 16];
    OsRng.fill_bytes(&mut pass_salt);
    OsRng.fill_bytes(&mut rec_salt);

    let pass_key = crate::security::crypto::derive_key_pub(password.as_bytes(), &pass_salt)?;
    let rec_key = crate::security::crypto::derive_key_pub(recovery_phrase.as_bytes(), &rec_salt)?;

    let kc = KeychainFile {
        password_salt: hex::encode(pass_salt),
        password_blob: wrap(&dek, &pass_key)?,
        recovery_salt: hex::encode(rec_salt),
        recovery_blob: wrap(&dek, &rec_key)?,
    };

    let path = keychain_path();
    std::fs::create_dir_all(path.parent().unwrap())
        .map_err(|e| format!("failed to create config dir: {e}"))?;
    let json = serde_json::to_string_pretty(&kc)
        .map_err(|e| format!("serialize failed: {e}"))?;
    std::fs::write(&path, json)
        .map_err(|e| format!("write keychain failed: {e}"))?;

    lockout_reset();
    Ok(dek.to_vec())
}

/// Unlock the vault with a password. Returns the DEK on success.
/// Records a failure and returns Err on wrong password.
pub fn keychain_unlock(password: &str) -> Result<Vec<u8>, String> {
    let remaining = lockout_check();
    if remaining > 0 {
        return Err(format!("locked:{remaining}"));
    }

    let path = keychain_path();
    let json = std::fs::read_to_string(&path)
        .map_err(|_| "keychain not found".to_string())?;
    let kc: KeychainFile = serde_json::from_str(&json)
        .map_err(|_| "keychain corrupt".to_string())?;

    let key = derive(password.as_bytes(), &kc.password_salt)?;
    match unwrap(&kc.password_blob, &key) {
        Ok(dek) => {
            lockout_reset();
            Ok(dek)
        }
        Err(_) => {
            lockout_record_failure();
            let remaining = lockout_check();
            if remaining > 0 {
                Err(format!("locked:{remaining}"))
            } else {
                let left = MAX_FAILURES - lockout_failures();
                Err(format!("wrong_password:{left}"))
            }
        }
    }
}

/// Recover access using the recovery phrase and set a new password. Returns the DEK.
pub fn keychain_recover(recovery_phrase: &str, new_password: &str) -> Result<Vec<u8>, String> {
    let path = keychain_path();
    let json = std::fs::read_to_string(&path)
        .map_err(|_| "keychain not found".to_string())?;
    let mut kc: KeychainFile = serde_json::from_str(&json)
        .map_err(|_| "keychain corrupt".to_string())?;

    let rec_key = derive(recovery_phrase.as_bytes(), &kc.recovery_salt)?;
    let dek = unwrap(&kc.recovery_blob, &rec_key)
        .map_err(|_| "wrong recovery phrase".to_string())?;

    let mut new_salt = [0u8; 16];
    OsRng.fill_bytes(&mut new_salt);
    let new_key = crate::security::crypto::derive_key_pub(new_password.as_bytes(), &new_salt)?;

    kc.password_salt = hex::encode(new_salt);
    kc.password_blob = wrap(&dek, &new_key)?;

    let updated = serde_json::to_string_pretty(&kc)
        .map_err(|e| format!("serialize failed: {e}"))?;
    std::fs::write(&path, updated)
        .map_err(|e| format!("write keychain failed: {e}"))?;

    lockout_reset();
    Ok(dek)
}
