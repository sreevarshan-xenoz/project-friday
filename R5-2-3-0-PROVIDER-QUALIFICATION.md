# R5-2-3-0 — Provider Qualification Verification (FROZEN)

**Status:** FROZEN official-source verification. **No code in this phase.** Decides **which two providers** R5-2-3-1 integrates first.
**Rule being honored:** *no code until the seam is proven.* Every claim below is checked against **current (2026-08-30) primary/vendor documentation**, not secondary web summaries. If a provider fails, we swap it — the R5-2-1 architecture is provider-agnostic and does not change.
**Sources:** `github.com/github/copilot-sdk` (primary repo + docs, MIT), GitHub changelog (2026-06-02 GA), `console.groq.com/docs` (official rate-limits/openai/models pages).

---

## 1. What we are qualifying
Per R5-2-2 recommendation, two seams, each proving one architectural case:
- **Copilot SDK** = the **official account-auth** case: FRIDAY obtains reasoning through a user's **existing subscription** without FRIDAY possessing an API key.
- **Groq** = the **zero-subscription free-remote floor**.

Adding Gemini/OpenRouter/Cerebras later is provider-specific qualification (R5-2-X), not architecture work.

---

## 2. Copilot SDK — VERIFIED ✅ (account-auth case)

| Criterion | Evidence (primary source) | Verdict |
|---|---|---|
| **Availability / maturity** | **Generally Available** since 2026-06-02 (GitHub changelog), stable API, semver, production-ready; repo `github.com/github/copilot-sdk` | ✅ GA |
| **Language surface incl. FRIDAY's Rust** | Officially ships **Node/TS, Python, Go, .NET, Java, Rust** (`cargo add github-copilot-sdk`, API docs at docs.rs) | ✅ Rust available |
| **Auth flow** | "GitHub signed-in user" (OAuth creds from `copilot` CLI login), **OAuth GitHub App** (pass user tokens), env tokens (`COPILOT_GITHUB_TOKEN`/`GH_TOKEN`/`GITHUB_TOKEN`), BYOK. Token types: `gho_` (OAuth), `ghu_` (GitHub App), `github_pat_` (fine-grained PAT); `ghp_` classic deprecated | ✅ official OAuth |
| **Third-party use of user's entitlement** | *"Each user authenticates with their own GitHub account… Copilot usage is billed to each user's subscription. **Your app never handles model API keys—GitHub manages everything.**"* Multi-user: per-user SDK client / per-session `gitHubToken` | ✅ **the core case, explicitly supported** |
| **Models** | All Copilot CLI models supported; runtime model enumeration method; `model: "auto"` | ✅ |
| **Linux / headless / server** | Designed for server/backend: `mode: "empty"` (disables ambient OS tools, safe multi-user baseline), per-session tokens, `COPILOT_HOME` isolation, `RuntimeConnection.forUri` external-runtime connection, tool permission handlers, OpenTelemetry | ✅ headless-capable |
| **LICENSE** | **MIT** (repo LICENSE + README footer) | ✅ redistributable in an OS |
| **Subscription requirement** | Copilot subscription required **unless BYOK**. **Copilot Free (personal) supported** — "available to all existing GitHub Copilot subscribers, including Copilot Free for personal use, and to non-Copilot users via BYOK" | ✅ (free tier exists; BYOK fallback for non-subscribers) |
| **Billing model** | SDK usage counted toward each user's Copilot allowance (same as Copilot CLI) | ✅ predictable per-user |

**Verdict: QUALIFY.** Copilot SDK is the officially supported "bring-your-own-subscription to a third-party app" mechanism, Rust + MIT + headless-ready, and is the strongest possible account-auth seam for FRIDAY.

### R5-2-3-0 decision detail
- **Primary auth for FRIDAY:** GitHub OAuth App, per-user tokens, `mode: "empty"` + per-session token + explicit-tool policy (mirrors R5-2-1 §4 sanctioned-seam rule; never a web-UI driver).
- **No-key guarantee holds:** FRIDAY stores/refreshes the user's GitHub OAuth token (its own responsibility) but never any provider model key.
- **Open item carried to R5-2-3-1:** confirm exact Copilot Free tier limits and that a Rust-side bundling of the CLI is acceptable in FRIDAY's distribution (Rust SDK requires `copilot` CLI in PATH or app-level bundling).

