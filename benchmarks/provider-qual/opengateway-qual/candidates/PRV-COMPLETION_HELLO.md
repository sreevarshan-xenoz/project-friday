PRV-COMPLETION_HELLO  

## Title  
Provider qualification failure: completion.hello  

## Observed  
corpus completion.hello (completion)  

## Expected  
completion returns a non-empty response  

## Actual  
action_ok=false, verified=Some(false) — error: All candidates failed. Last error: Provider opengateway returned error status 401 Unauthorized: {"error":{"message":"API key required","type":"authentication_error","code":"api_key_required"}}  

## Benchmark  
Provider Qualification / completion / Corpus  

## Status  
candidate (seen 1 time(s)) — pending human review  

