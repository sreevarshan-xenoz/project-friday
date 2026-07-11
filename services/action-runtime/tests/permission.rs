//! Unit tests for the permission engine's deny-list enforcement.

use friday_actions::permission::{PermissionDecision, PermissionEngine};
use friday_actions::request::ActionRequest;

fn req(tool: &str, action: &str) -> ActionRequest {
    ActionRequest::new(tool, action, serde_json::Value::Null)
}

#[test]
fn denies_forbidden_capabilities() {
    let engine = PermissionEngine::new();
    assert_eq!(
        engine.decide(&req("filesystem", "delete_file")),
        PermissionDecision::Deny
    );
    assert_eq!(
        engine.decide(&req("mail", "send_mail")),
        PermissionDecision::Deny
    );
    assert_eq!(
        engine.decide(&req("system", "format")),
        PermissionDecision::Deny
    );
}

#[test]
fn allows_safe_capabilities_by_default() {
    let engine = PermissionEngine::new();
    assert_eq!(
        engine.decide(&req("filesystem", "create_file")),
        PermissionDecision::AllowOnce
    );
    assert_eq!(
        engine.decide(&req("terminal", "run")),
        PermissionDecision::AllowOnce
    );
}

#[test]
fn custom_deny_substring_is_enforced() {
    let engine = PermissionEngine::new().deny_substring("nuke");
    assert_eq!(
        engine.decide(&req("filesystem", "nuke_drive")),
        PermissionDecision::Deny
    );
}
