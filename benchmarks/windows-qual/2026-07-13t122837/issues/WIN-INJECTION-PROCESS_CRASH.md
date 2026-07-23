WIN-INJECTION-PROCESS_CRASH  

## Title  
Windows qualification failure: injection.process_crash  

## Observed  
corpus injection.process_crash (failure-injection)  

## Expected  
action fails gracefully (error returned)  

## Actual  
action_ok=true, state_accuracy=Some(false) — graceful_failure=false (action_ok=true)  

## Benchmark  
Windows Qualification / failure-injection / Failure Injection  

## Status  
open  

