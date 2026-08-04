PRV-LONG_CONTEXT_NO_TRUNCATION  

## Title  
Provider qualification failure: long_context.no_truncation  

## Observed  
corpus long_context.no_truncation (long_context)  

## Expected  
long-context response is complete (no truncation)  

## Actual  
action_ok=false, verified=Some(false) — error: All candidates failed. Last error: Provider opengateway returned error status 429 Too Many Requests: {"error":{"message":"Rate limit exceeded: free-models-per-day-high-balance. ","type":"rate_limit_error","code":"429"}}  

## Benchmark  
Provider Qualification / long_context / Corpus  

## Status  
open (auto route only — see Resolution note)  

## Resolution note (2026-08-04)  
Root cause is the gateway's `auto` smart-routing route hitting its per-key daily free-model quota (429 `free-models-per-day-high-balance`) — a routing/quota policy constraint, NOT a provider defect. The same check passes 4/4 against free model `mindai/macaron-v1-tall` via the `OPENGATEWAY_MODEL` override — see benchmarks/provider-qual/opengateway-qual-2026-08-04-free (Completion 100%, JSON 100%, Context 100%). This issue tracks the `auto` route only; re-run when the quota resets.

## Additional observation (2026-08-04, transient — not reproducible)  
A `reasoning-free` tier run (`mindai/macaron-v1-tall`) once failed long_context with `os error 10054` (connection forcibly closed by the remote host) after ~50 s — a network/gateway drop, NOT a quota error and NOT reproducible: the immediate re-run passed 4/4 (see opengateway-qual-2026-08-04-tier-reasoning-free-2). The catalog run (auto route → free-model failover, served by `inclusionai/ling-3.0-flash:free`) also passed 4/4. The count-4 graduation is this single transient network observation added to the 3 auto-route quota observations; evidence does not indicate a long-context defect in the free tier.  

