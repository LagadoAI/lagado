//! auth/mod.rs — Authentication and vault unlock.
//!
//! Phase 1: stub interface. Phase 2: Argon2id passphrase → vault key via security/crypto.rs.
//! The vault key unlocks Tier 3 cold memory decryption.

/// Session state after successful auth.
#[derive(Debug, Clone)]
pub struct Session {
    pub user_id:  String,
    pub unlocked: bool,       // vault is decryptable
}

/// Auth result.
#[derive(Debug)]
pub enum AuthResult {
    Success(Session),
    Failed { reason: String },
}

pub struct Auth;

impl Auth {
    pub fn new() -> Self { Self }

    /// Phase 1: auto-unlock using machine passphrase (no user password UI yet).
    /// Phase 2: prompt user for passphrase via vault unlock page.
    pub fn auto_unlock(&self) -> AuthResult {
        let passphrase = crate::security::crypto::machine_passphrase();
        let user_id = format!("{:x}", passphrase.len()); // placeholder
        AuthResult::Success(Session { user_id, unlocked: true })
    }

    /// Derive the vault encryption key for a session.
    pub fn vault_key(session: &Session) -> Vec<u8> {
        if session.unlocked {
            crate::security::crypto::machine_passphrase()
        } else {
            vec![]
        }
    }
}
