//! Linux AT-SPI capability adapter: native inspection and operation of
//! accessible application controls over the user's session accessibility D-Bus.
//!
//! Typed verbs only:
//! - `apps`: list accessible applications (name, id)
//! - `tree`: hierarchy of showing nodes (depth <= 4, capped at 120 nodes)
//! - `find`: substring/role search for showing elements (capped at 10 matches)
//! - `read`: read text of an element (capped at 2000 chars)
//! - `click`: resolve a unique actionable element by path or name, and run its default action
//! - `type_text`: set text on an editable element (<= 2000 chars)
//!
//! Safety:
//! - No shell commands, no unsafe code.
//! - Password fields are strictly forbidden: never read, returned, or typed into.
//! - Dangerous click targets are classified via whole-word label analysis.
//! - Element text is never logged or printed.

#![cfg(target_os = "linux")]
#![forbid(unsafe_code)]

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::adapters::ActionAdapter;
use crate::request::{ActionRequest, ActionResult};

/// What a UI verb can do to the user's session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UiRisk {
    /// Read-only inspection (apps, tree, find, read).
    Read,
    /// Changes session state reversibly (type_text, normal click).
    Reversible,
    /// Potentially destructive action (click on delete/submit/logout/shutdown etc.).
    Destructive,
}

/// The static risk class of a UI verb; `None` for unknown verbs.
pub fn ui_risk_of(action: &str) -> Option<UiRisk> {
    Some(match action {
        "apps" | "tree" | "find" | "read" => UiRisk::Read,
        "type_text" | "click" => UiRisk::Reversible,
        _ => return None,
    })
}

/// Normalized dangerous word/phrase targets for whole-word label classification.
const DANGEROUS_TARGETS: &[&[&str]] = &[
    &["delete"],
    &["remove"],
    &["erase"],
    &["format"],
    &["uninstall"],
    &["empty", "trash"],
    &["send"],
    &["submit"],
    &["post"],
    &["publish"],
    &["pay"],
    &["buy"],
    &["purchase"],
    &["order"],
    &["confirm"],
    &["sign", "out"],
    &["log", "out"],
    &["logout"],
    &["shut", "down"],
    &["shutdown"],
    &["restart"],
    &["power", "off"],
    &["overwrite"],
    &["reset"],
    &["clear", "all"],
    &["discard"],
    &["install"],
    &["trash"],
    &["kill"],
    &["terminate"],
    &["unsubscribe"],
    &["revoke"],
    &["allow"],
    &["grant"],
    &["authorize"],
];

/// Returns true when the label (case-insensitive, whole-word) contains any dangerous term.
pub fn label_is_dangerous(label: &str) -> bool {
    let lower = label.to_lowercase();
    let cleaned: String = lower
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    DANGEROUS_TARGETS.iter().any(|target| {
        if target.len() > words.len() {
            return false;
        }
        words.windows(target.len()).any(|window| window == *target)
    })
}

/// Validates a path string composed of slash-separated child indexes (e.g. "0/2/5/1").
/// Must contain digits and slashes only, with 1 to `MAX_DEPTH` segments.
pub fn validate_path(path: &str) -> Result<Vec<usize>, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("path cannot be empty".to_string());
    }
    let segments: Vec<&str> = trimmed.split('/').collect();
    if segments.is_empty() || segments.len() > MAX_DEPTH {
        return Err(format!(
            "path has {} segments, but maximum allowed is {MAX_DEPTH}",
            segments.len()
        ));
    }
    let mut indexes = Vec::with_capacity(segments.len());
    for seg in segments {
        if seg.is_empty() || !seg.chars().all(|c| c.is_ascii_digit()) {
            return Err(format!(
                "invalid path segment \"{seg}\": digits only, no empty segments"
            ));
        }
        let idx = seg
            .parse::<usize>()
            .map_err(|e| format!("invalid path index \"{seg}\": {e}"))?;
        indexes.push(idx);
    }
    Ok(indexes)
}

/// A role as lowercase words: "PushButton" and "push_button" are both
/// ["push", "button"].
fn role_words(role: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut prev_lower = false;
    for c in role.chars() {
        if !c.is_alphanumeric() {
            if !cur.is_empty() {
                words.push(std::mem::take(&mut cur));
            }
            prev_lower = false;
            continue;
        }
        if c.is_uppercase() && prev_lower && !cur.is_empty() {
            words.push(std::mem::take(&mut cur));
        }
        cur.extend(c.to_lowercase());
        prev_lower = c.is_lowercase() || c.is_ascii_digit();
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words
}

/// Whether a node's role satisfies a role filter: every word of the filter is
/// a word of the role, so "button" finds a push button and a toggle button,
/// and "push button" only the first.
pub fn role_matches(role: &str, filter: &str) -> bool {
    let have = role_words(role);
    let want = role_words(filter);
    !want.is_empty() && want.iter().all(|w| have.contains(w))
}

/// Checks whether an accessible role represents a password field.
pub fn is_password_role(role: &str) -> bool {
    let r = role.to_lowercase().replace(['_', '-'], " ");
    r == "password"
        || r == "password text"
        || r == "passwordtext"
        || r.split_whitespace().any(|w| w == "password")
}

/// Information about an accessible application on the bus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppInfo {
    pub name: String,
    pub id: String,
}

/// Details of an accessible node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeDetails {
    pub id: String,
    pub name: String,
    pub role: String,
    pub states: Vec<String>,
    pub has_action: bool,
    pub is_editable: bool,
    pub is_showing: bool,
    pub is_password: bool,
    pub child_ids: Vec<String>,
}

/// Result of resolving a click target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedTarget {
    pub path: String,
    pub label: String,
}

/// Abstract backend interface for accessibility operations.
#[async_trait]
pub trait UiBackend: Send + Sync {
    /// List accessible applications (name, id).
    async fn list_applications(&self) -> Result<Vec<AppInfo>, String>;

    /// Read name, role, states, and child IDs of a node by its opaque node id.
    async fn get_node(&self, node_id: &str) -> Result<NodeDetails, String>;

    /// List the children of a node by its opaque node id.
    async fn get_children(&self, node_id: &str) -> Result<Vec<NodeDetails>, String> {
        let node = self.get_node(node_id).await?;
        let mut children = Vec::with_capacity(node.child_ids.len());
        for cid in &node.child_ids {
            children.push(self.get_node(cid).await?);
        }
        Ok(children)
    }

    /// Perform the default action on a node.
    async fn perform_default_action(&self, node_id: &str) -> Result<bool, String>;

    /// Type text into an editable node, the way a person typing would: at
    /// its caret, keeping what is already there.
    async fn set_text(&self, node_id: &str, text: &str) -> Result<bool, String>;

    /// Read text of a node.
    async fn read_text(&self, node_id: &str) -> Result<String, String>;
}

/// Real AT-SPI backend connected over the session accessibility bus.
pub struct AtspiBackend {
    conn: tokio::sync::OnceCell<atspi::connection::AccessibilityConnection>,
}

impl Default for AtspiBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl AtspiBackend {
    pub fn new() -> Self {
        Self {
            conn: tokio::sync::OnceCell::new(),
        }
    }

    async fn connection(&self) -> Result<&atspi::connection::AccessibilityConnection, String> {
        self.conn
            .get_or_try_init(|| async {
                let conn = atspi::connection::AccessibilityConnection::new()
                    .await
                    .map_err(|e| format!("the accessibility bus isn't running: {e}"))?;
                // Apps only describe themselves once the session's accessibility
                // status is on (it is off by default). Turn it on the first time
                // FRIDAY looks inside an app, and give open apps a moment to
                // register. Best effort: a refusal just means fewer apps show.
                if atspi::connection::set_session_accessibility(true)
                    .await
                    .is_ok()
                {
                    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                }
                Ok(conn)
            })
            .await
    }
}

fn parse_atspi_id(id: &str) -> Result<(&str, &str), String> {
    if let Some((bus, path)) = id.split_once('#') {
        Ok((bus, path))
    } else {
        Err(format!("invalid atspi node id \"{id}\""))
    }
}

