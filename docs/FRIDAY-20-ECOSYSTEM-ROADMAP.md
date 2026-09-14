# FRIDAY OS — The 20-Capability Ecosystem Integration Roadmap

> **Document Version**: 1.0.0  
> **Status**: APPROVED ARCHITECTURE  
> **Tracking**: M11 Integration Track  

This document serves as the master specification and implementation roadmap for integrating 20 strategic capabilities curated from high-synergy GitHub repositories into the **Friday OS Cognitive Operating System**.

---

## Strategic Waves Overview

```
Wave 1: Native Bridges & System Foundations (Items 1 - 4)
  │  1. Linux-Link (Rust Wayland Remote Desktop & Phone Pairing)
  │  2. my-quickshell-config (Upgraded Quickshell Shell & Roadmap)
  │  3. xencode (Offline-first AI Coding Engine)
  │  4. llmfit (Hardware Profiler & Automatic Model Selector)
  ▼
Wave 2: Desktop Shell & Visual Intelligence (Items 5 - 9)
  │  5. quickshell-overview (Drag-and-Drop Workspace Overview)
  │  6. thuki (Context-Aware Floating Assistant Companion)
  │  7. NibrasShell (AI-Integrated QML UI Widgets)
  │  8. skwd-wall (Dynamic Wallpaper & Video Selector in Rust)
  │  9. caelestia-dots/shell (Morphing Wayland Shell Architecture)
  ▼
Wave 3: Local Voice & Audio Subsystem (Items 10 - 12)
  │  10. KittenTTS (Ultra-fast <25MB On-Device TTS Engine)
  │  11. OpenVoice / VoiceStudio (Friday & Jarvis Voice Profiles)
  │  12. meetily (Local Whisper Transcription & Audio Engine in Rust)
  ▼
Wave 4: Autonomous Web & System Agents (Items 13 - 17)
  │  13. browser-use (Full Autonomous Web Operator)
  │  14. Agent-Reach (Zero-Fee Web Search & Content Fetcher)
  │  15. CLI-Anything (Universal Agent-Native CLI Adapter)
  │  16. alibaba/page-agent (In-Page GUI DOM Agent)
  │  17. agentic-awesome-skills (Catalog Discovery for Friday Skill Forge)
  ▼
Wave 5: Gateway & Developer Ecosystem (Items 18 - 20)
     18. OmniRoute (Multi-Provider Local AI Gateway & Token Compressor)
     19. charmbracelet/crush (Glamorous Terminal Agent)
     20. avante.nvim (Cursor AI-like Experience for Neovim)
```

---

## Detailed Capability Specifications

### Wave 1: Native Bridges & System Foundations

#### 1. `sreevarshan-xenoz/Linux-Link`
- **Subsystem**: `services/linux-link` & `/usr/bin/friday-link`
- **Tech Stack**: Pure Rust, Wayland `wlr-export-dmabuf` / `screencopy`, Tailscale.
- **Architectural Role**:
  - Provides low-latency (<100ms) remote desktop and device continuity.
  - Implements KDE Connect-like features without heavyweight dependencies: clipboard synchronization, remote keyboard/mouse input, and notification mirroring to Android/companion devices.
- **CLI Commands**:
  - `friday link status` — View active Tailscale peers and connected devices.
  - `friday link pair <device-code>` — Authenticate companion mobile device.
  - `friday link stream` — Start low-latency Wayland headless or desktop stream.
- **Verification Criteria**: Successfully stream Wayland display and mirror clipboard between VM and host over virtual network.

#### 2. `sreevarshan-xenoz/my-quickshell-config`
- **Subsystem**: `/etc/skel/.config/quickshell/` & `services/desktop-shell`
- **Tech Stack**: QML, Quickshell, C++/Qt6.
- **Architectural Role**:
  - Integrates the user's custom desktop enhancements and the `UPGRADE_ROADMAP.md` specs directly into the live session.
  - Adds interactive widgets for AI status, notification filtering, and cognitive telemetry.
- **Verification Criteria**: Quickshell loads without syntax errors; widgets render smoothly on Hyprland.

