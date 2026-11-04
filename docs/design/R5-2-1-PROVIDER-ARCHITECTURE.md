# R5-2-1 — FRIDAY Provider Architecture (FROZEN DECISION)

**Status:** FROZEN design decision. **No code in this phase.**
**Sequence:** R5-2-1 chooses the architecture (this doc) → R5-2-2 local reference provider → R5-2-3 authorized account access → R5-2-4 Context Gate → R5-2-5 OS-level reasoning.
**Input:** the frozen R5-2-0 provider survey (`76631e3`).
**Commit discipline:** R5-2-1 is a freeze point. R5-2-2 begins only after this is committed.

---

## 1. The core abstraction

> **FRIDAY needs a reasoning provider.**

FRIDAY is provider-agnostic. A "provider" is any adapter that, given an approved request + consented context, returns reasoning. The same OS request may be satisfied by different providers on different machines, at different times, for different capabilities.

Providers are **not** the subject of the product. Reasoning is. This mirrors the existing fabric's rule verbatim: *route by requirement, never provider name* (`planner_backend.rs:42`).

### Provider classes
1. **Local** — model on the user's own machine (ollama, llama.cpp/llama-server, LM Studio, MLX). Free, private, offline-capable.
2. **Free tier (official)** — legitimate quota-based cloud (Gemini free tier, OpenRouter, SambaNova, GitHub Models; CN-hosted gated Free-only).
3. **User-subscription (authorized account)** — an *officially supported authorized* seam into a service the user already pays for (the Codex-class OAuth/device-code path for OpenAI; equivalents elsewhere). No extra key required — the user's own account backs it.
4. **Paid API (optional)** — user-supplied official API key(s).
5. **Hybrid** — the orchestration over all of the above, decided by capability requirement + consent.

A user can install FRIDAY with **zero API key** and still have a usable brain (Local → Free tier). Subscription/API classes are voluntary escalations.

---

## 2. Provider hierarchy (resolution order)

Resolve a request by descending the hierarchy to the first class that (a) can meet the capability/sensitivity requirements and (b) is available/authorized on this machine:

```
1. LOCAL            — privacy-first default; meets simple Capability requirements
2. FREE TIER        — quality bump when Local is insufficient and privacy allows
3. SUBSCRIPTION     — frontier quality via the user's own authorized account (opt-in, consented)
4. PAID API         — explicit user-supplied key (opt-in)
```

- **Privacy is the dominant lever** (inherited from `qualification.rs`): `privacy=Required` must stay on Local; `privacy=Preferred` ranks Local above remote; only `privacy=Any` opens remote classes.
- **Fallback behavior:** if the highest available class fails or is unauthorized, fall *down* the hierarchy (never up across the privacy boundary). E.g. Local → Free-tier → Subscription, but Subscription never silently degrades into an unconsented remote call.
- **Never auto-escalate past consent.** Each step that crosses a privacy/consent boundary requires Context Gate authorization (§5); failures must be surfaced to the user, not silently routed.

---

## 3. Routing rules (requirements, not providers)

Routing is driven by `RequestRequirements` + a new **consent signal**, never by a provider name:

- **Capability**: vision / audio / tools / long-context / JSON — encoded today in `ModelCapability`; a provider must carry the needed capabilities.
- **Privacy preference**: `Any / Preferred / Required` (existing `Preference`). Gates which hierarchy classes are eligible.
- **Budget**: free-first policy (existing) — bounded budget opens paid classes deliberately.
- **Latency**: sort/score; low-latency local-first voices (M8 roadmap) prefer llama.cpp.
- **Context sensitivity (NEW)**: whether the request carries OS context (screen/selection/current-app/files/mic/clipboard). If sensitive → Local is preferred, and any remote resolution is *consent-gated* (§5).
- **Authorization (NEW)**: whether the user has consented to the class (e.g. subscribed account) and whether the provider's seam is currently authenticated.

**Mode selection maps cleanly onto the existing `RoutingMode` enum** (`Fast / Quality / Free / Local / Hybrid`), with `Hybrid` already meaning "local OR free remote" — the new subscription class is added as an explicitly-consented escalation, not folded into an automatic Hybrid default.

---

## 4. Authentication boundaries

| Class | Seam | Credential ownership | Boundary |
|---|---|---|---|
| Local | localhost endpoint (ollama/llama.cpp) | none (local only) | Never leaves the machine |
| Free tier | provider API key / anonymous endpoint | FRIDAY-managed or built-in | Remote, quota-bound |
| Subscription | **OAuth / device-code** (user-authorized) | **user's own account** | FRIDAY acts *as the user* only with explicit auth + consent |
| Paid API | user API key | user's own key | Straightforward |

- **The subscription boundary is the policy-critical one.** FRIDAY only ever uses *officially supported, sanctioned* seams (OAuth/device-code, API key, enterprise access token where applicable). FRIDAY **never** drives a web UI, scrapes a private endpoint, or reverse-engineers a service to bypass intended access. Trap, per R5-2-0 §2 — never built.
- **OAuth refresh/credential handling is FRIDAY's responsibility and risk**, and must be framed to the user as "FRIDAY will sign in to your account" with a clear consent + revoke path (Context Gate, §5).

---

## 5. Context Gate — exactly where it sits

The Context Gate is the **single consent boundary between OS context and any provider**. It sits *between* FRIDAY and all remote providers (Free tier, Subscription, Paid API); Local does not cross it because it never leaves the machine.

