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
pub mod error;
pub mod failures;
pub mod goal_runner;
pub mod probe;
pub mod protocol;
pub mod session;
pub mod shell_state;
pub mod standing;
pub mod transport;
pub mod upgrade;
pub mod voice;
pub mod web_search;

pub use auth::{sign_pairing, AuthError, AuthRegistry, PairingChallenge};
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
