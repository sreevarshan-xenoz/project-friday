# Intelligence Specification — Cognition Layer (M5)

**Status:** ✅ Implemented cognitive foundations (Router, Prompt Compiler, Response Interpreter).

The Cognition Layer governs FRIDAY's reasoning and sits **above the kernel**, not inside it. It translates the compiled `AgentContext` from the Context Builder into a structured, validated `TaskGraph` planner blueprint.

```
Goal.raw ──> Context Builder ──> AgentContext
                                      │
                                      ▼
                        [Intelligence Router]
                        ├───────────────────┐
                  (System 1)            (System 2)
                        │                   │
                  Direct Exec          Prompt Compiler
                                            │
                                            ▼
                                     Model Providers
                                            │
                                            ▼
                                   [Response Interpreter]
                                            │
                                            ▼
                                       TaskGraph
```

## Subsystem Architecture

The cognition layer is implemented in the `friday-intelligence` crate (`services/intelligence/`) and divided into four decoupled subsystems:

### 1. Intelligence Router (`router.rs`)
Directs user goals according to cognitive complexity:
- **System 1 (Reactive)**: Greetings, simple queries, or direct single-capability execution commands (e.g. "open notepad", "ping"). These route immediately without planning, simulation, or verification.
- **System 2 (Reflective)**: Complex, plannable, multi-step requests. These invoke the context-gathering pipeline, prompt compilation, simulation engines, and verification loops.

### 2. Prompt Compiler (`compiler.rs`)
Translates `AgentContext` into an intermediate structured **Prompt AST** containing:
- System instructions
- Registered capability schemas
- Relevant historical memories
- Relevant procedural execution experiences
- System/project state (cwd, sessions, healthy services)

The AST is compiled to target-tailored string formats:
- **Claude (XML)**: Uses precise XML boundary tags (`<capabilities>`, `<user_goal>`).
- **Ollama (Compact)**: Strips verbose telemetry to fit in local, smaller context windows.
- **Gemini / OpenAI (Markdown)**: Renders standard Markdown heading blocks.

### 3. Response Interpreter (`parser.rs`)
Strictly validates and parses raw model outputs to protect the system's ontology:
- Strips markdown formatting code-fences.
- Parses output into a typed `LlmResponse` intermediate struct.
- **Ontology Verification**: Compares proposed capability IDs against known registered IDs in the capability registry. Typos (e.g., `windows.launch`) trigger a fuzzy suggestion lookup (e.g., *"Did you mean 'windows.open'?"*) and raise a parser error, preventing the LLM from inventing new terms.
- Compiles the valid response into a planner `TaskGraph` DAG.

### 4. Isolated Model Clients (`providers.rs`)
Interfaces with LLM execution targets via the `ModelClient` async trait, ensuring that LLM backends can be swapped seamlessly (e.g., swapping Ollama for Gemini or Claude) without affecting the router, compiler, or interpreter code.

## Verification & Testing
All cognitive foundations are verified by unit and integration tests located in `services/intelligence/tests/intelligence_tests.rs`.
