use serde::{Deserialize, Serialize};
use friday_context::AgentContext;

/// Model providers that FRIDAY can route prompts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelProvider {
    Ollama,
    Gemini,
    OpenAi,
    Claude,
}

/// Abstract Syntax Tree (AST) representation of a prompt.
/// Rather than simple string concatenation, the compiler works on this AST
/// to structure the context cleanly.
#[derive(Debug, Clone)]
pub struct PromptAst {
    pub system_instruction: String,
    pub capabilities: Vec<String>,
    pub memories: Vec<String>,
    pub experiences: Vec<String>,
    pub active_state: String,
    pub goal: String,
}

pub struct PromptCompiler;

impl PromptCompiler {
    /// Compile the context into a prompt AST.
    pub fn compile_ast(context: &AgentContext) -> PromptAst {
        let system_instruction = "You are FRIDAY, a desktop cognitive operating system. \
            Your goal is to decompose the user's intent into a structured task graph of capabilities. \
            Only select tools from the available capabilities listed below. Do not invent new capabilities."
            .to_string();

        let capabilities = context.capabilities.iter().map(|c| {
            format!(
                "- ID: {}\n  Description: {}\n  Reliability: {:.2}\n  Latency: {}ms\n  Actions: {:?}",
                c.id, c.name, c.reliability, c.latency_ms, c.actions
            )
        }).collect();

        let memories = context.relevant_memories.iter().map(|m| {
            format!("- [{:?}] {}: {}", m.layer, m.key, serde_json::to_string(&m.content).unwrap_or_default())
        }).collect();

        let experiences = context.experiences.iter().map(|e| {
            format!(
                "- Intent: '{}' (Success: {}, Duration: {}ms, Timing Drift: {:.2})\n  Failed Tasks: {:?}",
                e.intent,
                e.success,
                e.actual_duration_ms,
                e.timing_drift,
                e.tasks.iter().filter(|t| !t.success).map(|t| &t.name).collect::<Vec<_>>()
            )
        }).collect();

        let active_state = format!(
            "Current Project CWD: {}\nHealthy Services: {:?}\nActive sessions: {:?}",
            context.current_project,
            context.healthy_services,
            context.active_sessions
        );

        PromptAst {
            system_instruction,
            capabilities,
            memories,
            experiences,
            active_state,
            goal: context.goal.clone(),
        }
    }

    /// Compile the prompt AST to a string format tailored for the specific model provider.
    pub fn compile_to_string(ast: &PromptAst, provider: ModelProvider) -> String {
        match provider {
            ModelProvider::Claude => {
                // Claude prefers XML tags for precise context boundary separation
                let mut p = String::new();
                p.push_str(&format!("<system>\n{}\n</system>\n\n", ast.system_instruction));
                
                p.push_str("<capabilities>\n");
                for cap in &ast.capabilities {
                    p.push_str(cap);
                    p.push_str("\n");
                }
                p.push_str("</capabilities>\n\n");

                if !ast.memories.is_empty() {
                    p.push_str("<relevant_memories>\n");
                    for mem in &ast.memories {
                        p.push_str(mem);
                        p.push_str("\n");
                    }
                    p.push_str("</relevant_memories>\n\n");
                }

                if !ast.experiences.is_empty() {
                    p.push_str("<procedural_memory_experiences>\n");
                    for exp in &ast.experiences {
                        p.push_str(exp);
                        p.push_str("\n");
                    }
                    p.push_str("</procedural_memory_experiences>\n\n");
                }

                p.push_str(&format!("<active_state>\n{}\n</active_state>\n\n", ast.active_state));
                p.push_str(&format!("<user_goal>\n{}\n</user_goal>", ast.goal));
                p
            }
            ModelProvider::Ollama => {
                // Keep it highly compact to respect smaller local context windows
                let mut p = String::new();
                p.push_str(&format!("INSTRUCTION: {}\n\n", ast.system_instruction));
                p.push_str("AVAILABLE TOOLS:\n");
                for cap in &ast.capabilities {
                    p.push_str(cap);
                    p.push_str("\n");
                }
                p.push_str("\nCONTEXT:\n");
                p.push_str(&ast.active_state);
                p.push_str("\n\nGOAL: ");
                p.push_str(&ast.goal);
                p
            }
            ModelProvider::Gemini | ModelProvider::OpenAi => {
                // Standard markdown header format
                let mut p = String::new();
                p.push_str(&format!("### System Instruction\n{}\n\n", ast.system_instruction));
                p.push_str("### Available Capabilities\n");
                for cap in &ast.capabilities {
                    p.push_str(cap);
                    p.push_str("\n");
                }
                p.push_str("\n### Relevant memories\n");
                for mem in &ast.memories {
                    p.push_str(mem);
                    p.push_str("\n");
                }
                p.push_str("\n### Past Experiences\n");
                for exp in &ast.experiences {
                    p.push_str(exp);
                    p.push_str("\n");
                }
                p.push_str(&format!("\n### Active System State\n{}\n\n", ast.active_state));
                p.push_str(&format!("### Target Goal\n{}", ast.goal));
                p
            }
        }
    }
}
