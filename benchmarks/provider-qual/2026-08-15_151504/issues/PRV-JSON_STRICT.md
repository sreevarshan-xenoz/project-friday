PRV-JSON_STRICT  

## Title  
Provider qualification failure: json.strict  

## Observed  
corpus json.strict (json)  

## Expected  
response is valid, schema-shaped JSON  

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
Provider Qualification / json / Corpus  

## Status  
open  