#### 3. `sreevarshan-xenoz/xencode`
- **Subsystem**: `/usr/bin/xencode` & `services/coding-agent`
- **Tech Stack**: Python/Rust, Local GGUF/Ollama inference.
- **Architectural Role**:
  - Powers offline-first software development and repository editing when no internet connection is available.
  - Plugs into `friday-linux-daemon` as the default local code editing engine.
- **CLI Commands**:
  - `xencode init` — Index local workspace.
  - `xencode ask "<question>"` — Query codebase offline.
- **Verification Criteria**: Runs offline in an air-gapped test VM and edits a sample source file.

#### 4. `AlexsJones/llmfit`
- **Subsystem**: `/usr/bin/llmfit` & `/root/installer` hardware detector
- **Tech Stack**: Rust.
- **Architectural Role**:
  - Runs during system installation or first-boot setup (`friday-firstboot`).
  - Benchmarks CPU capabilities (AVX2/AVX-512), GPU model, and available VRAM/RAM.
  - Automatically selects and suggests the optimal local model tier (e.g., `qwen2.5-coder:1.5b`, `llama3.2:3b`, or `deepseek-r1:8b`).
- **Verification Criteria**: CLI returns hardware recommendations and feeds model selection in the installer.

---

### Wave 2: Desktop Shell & Visual Intelligence

#### 5. `Shanu-Kumawat/quickshell-overview`
- **Subsystem**: Quickshell Overview Module (`~/.config/quickshell/overview/`)
- **Tech Stack**: QML, Quickshell, Hyprland IPC.
- **Architectural Role**:
  - Provides a full-screen or popup workspace overview with live window thumbnails.
  - Supports drag-and-drop window repositioning across workspaces.
- **Keybinding**: `Super + Tab` (toggles workspace overview).
- **Verification Criteria**: Invoking `Super + Tab` renders thumbnails of all open client windows across workspaces.

#### 6. `quiet-node/thuki`
- **Subsystem**: `/usr/bin/thuki` & `friday-ctl assistant`
- **Tech Stack**: Rust, Wayland layer-shell protocol.
- **Architectural Role**:
  - A context-aware floating desktop companion widget.
  - Anchors unobtrusively in a corner, displaying Friday's current goal, cognitive thoughts, or active tasks without stealing focus.
- **Verification Criteria**: Spawns a non-blocking Wayland overlay communicating with `friday-linux-daemon` via IPC socket.

#### 7. `AhmedSaadi0/NibrasShell`
- **Subsystem**: Quickshell AI Interface Components
- **Tech Stack**: QML.
- **Architectural Role**:
  - Supplies modular QML components for displaying multi-turn chat bubbles, code diffs, and confirmation prompts inside the desktop environment.
- **Verification Criteria**: AI chat drawer toggles seamlessly via `Super + Shift + Space`.

#### 8. `liixini/skwd-wall`
- **Subsystem**: `/usr/bin/skwd-wall`
- **Tech Stack**: Rust, `mpvpaper`, `swww`.
- **Architectural Role**:
  - Fast, memory-efficient wallpaper selector supporting static images, dynamic videos, and shader animations.
  - Automatically updates matugen / pywal system color palettes when a new wallpaper is selected.
- **CLI Commands**:
  - `friday wallpaper set <path>` / `skwd-wall select`
- **Verification Criteria**: Changing wallpaper updates system accent colors in real time.

#### 9. `caelestia-dots/shell`
- **Subsystem**: Alternate Desktop Shell Theme (`/usr/share/friday/themes/caelestia`)
- **Tech Stack**: QML, Quickshell.
- **Architectural Role**:
  - Offers a morphing, fluid desktop theme architecture selectable in the installer alongside `dots-hyprland` and `omarchy`.
- **Verification Criteria**: Selecting the theme in the installer boots into the Caelestia layout.

---

### Wave 3: Local Voice & Audio Subsystem

