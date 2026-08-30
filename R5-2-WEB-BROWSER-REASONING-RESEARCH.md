# R5-2-WEB — User-Authorized Browser Reasoning (RESEARCH BRANCH, FROZEN)

**Status:** FROZEN research branch — the headless-web / CDP / "ask my ChatGPT" idea. **No code produced.** This is deliberately **separate from the R5-2 build sequence** and does not gate R5-2-3 (which builds official OAuth seams).
**Date of evidence:** 2026-08-30 fresh web research (not guesses).
**Commit discipline:** R5-2-WEB is a freeze point; any future reconsideration must cite current provider rules, which are known to change.

---

## 1. The idea (architecturally, and what would make it safe)

```
FRIDAY account/session
   → user explicitly authorizes a browser session
   → isolated browser profile
   → CDP controller (navigate, inspect DOM, enter text, read rendered response)
   → provider web application
   → FRIDAY reasoning interface

Context Gate sits BEFORE the browser controller, not after:
   Screen/selection/mic/files → Context Gate (per-use user approval) → Browser Provider → remote reasoning
```

The Context Gate before the controller is what would make "Friday, look at my screen and ask my ChatGPT" safe — the remote service would only ever receive the specific context the user approved, never unrestricted desktop access.

---

## 2. The three questions — answered with fresh 2026 evidence

### Q1. Technical feasibility — YES (abstractly)
CDP / remote-debugging / Playwright can navigate, inspect DOM, enter text, and read responses. Both OpenAI and Google even document **"Computer Use"** browser-control APIs that work through Playwright/CDP. The mechanism is real.

### Q2. Authentication — PARTIAL / SAME-SHAPE
The user *can* attach an existing session, but see Q3: the same-shape patterns are exactly what got banned.

### Q3. Provider permission — **NO for the subscription-proxy pattern; YES only via official APIs**

**OpenAI — ChatGPT consumer web is NOT permitted as an automation surface.**
From openai.com/policies/row-terms-of-use (2026): under "What you cannot do":
- "Automatically or programmatically extract data or Output."
- "Interfere with or disrupt our Services, including circumvent any rate limits or restrictions or bypass any protective measures or safety mitigations we put on our Services."

Driving the ChatGPT website with a CDP/headless controller = bypassing protective measures to programmatically extract Output → **prohibited** for consumer ChatGPT.

**Google — the exact subscription-proxy pattern was BANNED and is actively enforced.**
From the 2026-05-11 Puter/Gemini research and Google's own docs:
- "The Gemini API does not have an `/oauth/authorize` flow that mints a token billed to the user's Google AI subscription."
- The Gemini CLI OAuth token (same shape as Claude Code's `sk-ant-oat01-` and OpenAI's Codex token) is **first-party only**. Third parties used to proxy it to back apps with a user's Google AI Pro/Ultra subscription — **Google banned this in February 2026 and mass-suspended accounts, including PAID Ultra subscribers. Detection enforced from March 25, 2026.**
- "Sign in with Google" is identity-only; it does not grant model usage on the user's plan.

**What IS permitted (the sanctioned, official ways):**
- **Google "Computer Use"** (Gemini 3.6 Flash, Agent Platform sandboxes): an official product capability to *build browser-control agents that automate tasks in a sandboxed, isolated browser via Playwright/CDP* — with built-in safety categories (require user confirmation for logins/financial/sensitive/ToS), prompt-injection detection, and human-in-the-loop. This is the **sanctioned browser-automation path** — but it targets arbitrary third-party sites and apps **through the Gemini API**, not the consumer Gemini web app, and not the user's AI subscription.
- **OpenAI Computer Use / Responses API**: browser automation as an API feature with an isolated harness + safety guidance.
- The **R5-2-3 official OAuth seams** (GitHub Copilot SDK, Codex device-code, etc.) documented in R5-2-2 — legitimate account auth, no web scraping.

---

## 3. Verdict (frozen)

| Path | Provider position | FRIDAY should |
|---|---|---|
| Drive ChatGPT consumer web via CDP | **Prohibited** (ToS: programmatic extraction, bypassing protections) | Do **not** build |
| Proxy user's Gemini/AI-sub OAuth token to back FRIDAY (the ChatGPT-web / codex-proxy shape) | **Actively banned** — Google Feb-2026 enforcement, mass account suspension incl. paid Ultra | Do **not** build; risky & enforced |
| "Sign in with Google" as the route to model usage | Identity only, grants **no** model usage on user's plan | Not a provider seam |
| Google "Computer Use" + OpenAI Computer Use via **their official APIs** | **Sanctioned**, official, safety-layered, sandboxed | **This** is the legitimate home of FRIDAY "look at my screen / drive this app" automation — via API, not scraping |
| Official account OAuth (Copilot SDK, Codex device-code, etc., R5-2-2) | **Sanctioned** for the plain "use your subscription for reasoning" need | **This** is the build target — R5-2-3 |

**Bottom line:** the original "attach my paid web subscription and back FRIDAY's reasoning with it" shape is **not a viable product foundation** — it is **prohibited and actively enforced against by both OpenAI and Google**. Technical cleverness does not override provider authorization. We do **not** build a brittle, ToS-violating workaround and call it a feature.

**What survives and is worthwhile:** the *experience* "Friday, look at my screen / my current app / my selection and reason over it" is achievable **legitimately** through the providers' **official Computer Use APIs** (sandboxed, safety-layered, human-in-the-loop) plus the **Context Gate** — with the **reasoning provider built in R5-2-3 (official seams) actually doing the thinking**. The browser is the *tool* the model drives (via sanctioned APIs), not the *interface we scrape*.

---

## 4. What this means for the R5-2 plan (update)

- **R5-2-WEB is DEFFERED / DOWNGRADED to a non-build research note.** No CDP controller against provider consumer sites.
- **The legitimate "look at my screen" assistant** folds into the R5-2-5 OS-level reasoning goal, powered by:
  - **R5-2-3** official reasoning providers (Copilot SDK / Codex / free-remote chain), **and**
  - **R5-2-4** the Context Gate (screen/selection/app/mic/files consent), **and**
  - optionally the **official Computer Use APIs** (OpenAI/Gemini sandboxed browser control) where a task genuinely needs the model to drive an app in a browser.
- **Do not** route a user's ChatGPT/AI-subscription sessions through FRIDAY's own CDP harness.

---

## 5. Acceptance boundary for R5-2-WEB
- [x] Q1 feasibility documented (yes, but moot). — §2
- [x] Q2 authentication documented (same-shape = banned). — §2
- [x] Q3 provider permission established with **fresh 2026 evidence** (OpenAI ToS prohibition; Google Feb-2026 ban + enforcement; official Computer Use APIs as the sanctioned alternative). — §2, §3
- [x] Verdict frozen: do NOT build the scraping / subscription-proxy shape; fold the legitimate "look at my screen" experience into R5-2-3/4/5 + official Computer Use APIs. — §3, §4
- [x] Committed as this freeze point.

**No code was written or touched in R5-2-WEB.** The R5-2 build path proceeds to **R5-2-3** (official OAuth + free-remote provider seams).
