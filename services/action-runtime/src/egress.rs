//! Destination egress policy — the central gate at the kernel boundary.
//!
//! This answers a different question from the permission engine (what FRIDAY
//! *does*) and from [`friday_intelligence::EgressGate`] (what secret leaves):
//!
//! > *"does this specific destination fall within the agent's egress
//! > authority?"*
//!
//! ```text
//! ActionRequest
//!      ↓
//! SecurityScope   ("what authority does this agent have?")
//!      ↓
//! EgressPolicy::check(destination)
//!      ↓
//!   Allow ──→ adapter
//!   Deny  ──→ audited, never dispatched
//! ```
//!
//! The authority ([`SecurityScope`]) and the policy are deliberately separate
//! concepts: a scope states what an agent may reach, the policy decides
//! whether a concrete destination falls inside that authority.
//!
//! The policy is **fail-closed**: a destination it cannot confidently place
//! inside the scope's egress list is denied. SSRF hardening is unconditional —
//! loopback, RFC1918, link-local, and IPv6 local/private destinations are
//! denied regardless of whether a scope is attached.
//!
//! **Off until attached (for authority).** Scope-based classification only
//! engages when a [`SecurityScope`] is carried on the [`ExecutionContext`]; an
//! external destination without a scope keeps the legacy behavior (the
//! adapters' own scheme guards still apply). [`EgressPolicy::with_deny_without_scope`]
//! hardens the default to deny even unscoped external egress.
//!
//! The policy is **pure**: no DNS resolution and no I/O, so a decision is
//! deterministic and never leaks a destination (or anything embedded in it)
//! into audit or error strings.
//!
//! **Three enforcement seams consume this policy.** The kernel's pre-dispatch
//! gate (`AgentKernel::run_action`) classifies a request's destination(s)
//! before any adapter runs; the [`crate::adapters::WebAdapter`] additionally
//! re-checks every redirect hop via [`EgressPolicy::check_url`] *before* the
//! next request is sent, so a 302 that lands on an internal network is stopped
//! (see the egress-redirect acceptance slice); and the
//! [`crate::adapters::BrowserAdapter`] arms CDP `Fetch` interception on every
//! page so a redirect hop followed *inside* Chrome is classified by the same
//! policy before the browser contacts it (see the browser-redirect boundary
//! slice, #12-E). In all three seams the initial URL stays the kernel's
//! pre-dispatch concern; only destinations the kernel never sees are
//! re-classified by the adapters.

use crate::request::ActionRequest;
use friday_agent_runtime::SecurityScope;
use url::{Host, Url};

/// Why a destination was denied. Labels only — a reason must never carry the
/// destination itself (it may embed secrets), so it is never echoed verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EgressDenyReason {
    /// The destination is not a parseable http(s) URL.
    InvalidDestination,
    /// The destination uses a scheme the adapters cannot honor (e.g. `file`).
    UnsupportedUrlScheme,
    /// Loopback / RFC1918 / link-local / IPv6 local — denied unconditionally.
    InternalDestination,
    /// No [`SecurityScope`] was attached and the policy is set to fail closed.
    NoScopeAttached,
    /// The scope's egress list does not confidently cover this destination.
    NotCoveredByScope,
}

impl EgressDenyReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            EgressDenyReason::InvalidDestination => "invalid_destination",
            EgressDenyReason::UnsupportedUrlScheme => "unsupported_url_scheme",
            EgressDenyReason::InternalDestination => "internal_destination",
            EgressDenyReason::NoScopeAttached => "no_scope_attached",
            EgressDenyReason::NotCoveredByScope => "destination_not_covered_by_scope",
        }
    }

    /// Reverse lookup from a classifier label (the only string an adapter may
    /// report as the reason for a denied redirect hop). Returns `None` for
    /// anything that is not a known label, so a misbehaving adapter can never
    /// smuggle a destination into an audit trail through this channel.
    pub fn parse(label: &str) -> Option<Self> {
        match label {
            "invalid_destination" => Some(Self::InvalidDestination),
            "unsupported_url_scheme" => Some(Self::UnsupportedUrlScheme),
            "internal_destination" => Some(Self::InternalDestination),
            "no_scope_attached" => Some(Self::NoScopeAttached),
            "destination_not_covered_by_scope" => Some(Self::NotCoveredByScope),
            _ => None,
        }
    }
}

/// The policy's verdict for one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EgressDecision {
    Allow,
    Deny { reason: EgressDenyReason },
}

impl EgressDecision {
    pub fn is_allow(&self) -> bool {
        matches!(self, EgressDecision::Allow)
    }
}

