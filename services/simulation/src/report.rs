//! Human-readable rendering of a [`SimulationReport`] (the UI backing for M7).

use crate::simulator::SimulationReport;

/// Render a full simulation report in the layout agreed for M4.7.
pub fn render_report(r: &SimulationReport) -> String {
    let mut out = String::new();
    out.push_str("Simulation Report\n");
    out.push_str("──────────────\n");

    // Plan Summary
    out.push_str(&format!(
        "Plan Summary\n{} actions\n{} windows\n{} browser\n{} files\nEstimated: {} seconds\nRisk: {}\n",
        r.summary.actions,
        r.summary.windows,
        r.summary.browsers,
        r.summary.files_affected,
        r.summary.estimated_seconds,
        r.risk.as_str(),
    ));

    // Risk Analysis
    let failure = (1.0 - r.confidence.confidence).clamp(0.0, 1.0);
    out.push_str(&format!(
        "Risk Analysis\nlevel: {}\nfailure probability: {:.1}%\n",
        r.risk.as_str(),
        failure * 100.0
    ));

    // Resource Forecast
    let f = &r.resource_forecast;
    out.push_str(&format!(
        "Resource Forecast\nCPU: {:.1}%\nMemory: {} MB\nNetwork: {} B\nDisk: {} B\nGPU: {:.1}%\nTokens: {}\n",
        f.cpu_pct, f.memory_mb, f.network_bytes, f.disk_bytes, f.gpu_pct, f.tokens
    ));

    // Predicted Timeline
    out.push_str(&format!(
        "Predicted Timeline\n{} ms (~{:.1} s)\n",
        r.timeline_ms,
        r.timeline_ms as f64 / 1000.0
    ));

    // Side Effects
    out.push_str("Side Effects\n");
    if r.side_effects.is_empty() {
        out.push_str("  (none)\n");
    } else {
        for e in &r.side_effects {
            render_effect(&mut out, e, 1);
        }
    }

    // Approval Needed
    out.push_str(&format!(
        "Approval Needed\n{}\n",
        if r.approval_required { "YES" } else { "NO" }
    ));

    // Rollback Availability
    out.push_str(&format!(
        "Rollback Availability\n{}\n",
        if r.rollback_available {
            "available"
        } else {
            "not available"
        }
    ));

    // Confidence Score
    out.push_str(&format!(
        "Confidence Score\nconfidence: {:.1}%\nuncertainty: {:.1}%\n",
        r.confidence.confidence * 100.0,
        r.confidence.uncertainty * 100.0
    ));
    if !r.confidence.assumptions.is_empty() {
        out.push_str("assumptions:\n");
        for a in &r.confidence.assumptions {
            out.push_str(&format!("  - {a}\n"));
        }
    }

    // Fallbacks
    if !r.fallbacks.is_empty() {
        out.push_str("Fallback Paths\n");
        for fb in &r.fallbacks {
            let status = if fb.available { "available" } else { "UNAVAILABLE" };
            out.push_str(&format!("  {} -> {} [{status}]\n", fb.from, fb.to));
        }
    }

    // Issues
    if !r.issues.is_empty() {
        out.push_str("Issues\n");
        for i in &r.issues {
            out.push_str(&format!("  [{}] {}: {}\n", i.severity.as_str(), i.kind, i.message));
        }
    }

    out
}

fn render_effect(out: &mut String, e: &friday_capabilities::sim::SideEffect, depth: usize) {
    let indent = "  ".repeat(depth);
    out.push_str(&format!(
        "{indent}+ {} ({})\n",
        e.description,
        if e.magnitude == 0.0 {
            String::new()
        } else {
            format!("{}", e.magnitude)
        }
    ));
    for c in &e.children {
        render_effect(out, c, depth + 1);
    }
}
