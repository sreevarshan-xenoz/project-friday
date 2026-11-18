use std::sync::Once;

static INIT: Once = Once::new();

/// Initialise the global tracing subscriber. Safe to call multiple times; only
/// the first call has an effect. Honours `RUST_LOG` / `FRIDAY_LOG` when set.
///
/// This is the single entry point for the "log everything" requirement: voice,
/// planning, errors, actions, memory, latency, models and performance all flow
/// through `tracing` and are captured centrally.
pub fn init() {
    INIT.call_once(|| {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "info".into()),
            )
            .with_target(true)
            // Colour only on a terminal: the tray and systemd write this to log
            // files, where escape codes make it unreadable.
            .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stdout()))
            .try_init();
    });
}
