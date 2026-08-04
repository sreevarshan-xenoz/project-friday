PRV-JSON_STRICT  

## Title  
Provider qualification failure: json.strict  

## Observed  
corpus json.strict (json)  

## Expected  
response is valid, schema-shaped JSON  

## Actual  
action_ok=false, verified=Some(false) — error: All candidates failed. Last error: Provider opengateway returned error status 429 Too Many Requests: {"error":{"message":"Rate limit exceeded: free-models-per-day-high-balance. ","type":"rate_limit_error","code":"429"}}  

## Benchmark  
Provider Qualification / json / Corpus  

## Status  
closed (2026-08-04) — auto route re-qualified green; see Closure note.  

## Resolution note (2026-08-04)  
Root cause is the gateway's `auto` smart-routing route hitting its per-key daily free-model quota (429 `free-models-per-day-high-balance`) — a routing/quota policy constraint, NOT a provider defect. The same check passes 4/4 against free model `mindai/macaron-v1-tall` via the `OPENGATEWAY_MODEL` override — see benchmarks/provider-qual/opengateway-qual-2026-08-04-free (Completion 100%, JSON 100%, Context 100%). This issue tracks the `auto` route only; re-run when the quota resets.

## Closure note (2026-08-04, ~05:23 UTC)  
`auto` route re-qualified 4/4 GREEN after the per-key daily quota window reset — benchmarks/provider-qual/2026-08-04_052257 (Completion 100%, JSON 100%, Context 100%; served `opengateway/inclusionai/ling-3.0-flash:free`). Confirms the 429s were quota-window exhaustion, not an `auto`-routing or provider defect.

## Update (2026-08-04, ~06:24 UTC) — 429 recurs, absorbed by design  
The `auto` route 429'd again within the same quota window, but the fabric failover absorbed it: benchmarks/provider-qual/2026-08-04_062415 `routing_decision` records chosen `opengateway/auto` → 429 `free-models-per-day-high-balance` → served `opengateway/inclusionai/ling-3.0-flash:free`, and the run still passed 4/4. The per-key free-model quota recurs; routing handles it. Not a provider defect — issue stays closed. 