/// Destination-oriented egress policy. Pure and deterministic.
#[derive(Debug, Clone)]
pub struct EgressPolicy {
    /// Deny all egress when no [`SecurityScope`] is attached, instead of
    /// preserving the legacy unscoped allow. Off by default so existing
    /// deployments are unchanged until they attach a scope.
    deny_without_scope: bool,
}

impl Default for EgressPolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl EgressPolicy {
    pub fn new() -> Self {
        Self {
            deny_without_scope: false,
        }
    }

    /// Hardened mode: refuse external egress for requests that carry no
    /// [`SecurityScope`] authority (fail closed for the whole category).
    /// Internal/private destinations are denied regardless of this flag.
    pub fn with_deny_without_scope(mut self, deny: bool) -> Self {
        self.deny_without_scope = deny;
        self
    }

    /// The single decision point: classify every destination a request would
    /// reach and require each one to fall inside the attached authority.
    ///
    /// - No destination to classify (a non-egress action, or one missing its
    ///   URL) → [`EgressDecision::Allow`]: there is nothing to egress, and the
    ///   adapter still refuses malformed requests itself.
    /// - Any classified destination that cannot be confidently allowed → deny.
    pub fn check(&self, scope: Option<&SecurityScope>, req: &ActionRequest) -> EgressDecision {
        let destinations = destination_candidates(req);
        if destinations.is_empty() {
            return EgressDecision::Allow;
        }
        for dest in destinations {
            if !self.classify(scope, &dest).is_allow() {
                return self.classify(scope, &dest);
            }
        }
        EgressDecision::Allow
    }

    /// Classify one concrete destination URL (a redirect hop, an evidence
    /// target, ...) against the attached authority. The same decision rules as
    /// [`EgressPolicy::check`], reused so classification never forks.
    pub fn check_url(&self, scope: Option<&SecurityScope>, dest: &str) -> EgressDecision {
        self.classify(scope, dest)
    }

    fn classify(&self, scope: Option<&SecurityScope>, dest: &str) -> EgressDecision {
        let deny = |reason| EgressDecision::Deny { reason };
        let url = match Url::parse(dest) {
            Ok(u) => u,
            Err(_) => return deny(EgressDenyReason::InvalidDestination),
        };
        if !matches!(url.scheme(), "http" | "https") {
            return deny(EgressDenyReason::UnsupportedUrlScheme);
        }
        let host = match url.host() {
            Some(h) => h,
            None => return deny(EgressDenyReason::InvalidDestination),
        };
        // Unconditional SSRF hardening — no scope can grant internal access.
        if is_internal_host(&host) {
            return deny(EgressDenyReason::InternalDestination);
        }
        let scope = match scope {
            Some(s) => s,
            None => {
                if self.deny_without_scope {
                    return deny(EgressDenyReason::NoScopeAttached);
                }
                return EgressDecision::Allow;
            }
        };
        if !scope_covers(scope, url.scheme(), &host) {
            return deny(EgressDenyReason::NotCoveredByScope);
        }
        EgressDecision::Allow
    }
}

/// Which tools/actions carry an egress destination, and where it lives in the
/// request parameters. Anything not listed here performs no network egress and
/// is not classified.
fn destination_candidates(req: &ActionRequest) -> Vec<String> {
    let url_param = || {
        req.parameters
            .get("url")
            .and_then(|v| v.as_str())
            .map(str::to_string)
    };
    match (req.tool.as_str(), req.action.as_str()) {
        // web.fetch (WebAdapter) and web.open (BrowserAdapter navigate) reach
        // exactly the URL in `url`.
        ("web", "fetch") | ("web", "open") => url_param().into_iter().collect(),
        // web.search reaches its `endpoint` param, or the adapter's default
        // search endpoint when the caller did not override it.
        ("web", "search") => vec![req
            .parameters
            .get("endpoint")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| crate::adapters::web::DEFAULT_SEARCH_ENDPOINT.to_string())],
        _ => Vec::new(),
    }
}

/// Unconditional internal-network denial: loopback, unspecified, link-local,
/// CGNAT, RFC1918, and IPv6 loopback/ULA/link-local. Never resolves DNS, so a
/// public-looking hostname that happens to resolve internally is not caught
/// here (documented gap — same as the adapters' own scheme-only guards).
fn is_internal_host(host: &Host<&str>) -> bool {
    match host {
        Host::Ipv4(ip) => is_private_ipv4(*ip),
        Host::Ipv6(ip) => {
            if ip.is_loopback() || ip.is_unspecified() {
                return true;
            }
            if let Some(v4) = ip.to_ipv4() {
                return is_private_ipv4(v4);
            }
            let seg = ip.segments();
            // fc00::/7 unique-local.
            if seg[0] & 0xfe00 == 0xfc00 {
                return true;
            }
            // fe80::/10 link-local.
            seg[0] & 0xffc0 == 0xfe80
        }
        Host::Domain(domain) => {
            let lower = domain.to_ascii_lowercase();
            lower == "localhost" || lower.ends_with(".localhost")
        }
    }
}
/// An address on this computer or the local network ([`is_internal_host`]).
pub fn ip_is_internal(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => is_internal_host(&Host::Ipv4(v4)),
        std::net::IpAddr::V6(v6) => is_internal_host(&Host::Ipv6(v6)),
    }
}

