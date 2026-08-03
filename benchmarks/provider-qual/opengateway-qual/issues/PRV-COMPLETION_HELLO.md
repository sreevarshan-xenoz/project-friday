PRV-COMPLETION_HELLO  

## Title  
Provider qualification failure: completion.hello  

## Observed  
corpus completion.hello (completion)  

## Expected  
completion returns a non-empty response  

## Actual  
action_ok=false, verified=Some(false) — error: All candidates failed. Last error: Provider opengateway returned error status 429 Too Many Requests: {"error":{"message":"Rate limit exceeded: free-models-per-day-high-balance. ","type":"rate_limit_error","code":"429"}}  

## Benchmark  
Provider Qualification / completion / Corpus  

## Status  
open (auto route only — see Resolution note)  

## Resolution note (2026-08-04)  
Root cause is the gateway's `auto` smart-routing route hitting its per-key daily free-model quota (429 `free-models-per-day-high-balance`) — a routing/quota policy constraint, NOT a provider defect. The same check passes 4/4 against free model `mindai/macaron-v1-tall` via the `OPENGATEWAY_MODEL` override — see benchmarks/provider-qual/opengateway-qual-2026-08-04-free (Completion 100%, JSON 100%, Context 100%). This issue tracks the `auto` route only; re-run when the quota resets.  

