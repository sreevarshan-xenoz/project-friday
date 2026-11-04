# R5-2-0 — FRIDAY Reasoning Provider Survey (FROZEN)

**Status:** FROZEN research/evidence pass — **no code produced in this phase**.
**Sequence:** R5-2-1 selects the provider architecture from the evidence gathered here.
**Scope question:** *"How can FRIDAY obtain high-quality reasoning capability without forcing every FRIDAY user to own a paid API key?"*
**Commit discipline:** this document is the freeze point. R5-2-0 is complete once it is committed.

---

## 1. What this survey is, and is not

R5-2-0 is a **report**, not an implementation. It answers the scope question by mapping:

1. The **reality** of each reasoning route as of 2026 (Local / Web-session / Free-tier / API / Hybrid).
2. The **policy boundary** that separates the *authorized architecture* from the *trap*.
3. What FRIDAY's **existing fabric already provides** (so R5-2-1 builds on it, never rebuilds it).
4. A **capability map + architecture options** for R5-2-1 to choose from.

It deliberately does **not** pick a winner — that is R5-2-1's job, and only R5-2-0's evidence informs it.

---

## 2. The core policy line (anchor)

There are two shapes of "use the user's ChatGPT":

| | **Authorized architecture** (the goal) | **The trap** (never build) |
|---|---|---|
| Ownership | User owns the account | FRIDAY fabric automation owns/borrows it |
| Authorization | User explicitly authorizes FRIDAY to act as them | FRIDAY pretends to have an API it doesn't |
| Access | FRIDAY uses a *supported*, sanctioned seam (OAuth/device-code/API key/access token) | FRIDAY drives a web UI / reverse-engineers a private endpoint |
| Context | FRIDAY supplies approved OS context (Context Gate consent) | FRIDAY exfiltrates context past the user's intent |
| Provider role | Provider reasons over what FRIDAY shows it | Provider selection drives what FRIDAY sees |

**FRIDAY decides "what reasoning capability do I need"; providers never drive what FRIDAY sees.** The Context Gate is a real OS security boundary, not a UI checkbox.

---

## 3. Route reality (2026 evidence)

### 3.1 LOCAL route — run the model on the user's own hardware
Runtimes: **Ollama** (`localhost:11434/v1`, OpenAI-compatible, exists on Windows/macOS/Linux), **llama.cpp** (llama-server, `localhost:8080/v1`), LM Studio, MLX (Apple).

- **Free / offline / private**: prompts never leave the machine. Best for privacy-sensitive OS context.
- **Hardware reality:**
  - CPU-only floor: 8 GB RAM, no GPU needed. 3–8B models run at ~10–15 tok/s (usable, "fast typist" tier).
  - 7B–8B @ Q4: ~5 GB, needs 16 GB RAM. Good daily usability.
  - 13B–14B @ Q4: ~9–10 GB, needs 32 GB RAM, ~5–8 tok/s (slow but workable).
  - 30B+ on CPU: <3 tok/s — impractical.
  - GPU sweet spot: RTX 3060 12 GB / RTX 4060 Ti 16 GB → 7B–14B at 30–60 tok/s.
  - Unified memory (Apple Silicon / Ryzen AI): a 16 GB M-series Mac runs the 12B tier well.
