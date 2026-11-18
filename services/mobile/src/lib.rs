//! FRIDAY Mobile Transport + Live Daemon Integration (M6.1 + M6.2a).
//!
//! The single phone-facing surface of FRIDAY. The Android launcher (M6.2b+) talks
//! only to this service over WebSocket: it performs QR-pair auth, and — via the live
//! integration — **subscribes** to the FRIDAY event bus to stream real planner,
//! capability, recovery and telemetry state, and issues structured commands that
//! are translated into real bus events (never free text, never internal knowledge).
//! All intelligence stays on the Windows PC.

pub mod agent_desktop;
pub mod auth;
pub mod bridge;
pub mod commands;
pub mod context_read;
pub mod desktop_tool;
pub mod desktop_windows;
pub mod error;
pub mod failures;
pub mod goal_runner;
pub mod probe;
pub mod protocol;
pub mod session;
pub mod shell_state;
pub mod speech;
pub mod standing;
pub mod system_tool;
pub mod tinyvoca;
pub mod transport;
pub mod upgrade;
pub mod voice;
pub mod web_search;

/// The next message from a broadcast receiver, or `None` once it is closed. A receiver
/// that fell behind skips what it missed and carries on: `while let Ok(..) = rx.recv()`
/// would end the loop (and with it the goal runner) the first time the bus overflowed.
pub async fn next_event<T: Clone>(rx: &mut tokio::sync::broadcast::Receiver<T>) -> Option<T> {
    use tokio::sync::broadcast::error::RecvError;
    loop {
        match rx.recv().await {
            Ok(v) => return Some(v),
            Err(RecvError::Lagged(skipped)) => {
                tracing::warn!(skipped, "fell behind the event bus; skipping ahead")
            }
            Err(RecvError::Closed) => return None,
        }
    }
}

pub use auth::{sign_pairing, AuthError, AuthRegistry};
pub use bridge::{translate, BusBridge};
pub use commands::{method, CommandGateway};
pub use error::MobileError;
pub use goal_runner::{GoalRunner, GoalRunnerHandle};
pub use probe::{SystemProbe, SystemSample};
pub use protocol::{
    ClientInfo, ClientMessage, Health, ServerMessage, ServiceStatus, SystemHealth, Telemetry,
};
pub use session::{MobileSession, PhoneCapabilities, SessionRegistry};
pub use transport::{Command, WsServer};
pub use upgrade::{is_executable_replaced, is_self_replaced, ExeSnapshot};
pub use voice::{VoiceConfig, VoiceIntake};

#[cfg(test)]
mod tests {
    use super::next_event;

    #[tokio::test]
    async fn a_receiver_that_fell_behind_skips_ahead_and_keeps_going() {
        let (tx, mut rx) = tokio::sync::broadcast::channel(2);
        for n in 0..5 {
            tx.send(n).unwrap();
        }
        // Three were lost; the two newest are still there.
        assert_eq!(next_event(&mut rx).await, Some(3));
        assert_eq!(next_event(&mut rx).await, Some(4));
        drop(tx);
        assert_eq!(next_event(&mut rx).await, None);
    }
}
