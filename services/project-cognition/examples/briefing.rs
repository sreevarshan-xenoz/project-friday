//! FRIDAY's "good morning" moment — the persistent project cognition
//! briefing.
//!
//! Run with:
//!
//! ```text
//! cargo run -p friday-project-cognition --example briefing
//! ```

use friday_project_cognition::ProjectCognition;

fn main() {
    // FRIDAY keeps her own project narrative — this is what she'd say before
    // the user says anything.
    let mut pc = ProjectCognition::new(
        "friday",
        "FRIDAY",
        "Build FRIDAY as an adaptive cognitive operating system",
    );

    pc.advance_milestone("Skill Engine", "M5 — skills are pure data");
    pc.advance_milestone("Agent Runtime", "M8 — planner spawns scoped workers");
    pc.set_progress(82.0);
    pc.set_next_milestone("Web Agent");
    pc.add_blocker("Desktop Agent — three unresolved issues");
    pc.open_decision("Provider routing policy");
    pc.open_decision("Persona blending thresholds");

    println!("FRIDAY:");
    println!("  {}", pc.status_briefing());

    // Close one decision and resolve a blocker; the reflection digest picks
    // up exactly what changed.
    pc.close_decision("Provider routing policy", "capability-first, cost second");
    pc.resolve_blocker("Desktop Agent — three unresolved issues");

    println!("\nReflection digest (facts since last digest):");
    for fact in pc.reflection_digest() {
        println!("  • {fact}");
    }
}
