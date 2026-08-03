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
open  

