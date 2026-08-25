PRV-LONG_CONTEXT_NO_TRUNCATION  

## Title  
Provider qualification failure: long_context.no_truncation  

## Observed  
corpus long_context.no_truncation (long_context)  

## Expected  
long-context response is complete (no truncation)  

## Actual  
action_ok=false, verified=Some(false) — error: All candidates failed. Last error: Provider gemini returned error status 404 Not Found: [{
  "error": {
    "code": 404,
    "message": "This model models/gemini-2.5-flash is no longer available to new users. Please update your code to use a newer model for the latest features and improvements. We recommend you to use the Interactions API (https://ai.google.dev/gemini-api/docs/migrate-to-interactions).",
    "status": "NOT_FOUND"
  }
}
]  

## Benchmark  
Provider Qualification / long_context / Corpus  

## Status  
open  

