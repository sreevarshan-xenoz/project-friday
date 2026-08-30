# R5-2-2 — Free Remote & Account-Authenticated Provider Survey (FROZEN)

**Status:** FROZEN research pass. **No code produced in this phase.**
**Correction applied (from R5-1 decision):** R5-2-2 is **not** a local-model build. The goal is:
> **FRIDAY should use capable remote reasoning services, preferably ones the user can access without buying an API key.**

The already-wired local seam (ollama + `llamacpp`/`gemma4:e4b`) remains available as the privacy=Required fallback, but is **not** the centerpiece of R5-2.
**Sequence:** R5-2-1 architecture (frozen) → **R5-2-2 this survey** → R5-2-3 account-authenticated providers → R5-2-4 Context Gate → R5-2-5 OS-level reasoning.
**Commit discipline:** R5-2-2 is a freeze point; R5-2-3 begins only after this is committed.

---

## 1. The three-way classification (governs everything)

Every candidate is classified by how FRIDAY would use it:

| Method | FRIDAY uses | Worth pursuing | Legal/technical |
|---|---|---|---|
| **Official API + user's API key** | API endpoint | ✅ | Costs may apply, but clean & official |
| **Official OAuth / account integration** | User's existing account | ⭐ **the target to investigate** | Washes clean if the provider publishes a sanctioned seam |
| **Automating a website's logged-in UI / headless browser** | ChatGPT/Gemini website session | ⚠️ research only | Potentially prohibited (ToS) & fragile — see R5-2-0 §2 trap |

**FRIDAY's headless-websession idea lives in category 3** — it is a specifically marked **research branch**, investigated for policy understanding, **never** an unexamined implementation default and **never** the R5-2 centerpiece.

---

## 2. FREE REMOTE — the "no API key, no subscription" floor (2026)

These give FRIDAY a real free-first brain **without owning anything**. All are **permanent** free tiers (rate-limited), not trials.

| Provider | No card? | Current free tier (late 2026) | Notable free models | OpenAI-compatible |
|---|---|---|---|---|
| **Groq** | ✅ | 30 RPM, 14,400 RPD, ~500K tok/day (Llama 3.1 8B); sub-second LPU speed | Llama 3.3 70B, GPT-OSS, Qwen3 32B, DeepSeek R1 distill | ✅ |
| **Gemini API (AI Studio)** | ✅ | ~10–15 RPM, up to 1,500 RPD; free input+output tokens | **Flash / Flash-Lite only** (Pro moved behind billing in 2026) | SDK |
| **Cerebras** | ✅ | ~1M tokens/day, extreme tok/s (2.6k) | Llama models | ✅ |
| **OpenRouter** | ✅ | 20 RPM; ~50 RPD (≈1,000/day after ever buying $10 credits); **volatile pool** | Rotating open-weight, occasionally frontier-free (Nemotron Ultra 1M ctx, gpt-oss-20b) | ✅ (already wired) |
| **SambaNova Cloud** | ✅ | 20 req/day/model, 200K tok/day (eval budget); one-off $5 credit | Llama 3.3 70B, DeepSeek V3.1, gpt-oss-120b @ ~400 tok/s | ✅ (already wired) |
| **Cerebras / Cohere / NVIDIA NIM / GitHub Models / Mistral / Hugging Face / Cloudflare Workers AI** | ✅ permanent | Various | Open-weight + some frontier | ✅ |