---

## 3. Groq — VERIFIED ✅ (free-remote floor)

| Criterion | Evidence (primary: console.groq.com/docs) | Verdict |
|---|---|---|
| **Free tier exists, no card** | Official **Free Plan** rate-limits page | ✅ |
| **Free tier limits (current)** | Free Plan: e.g. `openai/gpt-oss-120b` 30 RPM / 1K RPD / 8K TPM / 200K TPD; `openai/gpt-oss-20b` same; `qwen/qwen3.6-27b`, `qwen/qwen3.8-27b` (2M TPD); whisper. Limits apply **per organization**; 429 + `retry-after` + rate-limit headers returned so clients can back off and rotate | ✅ generous for FRIDAY's light per-user load |
| **OpenAI-compatible API** | Official OpenAI-compatibility page: set `base_url` to `https://api.groq.com/openai/v1`, pass `GROQ_API_KEY` | ✅ drop-in for FRIDAY's existing `OpenAiProvider` |
| **Supported models** | Official models page + live `GET /v1/models` catalog endpoint; open-weight (GPT-OSS 120B/20B, Llama, Qwen, Mistral, etc.) + tool calling, reasoning, structured outputs, browser search, code execution | ✅ |
| **No card required to start** | Official free tier; sign up with email | ✅ |
| **Local-first / no account needed** | Groq needs a (free) API key — it is the *zero-subscription* floor, not *zero-key*; the true zero-key floor remains local Ollama/llama.cpp (already wired) as privacy=Required fallback | ✅ free remote floor |

**Verdict: QUALIFY.** Groq is the official, OpenAI-compatible, no-card free-remote floor and drops directly into the existing `OpenAiProvider` seam.

---

## 4. Frozen decision for R5-2-3-1

**Real** the following two adapters through the existing `InferenceFabric` (provider-agnostic architecture unchanged):
1. **Copilot SDK** — account-auth provider (user's existing GitHub Copilot entitlement, incl. Copilot Free; FRIDAY never holds a model key).
2. **Groq** — free-remote provider (no subscription; official OpenAI-compatible endpoint).

**Fabric mapping (existing, no architecture change):**
- **Groq** → existing `OpenAiProvider { base_url: https://api.groq.com/openai/v1 }`, registered like the other free providers (OpenRouter/SambaNova/GitHub Models already are); capability-tagged planning/coding models; free-first.
- **Copilot** → a new minimally scoped adapter class under the "account-auth / subscription" class in the R5-2-1 hierarchy (local > free-remote > **subscription** > paid API). It shells to the Rust Copilot SDK session (a small bridge), returning reasoning through `ModelCapability`-tagged models with `user` entitlement routing.
- **Fallback (R5-2-3-2):** free-remote (Groq) → user subscription (Copilot) → local (privacy=Required) uses the existing health-score + circuit-breaker + requirement-driven `select_for`; no pinning, no hard-coded dead endpoints (free tiers churn).

**Swap rule:** `R5-2-3-0` verified BOTH pass, so no swap needed now. If R5-2-3-1 uncovers a blocking integration issue (e.g. Rust CLI-bundling licensing inside the OS image), we substitute the strongest verified alternative (e.g. Codex device-code, or Gemini free tier) without touching the architecture.

---

## 5. Acceptance boundary for R5-2-3-0
- [x] Copilot SDK verified against primary sources on all criteria (GA, auth, third-party entitlement use, models, Rust/Linux/headless, MIT, OS-distribution, free tier) — §2.
- [x] Groq verified against primary docs (free tier terms, OpenAI-compatible access, models) — §3.
- [x] Two providers frozen for R5-2-3-1; fabrric mapping + swap rule stated — §4.
- [x] Committed as this freeze point.

**No code was written or touched in R5-2-3-0.** Next: **R5-2-3-1** — minimal fabric integration of Copilot (account-auth) + Groq (free-remote) through the existing `InferenceFabric`.
