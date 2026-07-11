# Security

FRIDAY runs on a real desktop with real consequences. Safety is a first-class
design constraint, not an afterthought.

## Permission layer

Every action that touches the outside world flows through a permission gate
before execution. The gate consults:

1. A static policy of **forbidden** actions (never allowed).
2. A per-session **allow-list** the user has approved.
3. An **explicit approval** prompt for anything not already allowed.

Dangerous operations **never** execute silently.

## Forbidden by default

FRIDAY must NEVER do any of the following without explicit, per-action approval:

- Delete user files or directories.
- Format drives or partitions.
- Send emails or messages on the user's behalf.
- Transfer money or make purchases.
- Install or uninstall software.
- Modify system/security settings.
- Exfiltrate data outside the local machine.

## Secrets

- No secrets are ever committed to the repository.
- Credentials live in OS-backed secure storage or a local `.env` (git-ignored).
- API keys are passed to plugins via `PluginContext.config`, never logged.
- The logger redacts known secret fields.

## Sandboxing

- Plugins run with least privilege.
- Native integrations (C#, terminal) execute in constrained contexts where
  possible.
- Every privileged action writes an immutable audit entry (who, what, when,
  trace id).

## Auditability

The "log everything" rule (`telemetry.rs`) captures voice, planning, errors,
actions, memory, latency, models, and performance. Combined with `trace_id`,
any autonomous decision can be replayed and explained after the fact.

## Supply chain

- Plugins are signed; unsigned plugins are rejected in release builds.
- Dependencies are pinned and audited in CI.