#### 10. `KittenML/KittenTTS`
- **Subsystem**: `/usr/lib/friday/audio/kittentts` & `services/sensors/src/audio.rs`
- **Tech Stack**: Python / ONNX runtime, under 25MB footprint.
- **Architectural Role**:
  - Ships directly in the base ISO to provide immediate, zero-download spoken responses.
  - Sub-second latency for hands-free feedback.
- **CLI Commands**:
  - `friday speak "<text>"`
- **Verification Criteria**: Synthesizes and plays audio through PipeWire in <200ms.

#### 11. `myshell-ai/OpenVoice` / `debpalash/VoiceStudio`
- **Subsystem**: `/usr/lib/friday/audio/voice-cloning`
- **Tech Stack**: PyTorch, ONNX, Audio foundation models.
- **Architectural Role**:
  - Allows users to generate or clone a high-fidelity "Friday" or "Jarvis" voice persona.
  - Bundles reference audio embeddings for out-of-the-box Marvel Friday voice timbre.
- **Verification Criteria**: Speech output matches the custom Friday voice acoustic timbre.

#### 12. `Zackriya-Solutions/meetily`
- **Subsystem**: `/usr/bin/meetily-daemon` & `services/sensors/audio_transcribe`
- **Tech Stack**: Rust, Parakeet / Whisper ONNX, Local Ollama summarization.
- **Architectural Role**:
  - 100% local, continuous meeting and voice note transcriber.
  - Automatically records audio streams (mic or system output), produces timestamped transcripts, and feeds action summaries to `friday-linux-daemon`.
- **CLI Commands**:
  - `friday listen start` / `friday listen stop`
  - `friday meeting summary`
- **Verification Criteria**: Accurately transcribes live microphone input into text without network access.

---

### Wave 4: Autonomous Web & System Agents

#### 13. `browser-use/browser-use`
- **Subsystem**: `/usr/lib/friday/agent/browser-use` & `services/action-runtime/src/adapters/browser.rs`
- **Tech Stack**: Python, Playwright, Chromium.
- **Architectural Role**:
  - Enables autonomous browser execution for high-level user goals (`friday goal "Find the cheapest train ticket from Paris to London"`).
  - Handles DOM parsing, clicks, form inputs, and screenshot feedback loops.
- **Verification Criteria**: Executes a multi-step web search and form interaction autonomously in headless or headful Chromium.

#### 14. `Panniantong/Agent-Reach`
- **Subsystem**: `/usr/lib/friday/agent/agent-reach`
- **Tech Stack**: Python.
- **Architectural Role**:
  - Provides zero-API-fee web search and scraping (GitHub, Reddit, Wikipedia, documentation sites).
  - Eliminates the need for paid Search API keys (Tavily/Google Search) for basic agent retrieval.
- **Verification Criteria**: Agent queries and retrieves full web text results with zero external API credentials.

#### 15. `HKUDS/CLI-Anything`
- **Subsystem**: `/usr/lib/friday/agent/cli-anything`
- **Tech Stack**: Python, Schema reflection.
- **Architectural Role**:
  - Translates installed Linux CLI tools into standardized JSON agent-executable tool contracts.
  - Enables Friday's planner to dynamically discover and use tools like `git`, `ffmpeg`, `docker`, and `pacman` with self-correcting syntax.
- **Verification Criteria**: Generates working agent tool definitions for local Linux binaries on the fly.

#### 16. `alibaba/page-agent`
- **Subsystem**: `/usr/lib/friday/agent/page-agent`
- **Tech Stack**: TypeScript / Node.js.
- **Architectural Role**:
  - In-page GUI agent that interacts directly with browser web interfaces using natural language DOM instructions.
- **Verification Criteria**: Controls an active browser window via natural language commands.

#### 17. `sickn33/agentic-awesome-skills`
- **Subsystem**: `/usr/share/friday/skills/` & `friday-skill-forge`
- **Tech Stack**: Python, YAML/JSON skill manifests.
- **Architectural Role**:
  - Populates Friday's skill catalog with hundreds of production-ready skills (system admin, coding, data analysis, DevOps).
  - Accessible through `friday skill list`, `friday skill search`, and `friday skill install`.
- **Verification Criteria**: Successfully imports and validates a skill into `friday-skill-forge`.

