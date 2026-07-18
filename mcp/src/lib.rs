//! FRIDAY MCP integration.
//!
//! Modeled as a clean extension of the capability system — not a planner
//! feature. The pieces are deliberately layered so each can land in its own PR:
//!
//! * [`protocol`] — the MCP JSON-RPC state machine ([`protocol::McpClient`]),
//!   transport-agnostic.
//! * [`transport`] — [`transport::Transport`] implementations (stdio now;
//!   WebSocket/SSE later) plus [`transport::ProcessLauncher`] (process lifecycle
//!   kept separate from transport).
//! * [`server`] — a hermetic in-process [`server::FakeMcpServer`] for tests (no
//!   Node required).
//! * `adapter` / `discovery` — land in later PRs (MCP tool → `CapabilitySpec`,
//!   dynamic registry registration + health polling).
//!
//! Nothing here imports the planner's supervisor/scheduler; MCP registers
//! capabilities through the same [`friday_capabilities::spec::CapabilitySpec`]
//! contract native adapters use.

pub mod adapter;
pub mod discovery;
pub mod integration;
pub mod protocol;
pub mod server;
pub mod transport;
pub mod transport_ws;
