use serde_json::json;

use super::task_graph::TaskGraph;
use crate::intent::classifier::IntentType;
use crate::intent::Intent;

/// Convert a resolved [`Intent`] into an executable [`TaskGraph`]. This engine
/// decides **what** must happen; it never executes anything.
pub fn plan(intent: &Intent) -> TaskGraph {
    let kind = IntentType::classify(&intent.raw);
    let mut graph = TaskGraph::new(intent.intent.clone());
    match kind {
        IntentType::BrowserSearch => plan_browser_search(&mut graph, intent),
        IntentType::RoutineMorning => plan_morning(&mut graph),
        IntentType::OpenApp => plan_open_app(&mut graph, intent),
        IntentType::Unknown => plan_generic(&mut graph, intent),
    }
    graph
}

fn plan_browser_search(graph: &mut TaskGraph, intent: &Intent) {
    let browser = intent
        .entity_str("browser")
        .unwrap_or("default")
        .to_string();
    let query = intent.entity_str("query").unwrap_or("").to_string();

    let launch = graph.add_task(
        "Launch Browser",
        "browser.launch",
        json!({ "browser": browser }),
        &[],
    );
    let ready = graph.add_task(
        "Wait Until Browser Ready",
        "browser.wait_ready",
        json!({}),
        &[launch],
    );
    let navigate = graph.add_task(
        "Navigate Search",
        "browser.navigate",
        json!({ "query": query }),
        &[ready],
    );
    graph.add_task("Verify Page Loaded", "page.verify", json!({}), &[navigate]);
}

fn plan_open_app(graph: &mut TaskGraph, intent: &Intent) {
    let app = intent.entity_str("app").unwrap_or("app").to_string();
    graph.add_task("Open Application", "app.open", json!({ "app": app }), &[]);
}

fn plan_morning(graph: &mut TaskGraph) {
    let browser = graph.add_task("Open Browser", "app.open", json!({ "app": "browser" }), &[]);
    let spotify = graph.add_task("Open Spotify", "app.open", json!({ "app": "spotify" }), &[]);
    let calendar = graph.add_task("Read Calendar", "calendar.read", json!({}), &[]);
    let merge = graph.add_task(
        "Merge Results",
        "routine.merge",
        json!({}),
        &[browser, spotify, calendar],
    );
    graph.add_task("Reply With Summary", "routine.reply", json!({}), &[merge]);
}

fn plan_generic(graph: &mut TaskGraph, intent: &Intent) {
    graph.add_task(
        "Handle Request",
        intent.intent.clone(),
        json!({ "raw": intent.raw }),
        &[],
    );
}