fn is_private_ipv4(ip: std::net::Ipv4Addr) -> bool {
    let o = ip.octets();
    // 0.0.0.0/8 — "this host".
    if o[0] == 0 {
        return true;
    }
    // 10.0.0.0/8.
    if o[0] == 10 {
        return true;
    }
    // 100.64.0.0/10 — shared address space (CGNAT).
    if o[0] == 100 && (o[1] & 0xc0) == 0x40 {
        return true;
    }
    // 127.0.0.0/8 — loopback.
    if o[0] == 127 {
        return true;
    }
    // 169.254.0.0/16 — link-local.
    if o[0] == 169 && o[1] == 254 {
        return true;
    }
    // 172.16.0.0/12.
    if o[0] == 172 && (o[1] & 0xf0) == 16 {
        return true;
    }
    // 192.168.0.0/16.
    o[0] == 192 && o[1] == 168
}

/// Does the scope's egress list confidently cover this (scheme, host)?
///
/// An entry is either:
/// - a plain hostname (`example.com`) — covers the host and its subdomains;
/// - a scheme-qualified origin (`https://example.com`) — additionally pins the
///   scheme;
/// - a wildcard hostname (`*.example.com`) — covers subdomains only, never the
///   apex.
///
/// Anything else — a hostname or literal the list does not name — is not
/// confidently inside the authority and fails closed.
fn scope_covers(scope: &SecurityScope, scheme: &str, host: &Host<&str>) -> bool {
    if scope.egress.is_empty() {
        return false;
    }
    let host_name = match host {
        Host::Domain(d) => d.to_ascii_lowercase(),
        // A scope may name a public IP literal directly.
        Host::Ipv4(ip) => ip.to_string(),
        Host::Ipv6(ip) => ip.to_string(),
    };
    scope
        .egress
        .iter()
        .any(|entry| entry_covers(entry, scheme, &host_name))
}