```
             FRIDAY
                │
        ┌───────▼────────┐
        │   Context Gate  │  ← consent + minimal-surface policy
        └───────┬────────┘
                │ approved context only
        ┌───────▼────────┐
        │     Provider    │  (Local may bypass; remote never bypasses)
        └───────┬────────┘
                │ reasoning / response
        ┌───────▼────────┐
        │    FRIDAY OS UX  │
        └────────────────┘
```

- **What it gates:** screen / selected text / current app / files / mic / clipboard — each is an independent, explicit, per-use consent check.
- **What it does NOT gate:** the OS is never blanket-exfiltrated. FRIDAY decides "what reasoning capability do I need" (Local-first), and only when it must go remote does it request the *minimum* surface, itemized, with a revoke path and a visible disclosure of free-tier "data improves products" behavior.
- **Local bypass is a feature, not a loophole:** privacy=Required stays local and needs no remote consent. The Gate is the boundary for *remote* escalation only.
- **Policy invariant:** provider selection never drives what FRIDAY sees; FRIDAY (via requirement + consent) drives what the provider sees.

---

## 6. Decision for R5-2-2 onward (frozen)

1. **R5-2-2 — Free remote provider + account-auth survey (build nothing).** FRIDAY's goal is *capable remote reasoning the user can access without buying an API key*. Survey what is actually available in 2026, what authentication each officially supports, what "free" really means, and what FRIDAY can legally/technically integrate. Do **not** assume Google / ChatGPT / Gemini yet. Classify every candidate by the three-way method table: Official API+key / Official OAuth-account / Website-automation (the last is research-only, potentially prohibited & fragile — see R5-2-0 §2 trap). The ChatGPT headless-websession idea is a **specifically marked research branch**, never an unexamined default.
2. **R5-2-3 — Account-authenticated providers.** Build the subscription/account class via an *officially supported authorized seam* (e.g. OAuth/device-code) chosen from the R5-2-2 evidence — the sanctioned answer to "who owns a paid key here." Never a web-UI driver. Preceded by a contract/expertise sign-off where embedding a third party's auth flow (open risk from R5-2-0 §6) requires it.
3. **R5-2-4 — Context Gate.** Build the real OS security boundary between context and remote providers (per-use, itemized consent + revoke + disclosure).
4. **R5-2-5 — OS-level reasoning.** "Friday, what's wrong with this?" anywhere in the OS: FRIDAY derives relevant context (current app + selection + …), consents via the Gate if remote, sends only approved context to the best available authorized class, and presents inside FRIDAY UX.

**Non-goals (explicitly not in R5-2):** web-UI automation/scraping (trap — banned), blanket OS exfiltration, forcing a paid key, "ChatGPT inside FRIDAY" as the center of the design. The local seam (already wired: ollama + `llamacpp`/`gemma4:e4b`) stays available as the privacy=Required fallback, but is **not** the R5-2 centerpiece.

### R5-2-WEB (separate, research-only branch) — User-authorized browser reasoning

Not part of the R5-2 build sequence. It is a **dedicated research branch** for the headless-web "ask my ChatGPT / ask my Gemini" idea. Distinct from R5-2-3 (official OAuth seams), which is the sanctioned build path.

Architecture proposed for research (NOT built):
```
FRIDAY account/session
   → user explicitly authorizes a browser session
   → isolated browser profile
   → CDP controller (navigate, inspect DOM, enter text, read response)
   → provider web application
   → FRIDAY reasoning interface

Context Gate sits BEFORE the browser controller, not after:
   Screen/selection/mic/files → Context Gate (user approves) → Browser Provider → remote reasoning
```
The Context Gate preventing unrestricted remote desktop access is what makes "Friday, look at my screen and ask my ChatGPT" architecturally safe.

**Three independent questions to answer by fresh research (not guessing):**
1. *Technical feasibility* — browser automation/CDP can control a session. (Yes, in the abstract.)
2. *Authentication* — can the user legitimately attach an existing account/session to an isolated profile?
3. *Provider permission* — does the service **permit** automated use of its web interface this way?

**Hard boundary:** if provider rules offer no legitimate account/session integration path, we must **not** build a brittle workaround and call it a product feature. Technical cleverness does not override provider authorization. Vendors are re-evaluated on current rules, never on stale assumptions (openai.com/chatgpt.com policies, ai.google.dev, anthropic rules — fresh as of research date).

---

## 7. Acceptance boundary for R5-2-1

- [x] Core abstraction frozen ("FRIDAY needs a reasoning provider", provider-agnostic).
- [x] Provider hierarchy + resolution/fallback order frozen (§2).
- [x] Routing rules, mode mapping to existing `RoutingMode`, and the new context-sensitivity + authorization signals frozen (§3).
- [x] Authentication boundaries incl. the sanctioned-seam rule frozen (§4).
- [x] Context Gate position + policy frozen (§5).
- [x] R5-2-2…R5-2-5 build order and non-goals frozen (§6; R5-2-2 corrected 2026-08-30 to a remote-first survey, no local-model build).
- [x] **R5-2-WEB separate research-only branch** scoped (§6; isolated browser profile + CDP, Context Gate before controller, three-question evidence gate, no build without provider permission).
- [x] Committed as this freeze point.

**No code was written or touched in R5-2-1.** Next: **R5-2-2** surveys free remote reasoning and account-authenticated routes (build nothing).