### Key truths for FRIDAY
- **The "capable remote without buying a key" requirement is fully satisfiable today** — Groq (speed) and Gemini Flash (quality) are strong, generous, permanent, no-card free tiers, and FRIDAY's fabric already knows `groq`, `sambanova`, `openrouter`, `github-models`.
- **Never hard-code a free provider/model ID.** The free landscape churns weekly (OpenRouter's pool dropped 20→14 in weeks; Gemini Pro moved behind billing). A production FRIDAY must **pull a live catalog / rotate / fall back across providers**, not pin one free endpoint. The existing health-score + circuit-breaker machinery in `providers.rs` already supports this.
- **Free-tier data caveats must surface in the Context Gate UX** (R5-2-4):
  - Gemini free tier / OpenRouter free endpoints may use prompts to improve products.
  - **Gemini gotcha:** enabling billing on a project removes that project's free tier — keep free & paid in separate projects.
  - Lineup/limits vary by region & account; treat published numbers as starting points.
- **Free tiers are quota-bound** (per-minute + per-day): right for FRIDAY's light, per-user OS-assistant load, wrong for a public multi-tenant product — which FRIDAY is not.

---

## 3. ACCOUNT-AUTH — the ⭐ category (2026 is the turning point)

This is the big finding for R5-2-3: **official, sanctioned user-account OAuth integration is now a mainstream, multi-provider reality** — not an exotic Codex-only path.

| Provider | Official seam | FRIDAY's role | Subscription needed? |
|---|---|---|---|
| **GitHub Copilot SDK** | Official OAuth app; user authorizes → user token → SDK | Passes token; **FRIDAY never handles model keys; GitHub manages everything.** Billing = each user's own Copilot subscription. Multi-user isolation per token. | ✅ Copilot subscription |
| **OpenAI Codex** | ChatGPT OAuth browser + **device-code (headless)**; tokens cached & auto-refreshed | User's own ChatGPT subscription powers frontier models, **no API key** | ✅ ChatGPT sub (or API key) |
| **Claude Code OAuth** | User runs `claude`, browser OAuth; creds cached with `user:inference` scope; routes as Claude Code | Reuse the user's Claude auth | ✅ Pro/Max+ depending on path |
| **xAI Grok OAuth** | Browser login, bearer token reused across tooling | No key | ✅ SuperGrok/Premium+ |
| **Qwen OAuth / MiniMax OAuth** | Browser **PKCE** login, refresh token persisted | No key | ✅ (their portal accounts) |
| **Nous Portal** | One OAuth login covers **300+ frontier agentic models** (Claude/GPT/Gemini/DeepSeek/Qwen/Kimi/GLM/MiniMax/Grok + tool gateway) | Unified subscription; scoped `inference:invoke` JWTs, auto-rotation | ✅ Nous subscription |
| **Vertex AI (Gemini, enterprise)** | OAuth2 via service-account JSON / ADC; FRIDAY mints & auto-refreshes short-lived tokens | No static key | GCP-billed |

### Why this matters for FRIDAY
- R5-2-1's "user-subscription (authorized account)" class is **confirmed real and multi-vendor**. FRIDAY can offer a **bring-your-own-account** capability across Copilot, Codex/ChatGPT, Claude, Grok, Qwen, MiniMax, Nous — all through **officially supported seams**, satisfying the policy line (R5-2-1 §4: never a web-UI driver).
- **GitHub Copilot SDK is the cleanest first exemplar for R5-2-3**: an official, documented, embeddable OAuth flow where **FRIDAY never touches/stores provider keys** and billing rides each user's existing Copilot subscription. This de-risks the "who owns a paid key" problem entirely.
- **Headless is a supported concern** in several seams (Codex device-code; Copilot OAuth; Vertex ADC), so FRIDAY's OS-first, headless-ish desktop daemon can authenticate properly rather than scraping a website.
- **FRIDAY owns the token lifecycle** (store/refresh/revoke) and must give the user a clear "FRIDAY will sign in to your account" consent + revoke path — this is the Context Gate's auth half (R5-2-4).
- **Open item for R5-2-3:** each provider's license/ToS for *third-party embedding* of its auth flow should be confirmed before handing over the seam (flagged in R5-2-0 §6). Copilot SDK is explicitly an SDK meant for embedding; Codex is a reference CLI; Claude/Grok/Qwen/MiniMax OAuth adoption needs their docs reviewed.

---

## 4. Synthesis → how this maps to the R5-2-1 abstraction

```
FRIDAY needs a reasoning provider.

  Free Remote (no key)         Account-Auth (no API key)         Paid API (optional)
  ───────────────────          ─────────────────────────        ──────────────────
  Groq (speed)                 GitHub Copilot SDK (cleanest)    user API key (any)
  Gemini Flash (quality)       OpenAI Codex (ChatGPT sub)
  Cerebras / OpenRouter pool   Claude Code OAuth
  SambaNova / GitHub Models    xAI Grok OAuth
                               Qwen / MiniMax OAuth
                               Nous Portal (300+ models)

                └── Reasoning Router (requirement-driven, health-scored)
                              │
                         Context Gate (consent)   → R5-2-4
                              │
                      FRIDAY Desktop UX           → R5-2-5
```

**A user with zero API key** gets a useful FRIDAY via Free Remote (Groq/Gemini fallback chain). **A user with an existing subscription/account** can connect it through an officially supported OAuth route (Copilot/Codex/Claude/Grok/Qwen/MiniMax) — exactly R5-2-1's promise. **A user who wants it** can add explicit paid API keys.

---

## 5. Recommendation to hand R5-2-3 (evidence-based, not binding)
1. **R5-2-3 build target:** start with the **GitHub Copilot SDK seam** as the account-auth exemplar — official, embeddable, "FRIDAY never handles keys", billing rides the user's own Copilot subscription. Add a **free-remote default chain (Groq → Gemini Flash → OpenRouter/Cerebras)** as the zero-key floor in the same pass if it proves low-cost.
2. **Secondary account seams** (Codex/ChatGPT, Claude OAuth, Grok, Qwen/MiniMax, Nous) are matched by capability/hierarchy, adopted after their embedding ToS is confirmed.
3. **Do not build a web-UI/headless-browser driver.** The authorized seam is category 2 (OAuth), not category 3 (automation). The headless-websession idea stays a documented research note only.

---

## 6. Open items for R5-2-3 / later
- Confirm each vendor's embedding/redistribution ToS for its OAuth seam (Copilot SDK: OK — explicit SDK; others: review).
- Free-tier quota volatility + "data improves products" disclosure — must be itemized in the Context Gate consent surface.
- Gemini "enabling billing removes free tier on that project" — affects any provider config UI (separate free/paid projects).
- Dead-provider churn → the router must poll a live catalog and rotate/fall back, never pin a free endpoint.

---

## 7. Acceptance boundary for R5-2-2
- [x] Free-remote no-key floor documented (Groq, Gemini Flash, Cerebras, OpenRouter, SambaNova, GitHub Models…) — §2.
- [x] Account-auth ⭐ category documented as a mainstream, multi-vendor reality (Copilot SDK, Codex, Claude, Grok, Qwen/MiniMax, Nous, Vertex) — §3.
- [x] Headless-websession correctly placed in the research-only category 3 — §1.
- [x] Mapping to the R5-2-1 abstraction + recommendation to R5-2-3 — §4, §5.
- [x] Non-goals: no web-UI driver, no local-model build as centerpiece — §1, §5.
- [ ] Committed as this freeze point.

**No code was written or touched in R5-2-2.** Next: **R5-2-3** builds the account-authenticated provider seam (Copilot SDK first) + the free-remote default chain.
