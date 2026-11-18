//! Pairing authentication for the mobile console (Alpha).
//!
//! Flow:
//! 1. The PC calls [`AuthRegistry::begin_pairing`] (or [`AuthRegistry::ensure_pairing`]
//!    when a phone connects) and shows the `pairing_code` on the PC. The code is
//!    never sent over the socket: an unpaired client that could read it from the
//!    wire could pair without ever seeing the PC.
//! 2. An unpaired phone says `Hello`; the server answers `AuthChallenge` with a
//!    fresh per-connection `challenge` (no code in it).
//! 3. The user types the code on the phone, which sends `AuthResponse { code,
//!    signature }` with `signature = sha256(challenge || code)` (see [`sign_pairing`]).
//! 4. [`AuthRegistry::complete_pairing`] validates and returns a `token`.
//! 5. On reconnect the phone sends `Hello { token }`; [`AuthRegistry::validate_token`]
//!    short-circuits the pairing.
//!
//! The code is short, so guessing is limited: [`MAX_FAILURES`] wrong answers
//! discard every pending code and block new ones for [`LOCKOUT`].
//!
//! Alpha scope: this is not mutual auth and not safe against someone who can
//! read the connection; a real key exchange is a deliberate follow-up.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use uuid::Uuid;

/// How long a pending pairing stays valid before it must be completed.
const PAIRING_TTL: Duration = Duration::from_secs(120);
/// Wrong answers allowed before pairing is locked.
const MAX_FAILURES: u32 = 5;
/// How long pairing stays locked after too many wrong answers.
const LOCKOUT: Duration = Duration::from_secs(300);

#[derive(Default)]
struct Guard {
    failures: u32,
    locked_until: Option<Instant>,
}

/// In-memory registry of pending pairings and granted tokens.
pub struct AuthRegistry {
    pending: Mutex<HashMap<String, Instant>>,
    tokens: Mutex<HashMap<String, ()>>,
    guard: Mutex<Guard>,
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
            guard: Mutex::new(Guard::default()),
        }
    }

    /// Start a pairing and return its code, for the PC to show the user.
    pub fn begin_pairing(&self) -> String {
        let n: u32 = rand::Rng::gen_range(&mut rand::thread_rng(), 10_000_000..100_000_000);
        let code = n.to_string();
        self.pending
            .lock()
            .unwrap()
            .insert(code.clone(), Instant::now() + PAIRING_TTL);
        *self.guard.lock().unwrap() = Guard::default();
        code
    }

    /// A fresh code if none is live and pairing isn't locked; `None` otherwise.
    /// Called when an unpaired phone connects, so the PC shows a code to read.
    pub fn ensure_pairing(&self) -> Option<String> {
        let locked = self
            .guard
            .lock()
            .unwrap()
            .locked_until
            .is_some_and(|t| t > Instant::now());
        let live = {
            let mut pending = self.pending.lock().unwrap();
            pending.retain(|_, expires| *expires > Instant::now());
            !pending.is_empty()
        };
        (!locked && !live).then(|| self.begin_pairing())
    }

    /// A random per-connection challenge for the phone to sign.
    pub fn new_challenge(&self) -> String {
        let mut buf = [0u8; 16];
        rand::Rng::fill(&mut rand::thread_rng(), &mut buf);
        hex_encode(&buf)
    }

    /// Complete a pairing: validate the signature of `challenge` and, on
    /// success, mint a token.
    pub fn complete_pairing(
        &self,
        code: &str,
        challenge: &str,
        signature: &str,
    ) -> Result<String, AuthError> {
        {
            let guard = self.guard.lock().unwrap();
            if guard.locked_until.is_some_and(|t| t > Instant::now()) {
                return Err(AuthError::Locked);
            }
        }
        let outcome = match self.pending.lock().unwrap().get(code) {
            None => Err(AuthError::Unknown),
            Some(expires) if *expires <= Instant::now() => Err(AuthError::Expired),
            Some(_) if sign_pairing(challenge, code) != signature => {
                Err(AuthError::InvalidSignature)
            }
            Some(_) => Ok(()),
        };
        if let Err(e) = outcome {
            let mut guard = self.guard.lock().unwrap();
            guard.failures += 1;
            if guard.failures >= MAX_FAILURES {
                guard.locked_until = Some(Instant::now() + LOCKOUT);
                self.pending.lock().unwrap().clear();
                return Err(AuthError::Locked);
            }
            return Err(e);
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
    Locked,
}

impl AuthError {
    pub fn code(&self) -> &'static str {
        match self {
            AuthError::Unknown => "pairing_unknown",
            AuthError::Expired => "pairing_expired",
            AuthError::InvalidSignature => "invalid_signature",
            AuthError::Locked => "pairing_locked",
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            AuthError::Unknown => "pairing code unknown",
            AuthError::Expired => "pairing expired",
            AuthError::InvalidSignature => "invalid signature",
            AuthError::Locked => {
                "too many wrong codes; start pairing again on the PC in a few minutes"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(reg: &AuthRegistry, code: &str) -> Result<String, AuthError> {
        let challenge = reg.new_challenge();
        reg.complete_pairing(code, &challenge, &sign_pairing(&challenge, code))
    }

    #[test]
    fn full_pairing_handshake() {
        let reg = AuthRegistry::new();
        let code = reg.begin_pairing();
        assert_eq!(code.len(), 8);
        let token = pair(&reg, &code).unwrap();
        assert!(reg.validate_token(&token));
    }

    #[test]
    fn wrong_signature_rejected() {
        let reg = AuthRegistry::new();
        let code = reg.begin_pairing();
        let res = reg.complete_pairing(&code, &reg.new_challenge(), "deadbeef");
        assert_eq!(res.unwrap_err(), AuthError::InvalidSignature);
        assert!(!reg.validate_token("anything"));
    }

    #[test]
    fn unknown_code_rejected() {
        let reg = AuthRegistry::new();
        assert_eq!(pair(&reg, "00000000").unwrap_err(), AuthError::Unknown);
    }

    #[test]
    fn expired_pairing_rejected() {
        let reg = AuthRegistry::new();
        reg.pending
            .lock()
            .unwrap()
            .insert("00000001".into(), Instant::now() - Duration::from_secs(1));
        assert_eq!(pair(&reg, "00000001").unwrap_err(), AuthError::Expired);
    }

    #[test]
    fn too_many_wrong_codes_lock_pairing() {
        let reg = AuthRegistry::new();
        let code = reg.begin_pairing();
        for _ in 0..MAX_FAILURES - 1 {
            assert_eq!(pair(&reg, "00000000").unwrap_err(), AuthError::Unknown);
        }
        assert_eq!(pair(&reg, "00000000").unwrap_err(), AuthError::Locked);
        // The real code no longer works, and no new code is offered.
        assert_eq!(pair(&reg, &code).unwrap_err(), AuthError::Locked);
        assert!(reg.ensure_pairing().is_none());
    }

    #[test]
    fn a_code_is_offered_only_when_none_is_live() {
        let reg = AuthRegistry::new();
        let code = reg.ensure_pairing().expect("first phone gets a code");
        assert!(reg.ensure_pairing().is_none());
        assert!(pair(&reg, &code).is_ok());
        assert!(reg.ensure_pairing().is_some());
    }
}
