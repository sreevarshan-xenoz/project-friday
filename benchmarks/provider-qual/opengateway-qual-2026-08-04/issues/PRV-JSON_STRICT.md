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
open  