fn entry_covers(entry: &str, scheme: &str, host_name: &str) -> bool {
    let entry = entry.trim();
    let (entry_scheme, entry_host) = match entry.split_once("://") {
        Some((s, h)) => (
            Some(s.to_ascii_lowercase()),
            h.trim_end_matches('/').to_ascii_lowercase(),
        ),
        None => (None, entry.trim_end_matches('/').to_ascii_lowercase()),
    };
    if let Some(es) = entry_scheme {
        if es != scheme.to_ascii_lowercase() {
            return false;
        }
    }
    match entry_host.strip_prefix("*.") {
        Some(suffix) => {
            host_name.len() > suffix.len() + 1 && host_name.ends_with(&format!(".{suffix}"))
        }
        None => host_name == entry_host || host_name.ends_with(&format!(".{entry_host}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn policy() -> EgressPolicy {
        EgressPolicy::new()
    }

    fn scope(egress: &[&str]) -> SecurityScope {
        SecurityScope::new(egress, &[])
    }

    fn fetch(url: &str) -> ActionRequest {
        ActionRequest::new("web", "fetch", json!({ "url": url }))
    }

    fn search(endpoint: Option<&str>) -> ActionRequest {
        let params = match endpoint {
            Some(e) => json!({ "endpoint": e }),
            None => json!({}),
        };
        ActionRequest::new("web", "search", params)
    }

    #[test]
    fn allowed_https_destination_passes() {
        let p = policy();
        assert!(p
            .check(
                Some(&scope(&["example.com"])),
                &fetch("https://example.com/docs")
            )
            .is_allow());
        // Subdomains are covered by the apex entry.
        assert!(p
            .check(
                Some(&scope(&["example.com"])),
                &fetch("https://api.example.com/v1")
            )
            .is_allow());
    }

    #[test]
    fn scheme_qualified_grant_pins_the_scheme() {
        let p = policy();
        let s = scope(&["https://example.com"]);
        assert!(p
            .check(Some(&s), &fetch("https://example.com/x"))
            .is_allow());
        assert!(
            !p.check(Some(&s), &fetch("http://example.com/x")).is_allow(),
            "http is not covered by an https grant"
        );
    }

    #[test]
    fn wildcard_covers_subdomains_not_the_apex() {
        let p = policy();
        let s = scope(&["*.example.com"]);
        assert!(p
            .check(Some(&s), &fetch("https://api.example.com"))
            .is_allow());
        assert!(
            !p.check(Some(&s), &fetch("https://example.com")).is_allow(),
            "a wildcard grant never covers the apex"
        );
    }

    #[test]
    fn out_of_scope_destination_is_denied() {
        let p = policy();
        let s = scope(&["example.com"]);
        assert!(!p
            .check(Some(&s), &fetch("https://other-site.com/path"))
            .is_allow());
        // A similar-looking host is not confidently covered either.
        assert!(!p
            .check(Some(&s), &fetch("https://notexample.com"))
            .is_allow());
    }

    #[test]
    fn empty_scope_means_explicitly_denied_authority() {
        let p = policy();
        assert!(!p
            .check(Some(&scope(&[])), &fetch("https://example.com"))
            .is_allow());
    }

    #[test]
    fn internal_destinations_are_denied_unconditionally() {
        let p = policy();
        // Even a scope that explicitly grants the internal literal cannot open it.
        for (url, grants) in [
            ("http://localhost/x", vec!["localhost"]),
            ("http://127.0.0.1/x", vec!["127.0.0.1"]),
            ("http://192.168.1.5/x", vec!["192.168.1.5", "example.com"]),
            ("http://10.0.0.1/x", vec!["10.0.0.1", "example.com"]),
            ("http://172.16.4.2/x", vec!["172.16.4.2", "example.com"]),
            (
                "http://169.254.169.254/latest/meta-data",
                vec!["169.254.169.254"],
            ),
            ("http://[::1]/x", vec!["[::1]", "example.com"]),
            ("http://[fc00::1]/x", vec!["fc00::1"]),
            ("http://[fe80::1]/x", vec!["fe80::1"]),
        ] {
            let s = scope(&grants.to_vec());
            let d = p.check(Some(&s), &fetch(url));
            assert_eq!(
                d,
                EgressDecision::Deny {
                    reason: EgressDenyReason::InternalDestination
                },
                "internal URL not denied: {url}"
            );
        }
    }

    #[test]
    fn non_http_schemes_are_denied() {
        let p = policy();
        assert_eq!(
            p.check(Some(&scope(&["example.com"])), &fetch("file:///etc/passwd")),
            EgressDecision::Deny {
                reason: EgressDenyReason::UnsupportedUrlScheme
            }
        );
    }

    #[test]
    fn unparseable_destination_is_denied_fail_closed() {
        let p = policy();
        assert_eq!(
            p.check(
                Some(&scope(&["example.com"])),
                &fetch("definitely not a url")
            ),
            EgressDecision::Deny {
                reason: EgressDenyReason::InvalidDestination
            }
        );
    }

    #[test]
    fn no_scope_preserves_legacy_behavior_until_hardened() {
        let p = policy();
        // Off until a scope is attached: external destinations keep flowing.
        assert!(p.check(None, &fetch("https://example.com")).is_allow());
        // ...but the unconditional SSRF gate still blocks internal networks.
        assert!(
            !p.check(None, &fetch("http://127.0.0.1")).is_allow(),
            "internal destinations are denied even without a scope"
        );
        // Hardened mode fails closed for external destinations too.
        let hard = policy().with_deny_without_scope(true);
        assert!(!hard.check(None, &fetch("https://example.com")).is_allow());
    }

    #[test]
    fn non_egress_actions_are_not_classified() {
        let p = policy();
        let fs = ActionRequest::new("filesystem", "read_file", json!({ "path": "/etc/hosts" }));
        assert!(p.check(Some(&scope(&[])), &fs).is_allow());
        // web.fetch without a url is nothing to egress — the adapter refuses it.
        let no_url = ActionRequest::new("web", "fetch", json!({}));
        assert!(p.check(Some(&scope(&["example.com"])), &no_url).is_allow());
    }

    #[test]
    fn search_checks_the_endpoint_param_or_default() {
        let p = policy();
        // An overridden endpoint is classified against it.
        let s = scope(&["api.example.com"]);
        assert!(p
            .check(Some(&s), &search(Some("https://api.example.com/search")))
            .is_allow());
        assert!(!p
            .check(Some(&s), &search(Some("https://other.example.com/search")))
            .is_allow());
        // Without an override the adapter's default endpoint is the destination.
        let duck = scope(&["html.duckduckgo.com"]);
        assert!(
            p.check(Some(&duck), &search(None)).is_allow(),
            "default search endpoint must be covered by the default host grant"
        );
    }
}
