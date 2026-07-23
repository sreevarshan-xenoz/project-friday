# scripts/

Build, dev, and release automation helpers.

## Notable scripts

| Path | Purpose |
| ---- | ------- |
| `artifacts/android/build-debug-artifact.ps1` | Reproducible Android debug APK artifact |

Add new automation here (or under `artifacts/`) when a process is repeated more
than once. Prefer documented PowerShell / shell scripts over one-off tribal
knowledge.

Related process docs: `engineering/release_process.md`,
`engineering/certification.md`.