---

### Wave 5: Gateway & Developer Ecosystem

#### 18. `diegosouzapw/OmniRoute`
- **Subsystem**: `/usr/bin/omniroute` & `systemd/system/omniroute.service`
- **Tech Stack**: TypeScript, Node.js.
- **Architectural Role**:
  - Local AI gateway proxy running on `127.0.0.1:8080`.
  - Routes requests across 350+ providers (local Ollama, Gemini, Claude, OpenAI, DeepSeek) with token compression and quota failover.
- **Verification Criteria**: Daemon transparently routes requests through OmniRoute with automatic provider fallback.

#### 19. `charmbracelet/crush`
- **Subsystem**: `/usr/bin/crush`
- **Tech Stack**: Go, Charmbracelet Bubbletea.
- **Architectural Role**:
  - Pre-installed terminal-native agentic coding tool with a beautiful TUI.
  - Available out-of-the-box for terminal users desiring rapid, interactive code sessions.
- **Verification Criteria**: Launching `crush` in `foot` presents an interactive coding interface.

#### 20. `avante-corp/avante.nvim`
- **Subsystem**: `/etc/skel/.config/nvim/`
- **Tech Stack**: Lua, Neovim 0.10+.
- **Architectural Role**:
  - Pre-configured Neovim environment providing Cursor AI-like inline editing, multi-file changes, and AI sidebar in terminal Neovim.
- **Verification Criteria**: Opening Neovim and triggering `:AvanteAsk` connects to local Ollama or configured LLM.

---

## Phased Implementation Schedule

| Wave | Item | Target Component | Status |
| :---: | :---: | :--- | :---: |
| **W1** | 1 | `sreevarshan-xenoz/Linux-Link` (Wayland Remote Desktop) | ⏳ Pending |
| **W1** | 2 | `sreevarshan-xenoz/my-quickshell-config` (Quickshell Enhancements) | ⏳ Pending |
| **W1** | 3 | `sreevarshan-xenoz/xencode` (Offline AI Engine) | ⏳ Pending |
| **W1** | 4 | `AlexsJones/llmfit` (Hardware Profiler & Model Picker) | ⏳ Pending |
| **W2** | 5 | `Shanu-Kumawat/quickshell-overview` (Workspace Overview) | ⏳ Pending |
| **W2** | 6 | `quiet-node/thuki` (Floating Desktop Companion) | ⏳ Pending |
| **W2** | 7 | `AhmedSaadi0/NibrasShell` (AI QML Widgets) | ⏳ Pending |
| **W2** | 8 | `liixini/skwd-wall` (Dynamic Wallpaper Engine) | ⏳ Pending |
| **W2** | 9 | `caelestia-dots/shell` (Alternative Shell Option) | ⏳ Pending |
| **W3** | 10 | `KittenML/KittenTTS` (On-Device Lightweight Voice) | ⏳ Pending |
| **W3** | 11 | `myshell-ai/OpenVoice` (Custom Friday Voice Profile) | ⏳ Pending |
| **W3** | 12 | `Zackriya-Solutions/meetily` (Local Audio Transcription) | ⏳ Pending |
| **W4** | 13 | `browser-use/browser-use` (Autonomous Web Agent) | ⏳ Pending |
| **W4** | 14 | `Panniantong/Agent-Reach` (Zero-Fee Web Search) | ⏳ Pending |
| **W4** | 15 | `HKUDS/CLI-Anything` (Universal Agent CLI Adapter) | ⏳ Pending |
| **W4** | 16 | `alibaba/page-agent` (In-Page GUI Agent) | ⏳ Pending |
| **W4** | 17 | `sickn33/agentic-awesome-skills` (Skill Catalog Integration) | ⏳ Pending |
| **W5** | 18 | `diegosouzapw/OmniRoute` (Local Multi-Model Gateway) | ⏳ Pending |
| **W5** | 19 | `charmbracelet/crush` (TUI Terminal Agent) | ⏳ Pending |
| **W5** | 20 | `avante-corp/avante.nvim` (AI-Powered Neovim) | ⏳ Pending |
