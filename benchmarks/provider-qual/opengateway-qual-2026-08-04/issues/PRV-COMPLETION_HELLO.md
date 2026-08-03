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
open  

