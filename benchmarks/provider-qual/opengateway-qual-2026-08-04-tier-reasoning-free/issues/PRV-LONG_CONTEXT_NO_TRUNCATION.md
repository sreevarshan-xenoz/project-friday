PRV-LONG_CONTEXT_NO_TRUNCATION  

## Title  
Provider qualification failure: long_context.no_truncation  

## Observed  
corpus long_context.no_truncation (long_context)  

## Expected  
long-context response is complete (no truncation)  

## Actual  
action_ok=false, verified=Some(false) — error: All candidates failed. Last error: HTTP request failed: error sending request for url (https://opengateway.gitlawb.com/v1/chat/completions): connection error: An existing connection was forcibly closed by the remote host. (os error 10054)  

## Benchmark  
Provider Qualification / long_context / Corpus  

## Status  
open  

