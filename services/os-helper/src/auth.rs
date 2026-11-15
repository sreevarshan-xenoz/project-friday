//! Polkit-based authorization checking for FRIDAY OS root helper.
//!
//! Gating actions behind PolicyKit ensures FRIDAY cannot invoke privileged
//! operations without explicit authorization rules or interactive user consent.

use async_trait::async_trait;
use std::collections::HashMap;

/// Action ID for system update (`pacman -Syu`).
pub const ACTION_UPDATE: &str = "org.friday.helper.update";

/// Action ID for package installation (`pacman -S --needed`).
pub const ACTION_INSTALL: &str = "org.friday.helper.install";

/// Action ID for package removal (`pacman -R`).
pub const ACTION_REMOVE: &str = "org.friday.helper.remove";

/// Authorization checker for D-Bus callers and action IDs.
#[async_trait]
pub trait Authorizer: Send + Sync {
    /// Checks whether `sender` (unique bus name) is authorized for `action_id`.
    ///
    /// Returns `Ok(true)` if authorized, `Ok(false)` if denied, or `Err(msg)` on failure.
    /// Any non-`Ok(true)` outcome MUST result in denial.
    ///
    /// `package` is the package an install or removal is about: it goes to polkit
    /// as the `package` detail so the prompt can say which one.
    async fn check(
        &self,
        sender: &str,
        action_id: &str,
        package: Option<&str>,
    ) -> Result<bool, String>;
}

/// Real implementation calling PolicyKit `CheckAuthorization` over the system D-Bus.
pub struct PolkitAuthorizer {
    conn: zbus::Connection,
}

impl PolkitAuthorizer {
    pub fn new(conn: zbus::Connection) -> Self {
        Self { conn }
    }
}

#[async_trait]
impl Authorizer for PolkitAuthorizer {
    async fn check(
        &self,
        sender: &str,
        action_id: &str,
        package: Option<&str>,
    ) -> Result<bool, String> {
        let mut subject_details = HashMap::new();
        subject_details.insert("name", zbus::zvariant::Value::from(sender));
        let subject = ("system-bus-name", subject_details);
        let mut details: HashMap<String, String> = HashMap::new();
        if let Some(p) = package {
            details.insert("package".to_string(), p.to_string());
        }
        let flags: u32 = 1; // AllowUserInteraction
        let cancellation_id = "";

        let reply = match self
            .conn
            .call_method(
                Some("org.freedesktop.PolicyKit1"),
                "/org/freedesktop/PolicyKit1/Authority",
                Some("org.freedesktop.PolicyKit1.Authority"),
                "CheckAuthorization",
                &(subject, action_id, details, flags, cancellation_id),
            )
            .await
        {
            Ok(msg) => msg,
            Err(e) => {
                // Any error => NOT authorized
                return Err(format!("polkit CheckAuthorization failed: {e}"));
            }
        };

        let (authorized, _challenge, _details): (bool, bool, HashMap<String, String>) = reply
            .body()
            .deserialize()
            .map_err(|e| format!("polkit reply deserialize error: {e}"))?;

        Ok(authorized)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Stub authorizer for unit testing without connecting to D-Bus.
    pub struct StubAuthorizer {
        authorized: bool,
        should_error: bool,
        calls: Mutex<Vec<(String, String)>>,
        packages: Mutex<Vec<Option<String>>>,
    }

    impl StubAuthorizer {
        pub fn new(authorized: bool) -> Arc<Self> {
            Arc::new(Self {
                authorized,
                should_error: false,
                calls: Mutex::new(Vec::new()),
                packages: Mutex::new(Vec::new()),
            })
        }

        pub fn failing() -> Arc<Self> {
            Arc::new(Self {
                authorized: false,
                should_error: true,
                calls: Mutex::new(Vec::new()),
                packages: Mutex::new(Vec::new()),
            })
        }

        pub fn calls(&self) -> Vec<(String, String)> {
            self.calls.lock().unwrap().clone()
        }

        pub fn call_count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }

        /// The package each check named, in order.
        pub fn packages(&self) -> Vec<Option<String>> {
            self.packages.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl Authorizer for StubAuthorizer {
        async fn check(
            &self,
            sender: &str,
            action_id: &str,
            package: Option<&str>,
        ) -> Result<bool, String> {
            self.calls
                .lock()
                .unwrap()
                .push((sender.to_string(), action_id.to_string()));
            self.packages
                .lock()
                .unwrap()
                .push(package.map(str::to_string));
            if self.should_error {
                return Err("authorizer error: polkit unreachable".into());
            }
            Ok(self.authorized)
        }
    }

    #[tokio::test]
    async fn test_stub_authorizer_behavior() {
        let auth = StubAuthorizer::new(true);
        assert!(auth.check(":1.42", ACTION_INSTALL, None).await.unwrap());
        assert_eq!(auth.calls(), vec![(":1.42".into(), ACTION_INSTALL.into())]);

        let denied = StubAuthorizer::new(false);
        assert!(!denied.check(":1.43", ACTION_REMOVE, None).await.unwrap());

        let err = StubAuthorizer::failing();
        assert!(err.check(":1.44", ACTION_UPDATE, None).await.is_err());
    }
}