- **2026 models matter:** Gemma 4 (Apache 2.0, release 2026-04-02) family:
  - `E2B` (~2 GB Q4) — any 8 GB machine.
  - `E4B` (~5 GB Q4) — 16 GB laptop default; safe baseline.
  - `12B` (~7 GB Q4, added June 2026) — 16 GB laptop sweet spot; strong reasoning, multimodal, native tool calling.
  - `26B` MoE (~16 GB Q4) — 24 GB GPU / Mac.
  - `31B` Dense (~20 GB Q4) — workstation.
  - Also on the menu: gpt-oss 20B (~16 GB, OpenAI's open model, native MXFP4), Llama 4, Qwen 3.
- **Gotchas:** Ollama defaults local models to a 4K context window regardless of the model's advertised 128K–256K — must override `num_ctx`. KV cache grows with context; long-context on consumer hardware is memory-bound. Single-file/small-scope reasoning is fine locally; whole-repo cross-file reasoning still favors a cloud model.
- **Verdict:** a genuinely viable free-first layer in 2026. A 16 GB machine runs Gemma 4 12B with tool calling and leaves headroom. Not frontier-class, but real reasoning.

### 3.2 WEB-SESSION route — user's own browser-login account (ChatGPT / Gemini / Claude)
This is the "ChatGPT headless session" candidate, and the research confirms the **critical distinction**:

- **The trap form is real and documented:** projects like `chatgpt-pro-browser` state plainly *"driving the ChatGPT web UI is against OpenAI's Terms of Service."* OpenAI ToS restrict: no account-credential sharing, no reverse-engineering of source, **no automatic/programmatic extraction of data or Output**, **no circumventing rate limits / protective measures / safety mitigations**, no using Output to build competing models. Scraping the web chat is fragile and ToS-violating. **Do not build the OS on an assumption a web interface stays automatable.**

- **BUT there is an official, sanctioned, user-authorized seam — the Codex model.** OpenAI Codex ships two supported sign-in paths for local agents:
  - **Sign in with ChatGPT** — OAuth browser flow; **device-code auth (beta)** for headless machines (`codex login --device-auth`); tokens cached at `~/.codex/auth.json`, auto-refreshed. This gives a local agent **frontier-model access through the user's own subscription, no API key**.
  - **Sign in with API key** — usage-based paid.
  - **Enterprise access tokens** — official authorized non-interactive automation.
  - Custom provider config: `requires_openai_auth = true` (reuse OpenAI auth) or `env_key=""` (**no auth → local models**).
- **What this means for FRIDAY:** the *authorized web-session route is not "drive the web UI"* — it is **adopt the same OAuth/device-code seam Codex uses** so the user's own subscription powers FRIDAY's reasoning. That is inside the policy line. Gemini and Claude equivalents are conceptually the same class (user account + OAuth / API key), but Gemini/Claude ToS were not separately deep-researched in this pass — flagged for R5-2-1 if that route is chosen as a secondary.
- **Caveat:** this path yields OpenAI/ChatGPT-model access, not a generic provider fabric route. It fits FRIDAY's *quality-tier* needs, not the *free-first local* needs.

### 3.3 FREE TIERS + official API — quotas and cost (2026)
- **Gemini API free tier:** input **and** output tokens **free of charge**; free content fine to use for non-commercial-improvement purposes (= data used to improve Google products); **grounding with Google Search/Maps free up to ~500 RPD**; rate limits per-project (RPM/TPM/RPD), reset at midnight Pacific. Upgrading to paid tiers removes "used to improve products" and raises quotas. **No card needed for free tier.**
- **OpenAI API tiers:** Free tier requires an allowed geography, ~$100/month usage limit. Tier 1+ needs paid spend. Not free like Gemini.
- **Provider landscape in FRIDAY already registered:** OpenRouter (Tier-1 free aggregator), OpenGateway (gitlawb, free), SambaNova Cloud (free tier, US-hosted, no card), GitHub Models (free), SiliconFlow (free models, CN-hosted, ~30 req/min), Zhipu GLM (free tier, CN-hosted). CN-hosted providers are gated Free-mode-ONLY for privacy.
- **Verdict:** official free tiers (esp. Gemini) give a real no-cost quality boost on top of local, with per-project quotas that fit FRIDAY's light, per-user usage. Hybrid "local first, escalate to free-quota cloud for harder reasoning, escalate to user-subscription quality tier when needed" is the natural shape.

### 3.4 API route (paid, optional)
OpenAI/Anthropic/Google paid keys via the official API. Highest quality, deterministic cost. **Optional** — for power users who want it, not the default FRIDAY guarantees.

### 3.5 HYBRID route (the synthesis)
FRIDAY routes **by capability requirement, not provider name** — which is exactly what the existing fabric already does (see §4). Local for privacy/cost, free-tier cloud for a quality bump, user-subscription (Codex-style seam) or paid API for the top tier, all behind the Context Gate.

---

## 4. What FRIDAY's fabric ALREADY provides (ground truth)

The survey grounds on existing code, not greenfield assumptions:

- **Requirement-driven routing, never provider names** — `planner_backend.rs:42`; `RequestRequirements` is constraints ("vision, prefer local, free"), never "use gpt-4o".
- **`RoutingMode` enum** — `Fast / Quality / Free / Local / Hybrid` (`providers.rs:58`), with `ModelCapability::Local` (`:72`).
- **Local routes are already wired:**
  - `ollama` provider — `localhost:11434/v1`, **always registered**, ships model `qwen2.5-coder:latest` tagged `Local` (`providers.rs:1540`, `:1546`).
  - `llamacpp` provider — llama-server `localhost:8080/v1`, gated on `LLAMACPP_ENABLED=true|1`, model defaulted to **`gemma4:e4b`** (`:1567`, `:1582`). Already tracks the 2026 local-model sweet spot.
- **Free-first cascade** — `qualification.rs` encodes free-first policy: **privacy is the dominant lever** (`Preference::Preferred/Required/Any`); privacy=Preferred ranks local above remote; privacy=Required excludes remote outright (`qualification.rs:230`, tests `:372`,`:387`).
- **Web-session route is the actual gap** — nothing in the fabric today connects to a user-subscription OAuth/device-code seam (the Codex-class path). This is the R5-2-3 target.
- **Registered free remote providers** — OpenRouter, OpenGateway, SambaNova, GitHub Models, SiliconFlow (CN, Free-only), Zhipu (CN, Free-only).
- **Provision of OS context to providers** — the Context Gate seam (`R5-2-4` in sequence) does not yet exist; today FRIDAY only reasons over planner/agent content, not consented OS context.

---

## 5. Capability map → the R5-2-1 decision surface

| Route | Cost | Privacy | Quality | Offline | Type/effort | Policy | Status in fabric |
|---|---|---|---|---|---|---|---|
| **Local** (Ollama/llama.cpp: Gemma 4 E4B→12B) | Free | Full (data stays) | Mid–good | Yes | Selector drop-in | Clean | **Already wired** (ollama always; llamacpp via env) |
| **Web-session (authorized, Codex-seam)** (user ChatGPT etc.) | Subscription (user's) | User-consented via Context Gate | Frontier | No | New OAuth/device-code adapter | Clean if Codex-style seam; **trap** if web-UI driving | **Gap** (R5-2-3) |
| **Free-tier cloud** (Gemini free, OpenRouter, SambaNova, GitHub) | Free (quota-bound) | Remote, some free-tier "improve products" | Good | No | Low (OpenAI-compatible) | Clean | Largely wired |
| **Paid API** (OpenAI/Anthropic/Google) | $$$ | Remote | Frontier | No | Low | Clean | Add key → works |
| **Hybrid** (context/requirement-driven cascade) | Varies | Dominant-lever privacy | Best-fit | Hybrid | Med (orchestration; mostly exists) | Clean | Partially wired (`RoutingMode::Hybrid` + `select_for`) |

**Working recommendation to hand R5-2-1** (evidence-based, not binding):
1. **R5-2-2 build the smallest real provider** — the **local model reference** first (Gemma 4 via the already-wired ollama/llamacpp seam). Smallest risk, fully free-first, exercises Context Gate, and validates the "FRIDAY owns the reasoning" boundary before any cloud dependency.
2. **R5-2-3 user-authorized web-session** — adopt the **Codex-style OAuth/device-code seam** (official, ToS-clean) as the quality tier, *never* a web-UI driver. This is the sanctioned answer to "who owns a paid API key."
3. **R5-2-4 Context Gate** — the real consent boundary between OS state and any provider.
4. **R5-2-5 FRIDAY reasons from OS context**, with local-first + free-tier cloud default and optional paid/subscription escalation.

---

## 6. Open risks / not yet researched (for R5-2-1)
- Gemini / Claude web-session ToS + permission-model specifics (only the OpenAI/Codex seam was deep-researched here). If a second quality route is desired, search these before committing.
- Whether the Codex OAuth/device-code seam is embedding-licensable by a third-party OS product (it is a reference implementation of a user-authorized flow — likely fine, but needs contract/expertise sign-off before R5-2-3).
- Free-tier quota volatility and "data used to improve products" disclosure burden (must be surfaced in the Context Gate consent UX).

---

## 7. Acceptance boundary for R5-2-0
R5-2-0 is accepted when:
- [x] Local route empirically documented (hardware floor, 2026 models, gotchas) — §3.1.
- [x] Web-session route resolved into its authorized seam vs. trap — §2, §3.2.
- [x] Free-tier/API quotas+cost captured — §3.3.
- [x] Existing fabric mapped (no code changed) — §4.
- [x] Capability map + options for R5-2-1 — §5.
- [ ] Committed as a freeze point (this commit).

**No code was written or touched in R5-2-0.** Next: **R5-2-1** chooses the architecture from this evidence.