#[async_trait]
impl UiBackend for AtspiBackend {
    async fn list_applications(&self) -> Result<Vec<AppInfo>, String> {
        let conn = self.connection().await?;
        let registry = atspi::proxy::accessible::AccessibleProxy::builder(conn.connection())
            .destination("org.a11y.atspi.Registry")
            .map_err(|e| format!("invalid registry destination: {e}"))?
            .path("/org/a11y/atspi/accessible/root")
            .map_err(|e| format!("invalid registry path: {e}"))?
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await
            .map_err(|e| format!("cannot connect to atspi registry: {e}"))?;

        let children = registry
            .get_children()
            .await
            .map_err(|e| format!("cannot list applications from registry: {e}"))?;

        let mut apps = Vec::new();
        for child_ref in children {
            if child_ref.is_null() {
                continue;
            }
            let bus_name = match child_ref.name_as_str() {
                Some(name) => name.to_string(),
                None => continue,
            };
            let path = child_ref.path_as_str().to_string();
            let app_id = format!("{bus_name}#{path}");

            let app_name =
                match atspi::proxy::accessible::AccessibleProxy::builder(conn.connection())
                    .destination(bus_name.as_str())
                    .and_then(|b| b.path(path.as_str()))
                {
                    Ok(b) => match b
                        .cache_properties(zbus::proxy::CacheProperties::No)
                        .build()
                        .await
                    {
                        Ok(proxy) => proxy.name().await.unwrap_or_else(|_| bus_name.clone()),
                        Err(_) => bus_name.clone(),
                    },
                    Err(_) => bus_name.clone(),
                };

            apps.push(AppInfo {
                name: app_name,
                id: app_id,
            });
        }
        Ok(apps)
    }

    async fn get_node(&self, node_id: &str) -> Result<NodeDetails, String> {
        let conn = self.connection().await?;
        let (bus_name, object_path) = parse_atspi_id(node_id)?;

        let proxy = atspi::proxy::accessible::AccessibleProxy::builder(conn.connection())
            .destination(bus_name)
            .map_err(|e| format!("invalid destination \"{bus_name}\": {e}"))?
            .path(object_path)
            .map_err(|e| format!("invalid object path \"{object_path}\": {e}"))?
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await
            .map_err(|e| format!("failed to build proxy for node: {e}"))?;

        let name = proxy.name().await.unwrap_or_default();
        let role = proxy.get_role().await.unwrap_or(atspi::Role::Invalid);
        let role_name = format!("{role:?}");
        let state_set = proxy.get_state().await.unwrap_or_default();
        let is_showing = state_set.contains(atspi::State::Showing);
        let mut is_editable = state_set.contains(atspi::State::Editable);
        let is_password = role == atspi::Role::PasswordText || is_password_role(&role_name);

        let mut has_action = false;
        if let Ok(interfaces) = proxy.get_interfaces().await {
            // GTK4 lists the Action interface on nearly every widget, most of
            // them with no actions: an element can be pressed only if it has one.
            if interfaces.contains(atspi::Interface::Action) {
                if let Ok(builder) = atspi::proxy::action::ActionProxy::builder(conn.connection())
                    .destination(bus_name)
                    .and_then(|b| b.path(object_path))
                {
                    if let Ok(actions) = builder
                        .cache_properties(zbus::proxy::CacheProperties::No)
                        .build()
                        .await
                    {
                        has_action = actions.nactions().await.unwrap_or(0) > 0;
                    }
                }
            }
            if interfaces.contains(atspi::Interface::EditableText) {
                is_editable = true;
            }
        }

        let mut child_ids = Vec::new();
        if let Ok(children) = proxy.get_children().await {
            for child in children {
                if !child.is_null() {
                    let c_bus = child.name_as_str().unwrap_or(bus_name);
                    let c_path = child.path_as_str();
                    child_ids.push(format!("{c_bus}#{c_path}"));
                }
            }
        }

        // A list row has no action but can be chosen: pressing it selects it.
        if state_set.contains(atspi::State::Selectable) {
            has_action = true;
        }

        let states: Vec<String> = state_set.iter().map(|s| format!("{s:?}")).collect();

        Ok(NodeDetails {
            id: node_id.to_string(),
            name,
            role: role_name,
            states,
            has_action,
            is_editable,
            is_showing,
            is_password,
            child_ids,
        })
    }

    async fn perform_default_action(&self, node_id: &str) -> Result<bool, String> {
        let conn = self.connection().await?;
        let (bus_name, object_path) = parse_atspi_id(node_id)?;

        let action_proxy = atspi::proxy::action::ActionProxy::builder(conn.connection())
            .destination(bus_name)
            .map_err(|e| format!("invalid destination: {e}"))?
            .path(object_path)
            .map_err(|e| format!("invalid path: {e}"))?
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await
            .map_err(|e| format!("cannot create action proxy: {e}"))?;

        if action_proxy.nactions().await.unwrap_or(0) > 0 {
            return action_proxy
                .do_action(0)
                .await
                .map_err(|e| format!("do_action failed: {e}"));
        }

        // No action: a selectable row is chosen through the list it is in.
        let accessible = atspi::proxy::accessible::AccessibleProxy::builder(conn.connection())
            .destination(bus_name)
            .map_err(|e| format!("invalid destination: {e}"))?
            .path(object_path)
            .map_err(|e| format!("invalid path: {e}"))?
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await
            .map_err(|e| format!("cannot create accessible proxy: {e}"))?;
        let index = accessible
            .get_index_in_parent()
            .await
            .map_err(|e| format!("cannot find the element's place in its list: {e}"))?;
        let parent = accessible
            .parent()
            .await
            .map_err(|e| format!("cannot find the element's list: {e}"))?;
        let (Some(parent_bus), parent_path) = (parent.name_as_str(), parent.path_as_str()) else {
            return Err("the element has no list to choose it from".to_string());
        };
        let selection = atspi::proxy::selection::SelectionProxy::builder(conn.connection())
            .destination(parent_bus)
            .map_err(|e| format!("invalid destination: {e}"))?
            .path(parent_path)
            .map_err(|e| format!("invalid path: {e}"))?
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await
            .map_err(|e| format!("cannot create selection proxy: {e}"))?;
        selection
            .select_child(index)
            .await
            .map_err(|e| format!("select failed: {e}"))
    }

    async fn set_text(&self, node_id: &str, text: &str) -> Result<bool, String> {
        let conn = self.connection().await?;
        let (bus_name, object_path) = parse_atspi_id(node_id)?;

        let editable_proxy =
            atspi::proxy::editable_text::EditableTextProxy::builder(conn.connection())
                .destination(bus_name)
                .map_err(|e| format!("invalid destination: {e}"))?
                .path(object_path)
                .map_err(|e| format!("invalid path: {e}"))?
                .cache_properties(zbus::proxy::CacheProperties::No)
                .build()
                .await
                .map_err(|e| format!("cannot create editable text proxy: {e}"))?;

        // At the caret, or at the end when the app doesn't say where it is:
        // what is already there stays.
        let text_proxy = atspi::proxy::text::TextProxy::builder(conn.connection())
            .destination(bus_name)
            .map_err(|e| format!("invalid destination: {e}"))?
            .path(object_path)
            .map_err(|e| format!("invalid path: {e}"))?
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await
            .ok();
        let mut existing = None;
        let mut at = None;
        if let Some(tp) = &text_proxy {
            let count = tp.character_count().await.ok();
            existing = count;
            at = match tp.caret_offset().await {
                Ok(c) if c >= 0 && count.is_none_or(|n| c <= n) => Some(c),
                _ => count,
            };
        }
        // The length is in bytes (ATK's, and GTK's, convention).
        let inserted = editable_proxy
            .insert_text(at.unwrap_or(0), text, text.len() as i32)
            .await
            .unwrap_or(false);
        if inserted {
            return Ok(true);
        }
        // Some toolkits take no insertion; an empty field can still be filled
        // whole, and a field with text in it is never replaced.
        if existing.unwrap_or(0) == 0 {
            return editable_proxy
                .set_text_contents(text)
                .await
                .map_err(|e| format!("set_text failed: {e}"));
        }
        Err("that field takes no typed text from here, and it already has text FRIDAY won't replace".to_string())
    }

    async fn read_text(&self, node_id: &str) -> Result<String, String> {
        let conn = self.connection().await?;
        let (bus_name, object_path) = parse_atspi_id(node_id)?;

        let text_proxy = atspi::proxy::text::TextProxy::builder(conn.connection())
            .destination(bus_name)
            .map_err(|e| format!("invalid destination: {e}"))?
            .path(object_path)
            .map_err(|e| format!("invalid path: {e}"))?
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await;

        if let Ok(tp) = text_proxy {
            if let Ok(count) = tp.character_count().await {
                if count > 0 {
                    if let Ok(t) = tp.get_text(0, count).await {
                        return Ok(t);
                    }
                }
            }
            if let Ok(t) = tp.get_text(0, -1).await {
                if !t.is_empty() {
                    return Ok(t);
                }
            }
        }

        let acc_proxy = atspi::proxy::accessible::AccessibleProxy::builder(conn.connection())
            .destination(bus_name)
            .map_err(|e| format!("invalid destination: {e}"))?
            .path(object_path)
            .map_err(|e| format!("invalid path: {e}"))?
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await
            .map_err(|e| format!("failed to build accessible proxy: {e}"))?;

        if let Ok(desc) = acc_proxy.description().await {
            if !desc.is_empty() {
                return Ok(desc);
            }
        }
        acc_proxy
            .name()
            .await
            .map_err(|e| format!("read text failed: {e}"))
    }
}

