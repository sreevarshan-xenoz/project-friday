# frontend/

Desktop UI built with **React + Tauri**. Intended surfaces:

- Status / dashboard
- Permission prompts
- Memory and experience viewers
- Capability / service health
- Plugin and plan previews (simulation reports)

Talks to the core over the protocol transport / local IPC — never embeds planner
or kernel logic.

## Status

⏳ **Planned** (product UI; not required for Alpha Qualification).

For a working remote console today, see the **Android client**
(`clients/android/`) and mobile daemon (`services/mobile`).
