PRV-JSON_STRICT  

## Title  
Provider qualification failure: json.strict  

## Observed  
corpus json.strict (json)  

## Expected  
response is valid, schema-shaped JSON  

## Actual  
action_ok=false, verified=Some(false) — error: All candidates failed. Last error: Provider opengateway returned error status 401 Unauthorized: {"error":{"message":"API key required","type":"authentication_error","code":"api_key_required"}}  

## Benchmark  
Provider Qualification / json / Corpus  

## Status  
candidate (seen 1 time(s)) — pending human review  