/// How deep a walk goes. GTK4 and libadwaita nest many unnamed wrapper panels,
/// so the useful controls of a window can sit well below level 12.
const MAX_DEPTH: usize = 40;
/// Most nodes one verb will look at, so a huge or runaway tree stays quick.
const VISIT_BUDGET: usize = 2500;

/// UI Adapter implementing the `ActionAdapter` trait for AT-SPI control.
pub struct UiAdapter {
    backend: Arc<dyn UiBackend>,
    visited: std::sync::atomic::AtomicUsize,
}

impl Default for UiAdapter {
    fn default() -> Self {
        Self::new()
    }
}

struct SearchCriteria<'a> {
    name_query: &'a str,
    role_filter: Option<&'a str>,
}

impl UiAdapter {
    pub fn new() -> Self {
        Self::with_backend(Arc::new(AtspiBackend::new()))
    }

    pub fn with_backend(backend: Arc<dyn UiBackend>) -> Self {
        Self {
            backend,
            visited: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Count `n` nodes against this verb's budget; true once it is used up.
    fn spend(&self, n: usize) -> bool {
        use std::sync::atomic::Ordering;
        self.visited.fetch_add(n, Ordering::Relaxed) + n > VISIT_BUDGET
    }

    /// List all accessible applications currently on the bus.
    pub async fn list_apps(&self) -> Result<Vec<AppInfo>, String> {
        self.backend.list_applications().await
    }

    /// Resolve an application query string to a unique `AppInfo`.
    async fn resolve_app(&self, app_query: &str) -> Result<AppInfo, String> {
        let q = app_query.trim().to_lowercase();
        if q.is_empty() {
            return Err("app name or id cannot be empty".to_string());
        }
        let apps = self.backend.list_applications().await?;
        if let Some(app) = apps.iter().find(|a| a.id == app_query) {
            return Ok(app.clone());
        }
        let exact: Vec<&AppInfo> = apps.iter().filter(|a| a.name.to_lowercase() == q).collect();
        if exact.len() == 1 {
            return Ok(exact[0].clone());
        }
        if exact.len() > 1 {
            let list = exact
                .iter()
                .map(|a| format!("{} ({})", a.name, a.id))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!(
                "ambiguous application \"{app_query}\"; matches: {list}"
            ));
        }
        let subs: Vec<&AppInfo> = apps
            .iter()
            .filter(|a| a.name.to_lowercase().contains(&q))
            .collect();
        match subs.as_slice() {
            [] => Err(format!("no accessible application matches \"{app_query}\"")),
            [one] => Ok((*one).clone()),
            many => {
                let list = many
                    .iter()
                    .map(|a| format!("{} ({})", a.name, a.id))
                    .collect::<Vec<_>>()
                    .join(", ");
                Err(format!(
                    "ambiguous application \"{app_query}\"; matches {} apps ({}); say which",
                    many.len(),
                    list
                ))
            }
        }
    }

    /// Traverse from an app root along a validated path to reach the target node.
    async fn resolve_path(&self, app_root_id: &str, path: &str) -> Result<NodeDetails, String> {
        let indexes = validate_path(path)?;
        let mut current_id = app_root_id.to_string();
        for (depth, &idx) in indexes.iter().enumerate() {
            let children = self.backend.get_children(&current_id).await?;
            if idx >= children.len() {
                return Err(format!(
                    "path \"{path}\" invalid at segment {depth}: child index {idx} out of bounds (has {} children)",
                    children.len()
                ));
            }
            current_id = children[idx].id.clone();
        }
        self.backend.get_node(&current_id).await
    }

    /// Collect showing elements up to `max_depth` (max 4), capped at 120 nodes.
    async fn collect_tree(
        &self,
        node_id: &str,
        current_path: &str,
        current_depth: usize,
        max_depth: usize,
        out: &mut Vec<Value>,
    ) -> Result<(), String> {
        if out.len() >= 120 {
            return Ok(());
        }
        let children = self.backend.get_children(node_id).await?;
        if self.spend(children.len()) {
            return Ok(());
        }
        for (i, child) in children.iter().enumerate() {
            if out.len() >= 120 {
                break;
            }
            let child_path = if current_path.is_empty() {
                format!("{i}")
            } else {
                format!("{current_path}/{i}")
            };

            // Safety rule (a): password fields are strictly refused and never returned.
            if child.is_password || is_password_role(&child.role) {
                continue;
            }

            // Unnamed wrapper panels are walked through, not listed.
            if child.is_showing && (!child.name.is_empty() || child.is_editable) {
                out.push(json!({
                    "path": child_path,
                    "role": child.role,
                    "name": child.name,
                }));
            }

            if current_depth < max_depth {
                Box::pin(self.collect_tree(
                    &child.id,
                    &child_path,
                    current_depth + 1,
                    max_depth,
                    out,
                ))
                .await?;
            }
        }
        Ok(())
    }

    /// Search showing elements by name substring and optional role, returning at most 10 matches.
    async fn search_nodes(
        &self,
        node_id: &str,
        current_path: &str,
        current_depth: usize,
        max_depth: usize,
        criteria: &SearchCriteria<'_>,
        matches: &mut Vec<Value>,
    ) -> Result<(), String> {
        if matches.len() >= 10 {
            return Ok(());
        }
        let children = self.backend.get_children(node_id).await?;
        if self.spend(children.len()) {
            return Ok(());
        }
        for (i, child) in children.iter().enumerate() {
            if matches.len() >= 10 {
                break;
            }
            let child_path = if current_path.is_empty() {
                format!("{i}")
            } else {
                format!("{current_path}/{i}")
            };

            if child.is_password || is_password_role(&child.role) {
                continue;
            }

            if child.is_showing {
                let name_matches = criteria.name_query.is_empty()
                    || child.name.to_lowercase().contains(criteria.name_query);
                let role_matches = match criteria.role_filter {
                    Some(r) => role_matches(&child.role, r),
                    None => true,
                };
                if name_matches && role_matches {
                    matches.push(json!({
                        "path": child_path,
                        "role": child.role,
                        "name": child.name,
                    }));
                }
            }

            if current_depth < max_depth {
                Box::pin(self.search_nodes(
                    &child.id,
                    &child_path,
                    current_depth + 1,
                    max_depth,
                    criteria,
                    matches,
                ))
                .await?;
            }
        }
        Ok(())
    }

    /// Collect all candidate clickable elements in the app subtree.
    async fn collect_click_candidates(
        &self,
        node_id: &str,
        current_path: &str,
        current_depth: usize,
        max_depth: usize,
        candidates: &mut Vec<(String, String, String, String)>, // (id, path, role, name)
    ) -> Result<(), String> {
        let children = self.backend.get_children(node_id).await?;
        if self.spend(children.len()) {
            return Ok(());
        }
        for (i, child) in children.iter().enumerate() {
            let child_path = if current_path.is_empty() {
                format!("{i}")
            } else {
                format!("{current_path}/{i}")
            };

            if child.is_password || is_password_role(&child.role) {
                continue;
            }

            if child.is_showing && child.has_action {
                candidates.push((
                    child.id.clone(),
                    child_path.clone(),
                    child.role.clone(),
                    child.name.clone(),
                ));
            }

            if current_depth < max_depth {
                Box::pin(self.collect_click_candidates(
                    &child.id,
                    &child_path,
                    current_depth + 1,
                    max_depth,
                    candidates,
                ))
                .await?;
            }
        }
        Ok(())
    }

    /// Collect all candidate editable elements in the app subtree.
    async fn collect_editable_candidates(
        &self,
        node_id: &str,
        current_path: &str,
        current_depth: usize,
        max_depth: usize,
        candidates: &mut Vec<(String, String, String, String)>, // (id, path, role, name)
    ) -> Result<(), String> {
        let children = self.backend.get_children(node_id).await?;
        if self.spend(children.len()) {
            return Ok(());
        }
        for (i, child) in children.iter().enumerate() {
            let child_path = if current_path.is_empty() {
                format!("{i}")
            } else {
                format!("{current_path}/{i}")
            };

            if child.is_showing && (child.is_password || is_password_role(&child.role)) {
                // Keep record of password fields to reject typing into them specifically
                candidates.push((
                    child.id.clone(),
                    child_path.clone(),
                    "PasswordText".to_string(),
                    child.name.clone(),
                ));
                continue;
            }

            if child.is_showing && child.is_editable {
                candidates.push((
                    child.id.clone(),
                    child_path.clone(),
                    child.role.clone(),
                    child.name.clone(),
                ));
            }

            if current_depth < max_depth {
                Box::pin(self.collect_editable_candidates(
                    &child.id,
                    &child_path,
                    current_depth + 1,
                    max_depth,
                    candidates,
                ))
                .await?;
            }
        }
        Ok(())
    }

    /// Resolve a click target by path or name, returning its path and label WITHOUT clicking.
    pub async fn resolve_click(
        &self,
        app: &str,
        path_or_name: &str,
    ) -> Result<ResolvedTarget, String> {
        let target = path_or_name.trim();
        if validate_path(target).is_ok() {
            if let Ok(res) = self.resolve_click_target(app, Some(target), None).await {
                return Ok(res);
            }
        }
        self.resolve_click_target(app, None, Some(target)).await
    }

    /// Resolve a click target with explicit optional path and name.
    pub async fn resolve_click_target(
        &self,
        app: &str,
        path: Option<&str>,
        name: Option<&str>,
    ) -> Result<ResolvedTarget, String> {
        let app_info = self.resolve_app(app).await?;

        match (
            path.filter(|p| !p.trim().is_empty()),
            name.filter(|n| !n.trim().is_empty()),
        ) {
            (Some(_), Some(_)) => Err("provide either path or name, not both".to_string()),
            (None, None) => Err("provide either path or name to click".to_string()),
            (Some(p), None) => {
                let node = self.resolve_path(&app_info.id, p).await?;
                if node.is_password || is_password_role(&node.role) {
                    return Err("cannot interact with password field".to_string());
                }
                if !node.has_action {
                    return Err(format!("element at path \"{p}\" has no action"));
                }
                Ok(ResolvedTarget {
                    path: p.to_string(),
                    label: node.name,
                })
            }
            (None, Some(n)) => {
                let mut candidates = Vec::new();
                self.collect_click_candidates(&app_info.id, "", 0, MAX_DEPTH, &mut candidates)
                    .await?;

                let t = n.trim().to_lowercase();
                let exact: Vec<&(String, String, String, String)> = candidates
                    .iter()
                    .filter(|c| c.3.trim().to_lowercase() == t)
                    .collect();

                let hits = if !exact.is_empty() {
                    exact
                } else {
                    candidates
                        .iter()
                        .filter(|c| c.3.to_lowercase().contains(&t))
                        .collect()
                };

                match hits.as_slice() {
                    [] => {
                        // On screen but with nothing to press (GTK4 apps expose
                        // no button actions): say that, not "no such element".
                        let mut seen = Vec::new();
                        let criteria = SearchCriteria {
                            name_query: &t,
                            role_filter: None,
                        };
                        let _ = self
                            .search_nodes(&app_info.id, "", 1, MAX_DEPTH, &criteria, &mut seen)
                            .await;
                        if seen.is_empty() {
                            Err(format!(
                                "no clickable element in \"{}\" matches \"{n}\"",
                                app_info.name
                            ))
                        } else {
                            Err(format!(
                                "\"{n}\" is on screen in \"{}\", but the app offers no way to press it from here (GTK4 apps don't expose button actions)",
                                app_info.name
                            ))
                        }
                    }
                    [one] => Ok(ResolvedTarget {
                        path: one.1.clone(),
                        label: one.3.clone(),
                    }),
                    many => {
                        let details = many
                            .iter()
                            .map(|c| format!("{}: \"{}\" at {}", c.2, c.3, c.1))
                            .collect::<Vec<_>>()
                            .join("; ");
                        Err(format!(
                            "ambiguous click target \"{n}\"; matches {} elements ({}); say which path to click",
                            many.len(),
                            details
                        ))
                    }
                }
            }
        }
    }

    /// Execute a UI verb.
    async fn run_action(
        &self,
        req: &ActionRequest,
        base_risk: UiRisk,
    ) -> (Result<Value, String>, UiRisk) {
        let p = |k: &str| {
            req.parameters
                .get(k)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
        };

        match req.action.as_str() {
            "apps" => {
                let apps = match self.backend.list_applications().await {
                    Ok(a) => a,
                    Err(e) => return (Err(e), base_risk),
                };
                let val = json!(apps
                    .into_iter()
                    .map(|a| json!({"name": a.name, "id": a.id}))
                    .collect::<Vec<_>>());
                (Ok(val), base_risk)
            }
            "tree" => {
                let app = match p("app") {
                    Some(a) => a,
                    None => return (Err("name the app to inspect tree".to_string()), base_risk),
                };
                let depth = match req.parameters.get("depth") {
                    Some(v) => {
                        let d = v
                            .as_u64()
                            .or_else(|| v.as_str().and_then(|s| s.parse().ok()));
                        match d {
                            Some(n) if n as usize > MAX_DEPTH => {
                                return (
                                    Err(format!("depth cannot exceed {MAX_DEPTH}")),
                                    base_risk,
                                );
                            }
                            Some(0) => {
                                return (Err("depth must be at least 1".to_string()), base_risk);
                            }
                            Some(n) => n as usize,
                            None => {
                                return (
                                    Err(format!(
                                        "depth must be an integer between 1 and {MAX_DEPTH}"
                                    )),
                                    base_risk,
                                )
                            }
                        }
                    }
                    None => MAX_DEPTH,
                };

                let app_info = match self.resolve_app(app).await {
                    Ok(info) => info,
                    Err(e) => return (Err(e), base_risk),
                };

                let mut nodes = Vec::new();
                if let Err(e) = self
                    .collect_tree(&app_info.id, "", 1, depth, &mut nodes)
                    .await
                {
                    return (Err(e), base_risk);
                }
                (Ok(json!(nodes)), base_risk)
            }
            "find" => {
                let app = match p("app") {
                    Some(a) => a,
                    None => return (Err("name the app to search".to_string()), base_risk),
                };
                let name_query = p("name").unwrap_or("");
                let role_filter = p("role");

                let app_info = match self.resolve_app(app).await {
                    Ok(info) => info,
                    Err(e) => return (Err(e), base_risk),
                };

                let mut matches = Vec::new();
                let criteria = SearchCriteria {
                    name_query: &name_query.to_lowercase(),
                    role_filter,
                };
                if let Err(e) = self
                    .search_nodes(&app_info.id, "", 1, MAX_DEPTH, &criteria, &mut matches)
                    .await
                {
                    return (Err(e), base_risk);
                }
                (Ok(json!(matches)), base_risk)
            }
            "read" => {
                let app = match p("app") {
                    Some(a) => a,
                    None => return (Err("name the app to read from".to_string()), base_risk),
                };
                let path = match p("path") {
                    Some(path) => path,
                    None => {
                        return (
                            Err("path is required to read element text".to_string()),
                            base_risk,
                        )
                    }
                };

                let app_info = match self.resolve_app(app).await {
                    Ok(info) => info,
                    Err(e) => return (Err(e), base_risk),
                };

                let node = match self.resolve_path(&app_info.id, path).await {
                    Ok(n) => n,
                    Err(e) => return (Err(e), base_risk),
                };

                if node.is_password || is_password_role(&node.role) {
                    return (
                        Err("refusing to read password field".to_string()),
                        base_risk,
                    );
                }

                let text = match self.backend.read_text(&node.id).await {
                    Ok(t) => t,
                    Err(e) => return (Err(e), base_risk),
                };

                let truncated: String = text.chars().take(2000).collect();
                (Ok(json!({ "text": truncated, "path": path })), base_risk)
            }
            "click" => {
                let app = match p("app") {
                    Some(a) => a,
                    None => return (Err("name the app to click in".to_string()), base_risk),
                };

                let resolved = match self.resolve_click_target(app, p("path"), p("name")).await {
                    Ok(t) => t,
                    Err(e) => return (Err(e), base_risk),
                };

                let actual_risk = if label_is_dangerous(&resolved.label) {
                    UiRisk::Destructive
                } else {
                    UiRisk::Reversible
                };

                // Defense in depth: a button that could lose or send something
                // is never pressed on the adapter's own say-so. The caller asks
                // the person first, then passes `"confirmed": true`.
                if actual_risk == UiRisk::Destructive
                    && req.parameters.get("confirmed").and_then(Value::as_bool) != Some(true)
                {
                    return (
                        Err(format!(
                            "\"{}\" could lose or send something, so it needs confirmation first",
                            resolved.label
                        )),
                        actual_risk,
                    );
                }

                let app_info = match self.resolve_app(app).await {
                    Ok(info) => info,
                    Err(e) => return (Err(e), actual_risk),
                };

                let node = match self.resolve_path(&app_info.id, &resolved.path).await {
                    Ok(n) => n,
                    Err(e) => return (Err(e), actual_risk),
                };

                match self.backend.perform_default_action(&node.id).await {
                    Ok(true) => (
                        Ok(json!({
                            "clicked": resolved.label,
                            "path": resolved.path,
                        })),
                        actual_risk,
                    ),
                    Ok(false) => (
                        Err(format!(
                            "default action on \"{}\" reported failure",
                            resolved.label
                        )),
                        actual_risk,
                    ),
                    Err(e) => (Err(e), actual_risk),
                }
            }
            "type_text" => {
                let app = match p("app") {
                    Some(a) => a,
                    None => return (Err("name the app to type into".to_string()), base_risk),
                };
                let text = match req.parameters.get("text").and_then(Value::as_str) {
                    Some(t) => t,
                    None => return (Err("text parameter is required".to_string()), base_risk),
                };

                if text.chars().count() > 2000 {
                    return (
                        Err("text exceeds maximum length of 2000 characters".to_string()),
                        base_risk,
                    );
                }

                let app_info = match self.resolve_app(app).await {
                    Ok(info) => info,
                    Err(e) => return (Err(e), base_risk),
                };

                let target_node = match (
                    p("path").filter(|p| !p.trim().is_empty()),
                    p("name").filter(|n| !n.trim().is_empty()),
                ) {
                    (Some(_), Some(_)) => {
                        return (
                            Err("provide either path or name, not both".to_string()),
                            base_risk,
                        )
                    }
                    (None, None) => {
                        return (
                            Err("provide either path or name to type into".to_string()),
                            base_risk,
                        )
                    }
                    (Some(path), None) => {
                        let node = match self.resolve_path(&app_info.id, path).await {
                            Ok(n) => n,
                            Err(e) => return (Err(e), base_risk),
                        };
                        if node.is_password || is_password_role(&node.role) {
                            return (
                                Err("refusing to type into password field".to_string()),
                                base_risk,
                            );
                        }
                        if !node.is_editable {
                            return (
                                Err(format!("element at path \"{path}\" is not editable")),
                                base_risk,
                            );
                        }
                        (node, path.to_string())
                    }
                    (None, Some(name)) => {
                        let mut candidates = Vec::new();
                        if let Err(e) = self
                            .collect_editable_candidates(
                                &app_info.id,
                                "",
                                0,
                                MAX_DEPTH,
                                &mut candidates,
                            )
                            .await
                        {
                            return (Err(e), base_risk);
                        }

                        let t = name.trim().to_lowercase();
                        let exact: Vec<&(String, String, String, String)> = candidates
                            .iter()
                            .filter(|c| c.3.trim().to_lowercase() == t)
                            .collect();

                        let hits = if !exact.is_empty() {
                            exact
                        } else {
                            candidates
                                .iter()
                                .filter(|c| c.3.to_lowercase().contains(&t))
                                .collect()
                        };

                        match hits.as_slice() {
                            [] => {
                                return (
                                    Err(format!(
                                        "no editable element in \"{}\" matches \"{name}\"",
                                        app_info.name
                                    )),
                                    base_risk,
                                )
                            }
                            [one] => {
                                if one.2 == "PasswordText" || is_password_role(&one.2) {
                                    return (
                                        Err("refusing to type into password field".to_string()),
                                        base_risk,
                                    );
                                }
                                let node = match self.backend.get_node(&one.0).await {
                                    Ok(n) => n,
                                    Err(e) => return (Err(e), base_risk),
                                };
                                (node, one.1.clone())
                            }
                            many => {
                                let details = many
                                    .iter()
                                    .map(|c| format!("{}: \"{}\" at {}", c.2, c.3, c.1))
                                    .collect::<Vec<_>>()
                                    .join("; ");
                                return (
                                    Err(format!(
                                        "ambiguous editable target \"{name}\"; matches {} elements ({}); say which path to type into",
                                        many.len(),
                                        details
                                    )),
                                    base_risk,
                                );
                            }
                        }
                    }
                };

                let (node, path) = target_node;
                match self.backend.set_text(&node.id, text).await {
                    Ok(_) => (
                        Ok(json!({
                            "typed": text.chars().count(),
                            "path": path,
                        })),
                        base_risk,
                    ),
                    Err(e) => (Err(e), base_risk),
                }
            }
            other => (Err(format!("the ui adapter has no \"{other}\"")), base_risk),
        }
    }
}

#[async_trait]
impl ActionAdapter for UiAdapter {
    fn tool(&self) -> &str {
        "ui"
    }

