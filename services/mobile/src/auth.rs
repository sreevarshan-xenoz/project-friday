//! QR-pair authentication for the mobile console (Alpha).
//!
//! Flow:
//! 1. PC calls [`AuthRegistry::begin_pairing`] and shows `pairing_code` as a QR.
//! 2. Phone sends `AuthResponse { code, signature }` where
//!    `signature = sha256(challenge || code)` (see [`sign_pairing`]).
//! 3. Server calls [`AuthRegistry::complete_pairing`], validates, returns a `token`.
//! 4. On reconnect the phone sends `Hello { token }`; [`AuthRegistry::validate_token`]
//!    short-circuits the pairing.
//!
//! Alpha scope: this proves the phone *observed* the QR. It is not mutual auth;
//! a real Ed25519 key exchange is a deliberate follow-up.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use uuid::Uuid;

/// How long a pending pairing stays valid before it must be completed.
const PAIRING_TTL: Duration = Duration::from_secs(120);

/// A challenge the PC surfaces for the phone to sign.
#[derive(Debug, Clone)]
pub struct PairingChallenge {
    pub pairing_code: String,
    pub challenge: String,
}

struct Pending {
    challenge: String,
    expires: Instant,
}

/// In-memory registry of pending pairings and granted tokens.
pub struct AuthRegistry {
    pending: Mutex<HashMap<String, Pending>>,
    tokens: Mutex<HashMap<String, ()>>,
}

impl Default for AuthRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl AuthRegistry {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            tokens: Mutex::new(HashMap::new()),
        }
    }

    /// Start a pairing: returns the challenge to render as a QR. The code is a
    /// short, human-readable string; the challenge is a high-entropy secret the
    /// phone must sign.
    pub fn begin_pairing(&self) -> PairingChallenge {
        let pairing_code = {
            let mut rng = rand::thread_rng();
            let n: u32 = rand::Rng::gen_range(&mut rng, 100000..1000000);
            format!("{n}")
        };
        let challenge = {
            let mut buf = [0u8; 16];
            rand::Rng::fill(&mut rand::thread_rng(), &mut buf);
            hex_encode(&buf)
        };
        self.pending.lock().unwrap().insert(
            pairing_code.clone(),
            Pending {
                challenge: challenge.clone(),
                expires: Instant::now() + PAIRING_TTL,
            },
        );
        PairingChallenge {
            pairing_code,
            challenge,
        }
    }

    /// Complete a pairing: validate the signature and, on success, mint a token.
    pub fn complete_pairing(&self, code: &str, signature: &str) -> Result<String, AuthError> {
        let (challenge, expired) = {
            let pending = self.pending.lock().unwrap();
            match pending.get(code) {
                Some(p) => (p.challenge.clone(), p.expires <= Instant::now()),
                None => return Err(AuthError::Unknown),
            }
        };
        if expired {
            self.pending.lock().unwrap().remove(code);
            return Err(AuthError::Expired);
        }
        let expected = sign_pairing(&challenge, code);
        if expected != signature {
            return Err(AuthError::InvalidSignature);
        }
        self.pending.lock().unwrap().remove(code);
        let token = Uuid::new_v4().to_string();
        self.tokens.lock().unwrap().insert(token.clone(), ());
        Ok(token)
    }

    /// Validate a previously granted token (used on reconnect).
    pub fn validate_token(&self, token: &str) -> bool {
        self.tokens.lock().unwrap().contains_key(token)
    }
}

/// Derive the signature a phone must return for a pairing challenge.
pub fn sign_pairing(challenge: &str, code: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(challenge.as_bytes());
    hasher.update(code.as_bytes());
    hex_encode(&hasher.finalize())
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Auth failure reasons surfaced to the client as an `Error` frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    Unknown,
    Expired,
    InvalidSignature,
}

impl AuthError {
    pub fn code(&self) -> &'static str {
        match self {
            AuthError::Unknown => "pairing_unknown",
            AuthError::Expired => "pairing_expired",
            AuthError::InvalidSignature => "invalid_signature",
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            AuthError::Unknown => "pairing code unknown",
            AuthError::Expired => "pairing expired",
            AuthError::InvalidSignature => "invalid signature",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_pairing_handshake() {
        let reg = AuthRegistry::new();
        let ch = reg.begin_pairing();
        assert_eq!(ch.pairing_code.len(), 6);
        let sig = sign_pairing(&ch.challenge, &ch.pairing_code);
        let token = reg.complete_pairing(&ch.pairing_code, &sig).unwrap();
        assert!(reg.validate_token(&token));
    }

    #[test]
    fn wrong_signature_rejected() {
        let reg = AuthRegistry::new();
        let ch = reg.begin_pairing();
        let res = reg.complete_pairing(&ch.pairing_code, "deadbeef");
        assert_eq!(res.unwrap_err(), AuthError::InvalidSignature);
        assert!(!reg.validate_token("anything"));
    }

    #[test]
    fn unknown_code_rejected() {
        let reg = AuthRegistry::new();
        let sig = sign_pairing("abc", "000000");
        assert_eq!(
            reg.complete_pairing("000000", &sig).unwrap_err(),
            AuthError::Unknown
        );
    }

    #[test]
    fn expired_pairing_rejected() {
        let reg = AuthRegistry::new();
        // Force expiry by manually inserting an already-expired pending entry.
        reg.pending.lock().unwrap().insert(
            "000001".into(),
            Pending {
                challenge: "x".into(),
                expires: Instant::now() - Duration::from_secs(1),
            },
        );
        let sig = sign_pairing("x", "000001");
        assert_eq!(
            reg.complete_pairing("000001", &sig).unwrap_err(),
            AuthError::Expired
        );
    }
}
