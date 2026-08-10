# scripts/

Build, dev, and release automation helpers.

## Notable scripts

| Path | Purpose |
| ---- | ------- |
| `artifacts/android/build-debug-artifact.ps1` | Reproducible Android debug APK artifact |
| `dev-portal.ps1` | One-command portal boot: launches (or reuses) the Mobile Protocol daemon (`FRIDAY_MOBILE_AUTH=off`) on `:18432` **and** the ReadModelBridge (`friday-bridge`) on `:18433`, then starts the Vite dev server with `VITE_CONSOLE_BRIDGE=1` so the Engineering Console is live, not mocked. `-Port <n>` sets the Vite port; `-KeepDaemon` / `-KeepBridge` leave the respective service up after Vite exits; `-SkipBridge` boots daemon + Vite only; `-Build` forces rebuilds. Reachable from `frontend/` as `npm run dev:portal` (or `npm run dev:portal:keep` for the keep-daemon variant). |

Add new automation here (or under `artifacts/`) when a process is repeated more
than once. Prefer documented PowerShell / shell scripts over one-off tribal
knowledge.

Related process docs: `engineering/release_process.md`,
`engineering/certification.md`.