    fn actions(&self) -> Vec<&str> {
        vec!["apps", "tree", "find", "read", "click", "type_text"]
    }

    async fn execute(&self, req: ActionRequest) -> ActionResult {
        let started = std::time::Instant::now();
        self.visited.store(0, std::sync::atomic::Ordering::Relaxed);
        let base_risk = match ui_risk_of(&req.action) {
            Some(r) => r,
            None => {
                return ActionResult::error(format!("the ui adapter has no \"{}\"", req.action));
            }
        };

        let (res, actual_risk) = self.run_action(&req, base_risk).await;
        let out = match res {
            Ok(v) => ActionResult::success(v),
            Err(e) => ActionResult::error(e),
        };

        out.with_latency_ms(started.elapsed().as_millis())
            .with_metadata("risk", json!(format!("{actual_risk:?}").to_lowercase()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    #[derive(Clone, Debug)]
    struct FakeNode {
        id: String,
        name: String,
        role: String,
        states: Vec<String>,
        has_action: bool,
        is_editable: bool,
        is_showing: bool,
        is_password: bool,
        children: Vec<String>,
        text: String,
        action_invoked: Arc<AtomicBool>,
    }

    struct FakeUiBackend {
        apps: Vec<AppInfo>,
        nodes: Mutex<HashMap<String, FakeNode>>,
    }

    impl FakeUiBackend {
        fn new() -> Self {
            Self {
                apps: Vec::new(),
                nodes: Mutex::new(HashMap::new()),
            }
        }

        fn add_app(&mut self, name: &str, id: &str) {
            self.apps.push(AppInfo {
                name: name.to_string(),
                id: id.to_string(),
            });
        }

        fn add_node(&self, node: FakeNode) {
            self.nodes.lock().unwrap().insert(node.id.clone(), node);
        }
    }

    #[async_trait]
    impl UiBackend for FakeUiBackend {
        async fn list_applications(&self) -> Result<Vec<AppInfo>, String> {
            Ok(self.apps.clone())
        }

        async fn get_node(&self, node_id: &str) -> Result<NodeDetails, String> {
            let nodes = self.nodes.lock().unwrap();
            let n = nodes
                .get(node_id)
                .ok_or_else(|| format!("node \"{node_id}\" not found"))?;
            Ok(NodeDetails {
                id: n.id.clone(),
                name: n.name.clone(),
                role: n.role.clone(),
                states: n.states.clone(),
                has_action: n.has_action,
                is_editable: n.is_editable,
                is_showing: n.is_showing,
                is_password: n.is_password,
                child_ids: n.children.clone(),
            })
        }

        async fn perform_default_action(&self, node_id: &str) -> Result<bool, String> {
            let nodes = self.nodes.lock().unwrap();
            let n = nodes
                .get(node_id)
                .ok_or_else(|| format!("node \"{node_id}\" not found"))?;
            n.action_invoked.store(true, Ordering::SeqCst);
            Ok(true)
        }

        async fn set_text(&self, node_id: &str, text: &str) -> Result<bool, String> {
            let mut nodes = self.nodes.lock().unwrap();
            let n = nodes
                .get_mut(node_id)
                .ok_or_else(|| format!("node \"{node_id}\" not found"))?;
            n.text = text.to_string();
            Ok(true)
        }

        async fn read_text(&self, node_id: &str) -> Result<String, String> {
            let nodes = self.nodes.lock().unwrap();
            let n = nodes
                .get(node_id)
                .ok_or_else(|| format!("node \"{node_id}\" not found"))?;
            Ok(n.text.clone())
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn make_test_node(
        id: &str,
        name: &str,
        role: &str,
        showing: bool,
        action: bool,
        editable: bool,
        password: bool,
        children: Vec<&str>,
    ) -> FakeNode {
        FakeNode {
            id: id.to_string(),
            name: name.to_string(),
            role: role.to_string(),
            states: if showing {
                vec!["showing".into()]
            } else {
                vec![]
            },
            has_action: action,
            is_editable: editable,
            is_showing: showing,
            is_password: password,
            children: children.into_iter().map(String::from).collect(),
            text: String::new(),
            action_invoked: Arc::new(AtomicBool::new(false)),
        }
    }

    #[test]
    fn test_path_validation() {
        assert!(validate_path("../x").is_err());
        assert!(validate_path("1//2").is_err());
        assert!(validate_path("a/b").is_err());
        let too_deep = vec!["0"; MAX_DEPTH + 1].join("/");
        assert!(validate_path(&too_deep).is_err());
        assert!(validate_path("").is_err());
        assert!(validate_path("/0/1").is_err());
        assert!(validate_path("0/1/").is_err());

        assert_eq!(validate_path("0/2/5/1").unwrap(), vec![0, 2, 5, 1]);
        assert_eq!(validate_path("0").unwrap(), vec![0]);
        let deepest = vec!["0"; MAX_DEPTH].join("/");
        assert_eq!(validate_path(&deepest).unwrap().len(), MAX_DEPTH);
    }

    #[test]
    fn test_label_is_dangerous_table() {
        assert!(label_is_dangerous("Delete"));
        assert!(label_is_dangerous("delete"));
        assert!(label_is_dangerous("Please Delete file"));
        assert!(!label_is_dangerous("Undelete notes"));
        assert!(label_is_dangerous("Send"));
        assert!(!label_is_dangerous("Resend"));
        assert!(!label_is_dangerous("Settings"));
        assert!(label_is_dangerous("Empty Trash"));
        assert!(label_is_dangerous("empty-trash"));
        assert!(label_is_dangerous("Sign Out"));
        assert!(label_is_dangerous("Log Out"));
        assert!(label_is_dangerous("Logout"));
        assert!(label_is_dangerous("Shut Down"));
        assert!(label_is_dangerous("Shutdown"));
        assert!(label_is_dangerous("Restart"));
        assert!(label_is_dangerous("Power Off"));
        assert!(label_is_dangerous("Clear All"));
        assert!(label_is_dangerous("Reset all"));
        assert!(label_is_dangerous("Discard"));
        assert!(label_is_dangerous("Confirm"));
        assert!(label_is_dangerous("Pay now"));
        assert!(label_is_dangerous("Buy"));
        assert!(label_is_dangerous("Purchase item"));
        assert!(label_is_dangerous("Order"));
        assert!(label_is_dangerous("Submit form"));
        assert!(label_is_dangerous("Post comment"));
        assert!(label_is_dangerous("Publish"));
        assert!(label_is_dangerous("Overwrite file"));
        assert!(label_is_dangerous("Format disk"));
        assert!(label_is_dangerous("Uninstall app"));
        assert!(label_is_dangerous("Install"));
        assert!(label_is_dangerous("Move to Trash"));
        assert!(label_is_dangerous("Allow"));
        assert!(label_is_dangerous("Grant access"));
        assert!(!label_is_dangerous("Installer notes"));
        assert!(!label_is_dangerous("Allowance"));

        assert!(!label_is_dangerous("Save"));
        assert!(!label_is_dangerous("Open file"));
        assert!(!label_is_dangerous("Cancel"));
        assert!(!label_is_dangerous("Border"));
        assert!(!label_is_dangerous("Poster"));
    }

    #[test]
    fn test_ui_risk_classification() {
        assert_eq!(ui_risk_of("apps"), Some(UiRisk::Read));
        assert_eq!(ui_risk_of("tree"), Some(UiRisk::Read));
        assert_eq!(ui_risk_of("find"), Some(UiRisk::Read));
        assert_eq!(ui_risk_of("read"), Some(UiRisk::Read));
        assert_eq!(ui_risk_of("type_text"), Some(UiRisk::Reversible));
        assert_eq!(ui_risk_of("click"), Some(UiRisk::Reversible));
        assert_eq!(ui_risk_of("unknown_verb"), None);
    }

    #[tokio::test]
    async fn test_tree_depth_and_node_caps_and_showing_filter() {
        let mut backend = FakeUiBackend::new();
        backend.add_app("TestApp", "app-root");

        // Hierarchy:
        // root -> c0 (showing), c1 (not showing), c2 (showing)
        // c0 -> c0_0 (showing) -> c0_0_0 (showing) -> c0_0_0_0 (showing) -> c0_0_0_0_0 (showing: depth 5)
        backend.add_node(make_test_node(
            "app-root",
            "App",
            "application",
            true,
            false,
            false,
            false,
            vec!["c0", "c1", "c2"],
        ));
        backend.add_node(make_test_node(
            "c0",
            "Child 0",
            "panel",
            true,
            false,
            false,
            false,
            vec!["c0_0"],
        ));
        backend.add_node(make_test_node(
            "c1",
            "Child 1",
            "panel",
            false,
            false,
            false,
            false,
            vec![],
        ));
        backend.add_node(make_test_node(
            "c2",
            "Child 2",
            "button",
            true,
            true,
            false,
            false,
            vec![],
        ));
        backend.add_node(make_test_node(
            "c0_0",
            "Child 0-0",
            "panel",
            true,
            false,
            false,
            false,
            vec!["c0_0_0"],
        ));
        backend.add_node(make_test_node(
            "c0_0_0",
            "Child 0-0-0",
            "panel",
            true,
            false,
            false,
            false,
            vec!["c0_0_0_0"],
        ));
        backend.add_node(make_test_node(
            "c0_0_0_0",
            "Child 0-0-0-0",
            "panel",
            true,
            false,
            false,
            false,
            vec!["c0_0_0_0_0"],
        ));
        backend.add_node(make_test_node(
            "c0_0_0_0_0",
            "Child 0-0-0-0-0",
            "panel",
            true,
            false,
            false,
            false,
            vec![],
        ));

        let adapter = UiAdapter::with_backend(Arc::new(backend));

        // Depth <= 4: depth 5 should NOT be included. Non-showing Child 1 should NOT be included.
        let req = ActionRequest::new("ui", "tree", json!({ "app": "TestApp", "depth": 4 }));
        let res = adapter.execute(req).await;
        assert!(res.success);
        let arr = res.output.as_array().unwrap();
        // Showing nodes at depths 1..=4:
        // depth 1: "0" (Child 0), "2" (Child 2). ("1" is not showing)
        // depth 2: "0/0" (Child 0-0)
        // depth 3: "0/0/0" (Child 0-0-0)
        // depth 4: "0/0/0/0" (Child 0-0-0-0)
        // depth 5: "0/0/0/0/0" must be excluded!
        let paths: Vec<&str> = arr.iter().map(|n| n["path"].as_str().unwrap()).collect();
        assert!(paths.contains(&"0"));
        assert!(paths.contains(&"2"));
        assert!(!paths.contains(&"1"));
        assert!(paths.contains(&"0/0"));
        assert!(paths.contains(&"0/0/0"));
        assert!(paths.contains(&"0/0/0/0"));
        assert!(!paths.contains(&"0/0/0/0/0"));
        assert_eq!(res.metadata["risk"], "read");

        // Depth 5 reaches the fifth level; more than the maximum is rejected.
        let deeper = adapter
            .execute(ActionRequest::new(
                "ui",
                "tree",
                json!({ "app": "TestApp", "depth": 5 }),
            ))
            .await;
        assert!(deeper.success);
        let paths: Vec<&str> = deeper
            .output
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["path"].as_str().unwrap())
            .collect();
        assert!(paths.contains(&"0/0/0/0/0"));
        let req_err = ActionRequest::new("ui", "tree", json!({ "app": "TestApp", "depth": 41 }));
        let res_err = adapter.execute(req_err).await;
        assert!(!res_err.success);
        assert!(res_err.error.unwrap().contains("depth cannot exceed 40"));
    }

    #[tokio::test]
    async fn test_tree_node_cap_120() {
        let mut backend = FakeUiBackend::new();
        backend.add_app("CapApp", "root");

        let mut child_ids = Vec::new();
        for i in 0..150 {
            let cid = format!("node_{i}");
            backend.add_node(make_test_node(
                &cid,
                &format!("Node {i}"),
                "button",
                true,
                true,
                false,
                false,
                vec![],
            ));
            child_ids.push(cid);
        }
        let child_refs: Vec<&str> = child_ids.iter().map(String::as_str).collect();
        backend.add_node(make_test_node(
            "root",
            "Root",
            "application",
            true,
            false,
            false,
            false,
            child_refs,
        ));

        let adapter = UiAdapter::with_backend(Arc::new(backend));
        let req = ActionRequest::new("ui", "tree", json!({ "app": "CapApp" }));
        let res = adapter.execute(req).await;
        assert!(res.success);
        let arr = res.output.as_array().unwrap();
        assert_eq!(arr.len(), 120);
    }

    #[tokio::test]
    async fn test_find_case_insensitive_role_filter_and_cap() {
        let mut backend = FakeUiBackend::new();
        backend.add_app("SearchApp", "root");

        // Add 15 matching buttons and 5 matching labels
        let mut child_ids = Vec::new();
        for i in 0..15 {
            let cid = format!("btn_{i}");
            backend.add_node(make_test_node(
                &cid,
                &format!("Save File {i}"),
                "button",
                true,
                true,
                false,
                false,
                vec![],
            ));
            child_ids.push(cid);
        }
        for i in 0..5 {
            let cid = format!("lbl_{i}");
            backend.add_node(make_test_node(
                &cid,
                &format!("save label {i}"),
                "label",
                true,
                false,
                false,
                false,
                vec![],
            ));
            child_ids.push(cid);
        }
        let child_refs: Vec<&str> = child_ids.iter().map(String::as_str).collect();
        backend.add_node(make_test_node(
            "root",
            "Root",
            "application",
            true,
            false,
            false,
            false,
            child_refs,
        ));

        let adapter = UiAdapter::with_backend(Arc::new(backend));

        // Case-insensitive substring "SAVE" with role "button" capped at 10
        let req = ActionRequest::new(
            "ui",
            "find",
            json!({
                "app": "SearchApp",
                "name": "SAVE",
                "role": "button"
            }),
        );
        let res = adapter.execute(req).await;
        assert!(res.success);
        let arr = res.output.as_array().unwrap();
        assert_eq!(arr.len(), 10);
        for item in arr {
            assert_eq!(item["role"], "button");
            assert!(item["name"].as_str().unwrap().contains("Save File"));
        }

        // Search with role "label"
        let req_lbl = ActionRequest::new(
            "ui",
            "find",
            json!({
                "app": "SearchApp",
                "name": "save",
                "role": "label"
            }),
        );
        let res_lbl = adapter.execute(req_lbl).await;
        assert!(res_lbl.success);
        let arr_lbl = res_lbl.output.as_array().unwrap();
        assert_eq!(arr_lbl.len(), 5);
        for item in arr_lbl {
            assert_eq!(item["role"], "label");
        }
    }

    #[test]
    fn test_role_matching_by_words() {
        assert!(role_matches("PushButton", "button"));
        assert!(role_matches("PushButton", "push button"));
        assert!(role_matches("push_button", "Push-Button"));
        assert!(role_matches("ToggleButton", "button"));
        assert!(!role_matches("ToggleButton", "push button"));
        assert!(role_matches("Frame", "frame"));
        assert!(role_matches("button", "button"));
        assert!(!role_matches("Panel", "button"));
        assert!(!role_matches("Frame", ""));
        assert!(!role_matches("Buttonish", "button"));
    }

    /// GTK4 and libadwaita bury a window's controls under many unnamed
    /// wrapper panels: a button 20 levels down is found, clicked by name,
    /// and the tree lists it while skipping the wrappers.
    #[tokio::test]
    async fn test_controls_deep_under_unnamed_wrappers_are_found() {
        let mut backend = FakeUiBackend::new();
        backend.add_app("DeepApp", "w0");
        let levels = 20;
        for i in 0..levels {
            let child = if i + 1 < levels {
                format!("w{}", i + 1)
            } else {
                "ok".to_string()
            };
            backend.add_node(make_test_node(
                &format!("w{i}"),
                "",
                "panel",
                true,
                false,
                false,
                false,
                vec![&child],
            ));
        }
        let ok = make_test_node(
            "ok",
            "Open",
            "push button",
            true,
            true,
            false,
            false,
            vec![],
        );
        let invoked = ok.action_invoked.clone();
        backend.add_node(ok);
        let adapter = UiAdapter::with_backend(Arc::new(backend));

        let found = adapter
            .execute(ActionRequest::new(
                "ui",
                "find",
                json!({ "app": "DeepApp", "name": "open", "role": "button" }),
            ))
            .await;
        assert!(found.success, "{:?}", found.error);
        assert_eq!(found.output.as_array().unwrap().len(), 1);

        let tree = adapter
            .execute(ActionRequest::new(
                "ui",
                "tree",
                json!({ "app": "DeepApp" }),
            ))
            .await;
        let listed = tree.output.as_array().unwrap();
        assert_eq!(listed.len(), 1, "wrappers are walked through, not listed");
        assert_eq!(listed[0]["name"], "Open");

        let clicked = adapter
            .execute(ActionRequest::new(
                "ui",
                "click",
                json!({ "app": "DeepApp", "name": "Open" }),
            ))
            .await;
        assert!(clicked.success, "{:?}", clicked.error);
        assert!(invoked.load(Ordering::SeqCst));
    }

    /// A huge tree is cut off by the visit budget, quickly and without an error.
    #[tokio::test]
    async fn test_a_huge_tree_stops_at_the_visit_budget() {
        let mut backend = FakeUiBackend::new();
        backend.add_app("BigApp", "root");
        let mut kids = Vec::new();
        for i in 0..60 {
            let id = format!("row{i}");
            let leaves: Vec<String> = (0..60).map(|j| format!("leaf{i}_{j}")).collect();
            for l in &leaves {
                backend.add_node(make_test_node(
                    l,
                    "Item",
                    "list item",
                    true,
                    false,
                    false,
                    false,
                    vec![],
                ));
            }
            backend.add_node(make_test_node(
                &id,
                "",
                "panel",
                true,
                false,
                false,
                false,
                leaves.iter().map(String::as_str).collect(),
            ));
            kids.push(id);
        }
        backend.add_node(make_test_node(
            "root",
            "Root",
            "application",
            true,
            false,
            false,
            false,
            kids.iter().map(String::as_str).collect(),
        ));
        let adapter = UiAdapter::with_backend(Arc::new(backend));
        let found = adapter
            .execute(ActionRequest::new(
                "ui",
                "find",
                json!({ "app": "BigApp", "name": "nothing like this" }),
            ))
            .await;
        assert!(found.success, "{:?}", found.error);
        assert!(found.output.as_array().unwrap().is_empty());
        // The budget is per verb: another verb starts fresh.
        let again = adapter
            .execute(ActionRequest::new(
                "ui",
                "find",
                json!({ "app": "BigApp", "name": "item" }),
            ))
            .await;
        assert_eq!(again.output.as_array().unwrap().len(), 10);
    }

    /// An element that is on screen but offers nothing to press (as GTK4's
    /// buttons) gets its own error, not "no such element".
    #[tokio::test]
    async fn test_on_screen_but_not_pressable_has_its_own_error() {
        let mut backend = FakeUiBackend::new();
        backend.add_app("Gtk4App", "root");
        backend.add_node(make_test_node(
            "b",
            "Back",
            "push button",
            true,
            false,
            false,
            false,
            vec![],
        ));
        backend.add_node(make_test_node(
            "root",
            "Root",
            "application",
            true,
            false,
            false,
            false,
            vec!["b"],
        ));
        let adapter = UiAdapter::with_backend(Arc::new(backend));
        let res = adapter
            .execute(ActionRequest::new(
                "ui",
                "click",
                json!({ "app": "Gtk4App", "name": "Back" }),
            ))
            .await;
        assert!(!res.success);
        assert!(
            res.error
                .as_ref()
                .unwrap()
                .contains("offers no way to press it"),
            "{:?}",
            res.error
        );
        let missing = adapter
            .execute(ActionRequest::new(
                "ui",
                "click",
                json!({ "app": "Gtk4App", "name": "Nonexistent" }),
            ))
            .await;
        assert!(missing.error.unwrap().contains("no clickable element"));
    }

    #[tokio::test]
    async fn test_click_by_path_and_by_name() {
        let mut backend = FakeUiBackend::new();
        backend.add_app("ClickApp", "root");

        let b1 = make_test_node("b1", "Submit", "button", true, true, false, false, vec![]);
        let b1_flag = b1.action_invoked.clone();
        let b2 = make_test_node("b2", "Cancel", "button", true, true, false, false, vec![]);
        let b2_flag = b2.action_invoked.clone();

        backend.add_node(b1);
        backend.add_node(b2);
        backend.add_node(make_test_node(
            "root",
            "Root",
            "application",
            true,
            false,
            false,
            false,
            vec!["b1", "b2"],
        ));

        let adapter = UiAdapter::with_backend(Arc::new(backend));

        // Click by name: "Submit" (dangerous label -> upgraded to destructive risk)
        let resolved = adapter.resolve_click("ClickApp", "Submit").await.unwrap();
        assert_eq!(resolved.path, "0");
        assert_eq!(resolved.label, "Submit");
        assert!(label_is_dangerous(&resolved.label));

        // Not pressed on its own say-so.
        let req = ActionRequest::new(
            "ui",
            "click",
            json!({ "app": "ClickApp", "name": "Submit" }),
        );
        let res = adapter.execute(req).await;
        assert!(!res.success);
        assert!(res.error.unwrap().contains("needs confirmation first"));
        assert!(
            !b1_flag.load(Ordering::SeqCst),
            "an unconfirmed Submit was pressed"
        );
        assert_eq!(res.metadata["risk"], "destructive");

        // Pressed once the caller has confirmed it with the person.
        let req = ActionRequest::new(
            "ui",
            "click",
            json!({ "app": "ClickApp", "name": "Submit", "confirmed": true }),
        );
        let res = adapter.execute(req).await;
        assert!(res.success);
        assert!(b1_flag.load(Ordering::SeqCst));
        assert!(!b2_flag.load(Ordering::SeqCst));
        assert_eq!(res.metadata["risk"], "destructive");

        // "confirmed" must be the boolean true, not a truthy string.
        b1_flag.store(false, Ordering::SeqCst);
        let res = adapter
            .execute(ActionRequest::new(
                "ui",
                "click",
                json!({ "app": "ClickApp", "name": "Submit", "confirmed": "yes" }),
            ))
            .await;
        assert!(!res.success);
        assert!(!b1_flag.load(Ordering::SeqCst));

        // Click by path: "1" ("Cancel" is reversible risk)
        let req_path = ActionRequest::new("ui", "click", json!({ "app": "ClickApp", "path": "1" }));
        let res_path = adapter.execute(req_path).await;
        assert!(res_path.success);
        assert!(b2_flag.load(Ordering::SeqCst));
        assert_eq!(res_path.metadata["risk"], "reversible");
    }

    #[tokio::test]
    async fn test_ambiguous_click_lists_candidates_and_clicks_nothing() {
        let mut backend = FakeUiBackend::new();
        backend.add_app("AmbApp", "root");

        let b1 = make_test_node("b1", "Close", "button", true, true, false, false, vec![]);
        let b1_flag = b1.action_invoked.clone();
        let b2 = make_test_node("b2", "Close", "button", true, true, false, false, vec![]);
        let b2_flag = b2.action_invoked.clone();

        backend.add_node(b1);
        backend.add_node(b2);
        backend.add_node(make_test_node(
            "root",
            "Root",
            "application",
            true,
            false,
            false,
            false,
            vec!["b1", "b2"],
        ));

        let adapter = UiAdapter::with_backend(Arc::new(backend));

        let req = ActionRequest::new("ui", "click", json!({ "app": "AmbApp", "name": "Close" }));
        let res = adapter.execute(req).await;
        assert!(!res.success);
        let err = res.error.unwrap();
        assert!(err.contains("ambiguous click target \"Close\""));
        assert!(err.contains("matches 2 elements"));
        assert!(err.contains("0"));
        assert!(err.contains("1"));

        // Clicks nothing!
        assert!(!b1_flag.load(Ordering::SeqCst));
        assert!(!b2_flag.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_click_no_match_error() {
        let mut backend = FakeUiBackend::new();
        backend.add_app("NoMatchApp", "root");
        backend.add_node(make_test_node(
            "root",
            "Root",
            "application",
            true,
            false,
            false,
            false,
            vec![],
        ));

        let adapter = UiAdapter::with_backend(Arc::new(backend));

        let req = ActionRequest::new(
            "ui",
            "click",
            json!({ "app": "NoMatchApp", "name": "NonExistent" }),
        );
        let res = adapter.execute(req).await;
        assert!(!res.success);
        assert!(res
            .error
            .unwrap()
            .contains("no clickable element in \"NoMatchApp\" matches \"NonExistent\""));

        let req_path =
            ActionRequest::new("ui", "click", json!({ "app": "NoMatchApp", "path": "5/2" }));
        let res_path = adapter.execute(req_path).await;
        assert!(!res_path.success);
        assert!(res_path.error.unwrap().contains("out of bounds"));
    }

    #[tokio::test]
    async fn test_password_fields_refused_for_read_type_text_tree_text() {
        let mut backend = FakeUiBackend::new();
        backend.add_app("PwdApp", "root");

        let mut pwd = make_test_node(
            "pwd",
            "SecretPassword",
            "PasswordText",
            true,
            false,
            true,
            true,
            vec![],
        );
        pwd.text = "super_secret_123".to_string();
        backend.add_node(pwd);
        backend.add_node(make_test_node(
            "root",
            "Root",
            "application",
            true,
            false,
            false,
            false,
            vec!["pwd"],
        ));

        let adapter = UiAdapter::with_backend(Arc::new(backend));

        // Read refused
        let req_read = ActionRequest::new("ui", "read", json!({ "app": "PwdApp", "path": "0" }));
        let res_read = adapter.execute(req_read).await;
        assert!(!res_read.success);
        assert!(res_read.error.unwrap().contains("password field"));

        // Type text refused
        let req_type = ActionRequest::new(
            "ui",
            "type_text",
            json!({
                "app": "PwdApp",
                "path": "0",
                "text": "new_secret"
            }),
        );
        let res_type = adapter.execute(req_type).await;
        assert!(!res_type.success);
        assert!(res_type.error.unwrap().contains("password field"));

        // Tree does not return password field
        let req_tree = ActionRequest::new("ui", "tree", json!({ "app": "PwdApp" }));
        let res_tree = adapter.execute(req_tree).await;
        assert!(res_tree.success);
        let arr = res_tree.output.as_array().unwrap();
        assert_eq!(arr.len(), 0);

        // Find does not return password field
        let req_find =
            ActionRequest::new("ui", "find", json!({ "app": "PwdApp", "name": "Secret" }));
        let res_find = adapter.execute(req_find).await;
        assert!(res_find.success);
        let find_arr = res_find.output.as_array().unwrap();
        assert_eq!(find_arr.len(), 0);
    }

    #[tokio::test]
    async fn test_text_length_cap_and_read_truncation() {
        let mut backend = FakeUiBackend::new();
        backend.add_app("TextApp", "root");

        let mut node = make_test_node("ed1", "Editor", "entry", true, false, true, false, vec![]);
        node.text = "A".repeat(3000);
        backend.add_node(node);
        backend.add_node(make_test_node(
            "root",
            "Root",
            "application",
            true,
            false,
            false,
            false,
            vec!["ed1"],
        ));

        let adapter = UiAdapter::with_backend(Arc::new(backend));

        // Read truncates to at most 2000 chars
        let req_read = ActionRequest::new("ui", "read", json!({ "app": "TextApp", "path": "0" }));
        let res_read = adapter.execute(req_read).await;
        assert!(res_read.success);
        let text = res_read.output["text"].as_str().unwrap();
        assert_eq!(text.len(), 2000);

        // Type text <= 2000 chars succeeds
        let valid_text = "B".repeat(2000);
        let req_type = ActionRequest::new(
            "ui",
            "type_text",
            json!({
                "app": "TextApp",
                "path": "0",
                "text": valid_text
            }),
        );
        let res_type = adapter.execute(req_type).await;
        assert!(res_type.success);
        assert_eq!(res_type.output["typed"], 2000);

        // Type text > 2000 chars is rejected
        let invalid_text = "C".repeat(2001);
        let req_type_err = ActionRequest::new(
            "ui",
            "type_text",
            json!({
                "app": "TextApp",
                "path": "0",
                "text": invalid_text
            }),
        );
        let res_type_err = adapter.execute(req_type_err).await;
        assert!(!res_type_err.success);
        assert!(res_type_err.error.unwrap().contains("2000 characters"));
    }

    #[tokio::test]
    async fn test_unknown_verb_refused() {
        let backend = FakeUiBackend::new();
        let adapter = UiAdapter::with_backend(Arc::new(backend));

        let req = ActionRequest::new("ui", "explode", json!({}));
        let res = adapter.execute(req).await;
        assert!(!res.success);
        assert!(res
            .error
            .unwrap()
            .contains("the ui adapter has no \"explode\""));
    }
}
