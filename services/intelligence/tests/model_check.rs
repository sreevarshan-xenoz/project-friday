use friday_intelligence::providers::{
    http_client, kilo_free_models, kilo_static_models, opencode_free_models,
    opencode_static_models, KILO_GATEWAY_BASE_URL, OPENCODE_ZEN_BASE_URL,
};
use std::collections::HashSet;
use std::time::{Duration, Instant};

struct ModelTarget {
    provider: &'static str,
    base_url: &'static str,
    model_id: String,
}

#[derive(Debug)]
struct CheckResult {
    provider: &'static str,
    model_id: String,
    reachable: bool,
    status_or_error: String,
    latency_ms: u64,
    contains_pong: bool,
    answered: bool,
    detail: String,
}

#[derive(Debug)]
struct CatalogDrift {
    provider: &'static str,
    catalog_url: String,
    fetched: bool,
    error: Option<String>,
    total_upstream_free: usize,
    missing_from_upstream: Vec<String>,
    upstream_not_builtin: Vec<String>,
    selected_by_refresh: Vec<String>,
}

async fn check_model(client: &reqwest::Client, target: &ModelTarget) -> CheckResult {
    let url = format!("{}/chat/completions", target.base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": target.model_id,
        "messages": [
            {
                "role": "user",
                "content": "Reply with the single word: pong"
            }
        ]
    });

    let mut attempt = 0;
    let mut last_res = None;

    while attempt < 2 {
        attempt += 1;
        let start = Instant::now();
        let res = client
            .post(&url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .json(&body)
            .timeout(Duration::from_secs(20))
            .send()
            .await;
        let elapsed = start.elapsed().as_millis() as u64;

        match res {
            Ok(response) => {
                let status = response.status();
                if status.is_success() {
                    let text = response.text().await.unwrap_or_default();
                    let content = serde_json::from_str::<serde_json::Value>(&text)
                        .ok()
                        .and_then(|j| {
                            j["choices"][0]["message"]["content"]
                                .as_str()
                                .map(str::to_string)
                        })
                        .unwrap_or_else(|| text.clone());
                    let contains_pong = content.to_lowercase().contains("pong");
                    return CheckResult {
                        provider: target.provider,
                        model_id: target.model_id.clone(),
                        reachable: true,
                        status_or_error: format!("HTTP {}", status.as_u16()),
                        latency_ms: elapsed,
                        contains_pong,
                        answered: contains_pong,
                        detail: content.trim().replace('\n', " "),
                    };
                } else {
                    let err_text = response.text().await.unwrap_or_default();
                    let detail = err_text.trim().replace('\n', " ");
                    // If rate-limited (HTTP 429), retrying immediately is counterproductive; return.
                    if status == reqwest::StatusCode::TOO_MANY_REQUESTS || attempt == 2 {
                        return CheckResult {
                            provider: target.provider,
                            model_id: target.model_id.clone(),
                            reachable: true,
                            status_or_error: format!("HTTP {}", status.as_u16()),
                            latency_ms: elapsed,
                            contains_pong: false,
                            answered: false,
                            detail,
                        };
                    }
                    last_res = Some((true, format!("HTTP {}", status.as_u16()), elapsed, detail));
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
            Err(e) => {
                let err_kind = if e.is_timeout() {
                    "timeout".to_string()
                } else if e.is_connect() {
                    "connect error".to_string()
                } else {
                    format!("error: {e}")
                };
                if attempt == 2 {
                    return CheckResult {
                        provider: target.provider,
                        model_id: target.model_id.clone(),
                        reachable: false,
                        status_or_error: err_kind.clone(),
                        latency_ms: elapsed,
                        contains_pong: false,
                        answered: false,
                        detail: err_kind,
                    };
                }
                last_res = Some((false, err_kind.clone(), elapsed, err_kind));
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }

    let (reachable, status_or_error, latency_ms, detail) =
        last_res.unwrap_or((false, "unknown error".into(), 0, String::new()));
    CheckResult {
        provider: target.provider,
        model_id: target.model_id.clone(),
        reachable,
        status_or_error,
        latency_ms,
        contains_pong: false,
        answered: false,
        detail,
    }
}

fn extract_opencode_free_ids(catalog: &serde_json::Value) -> Vec<String> {
    let Some(data) = catalog.get("data").and_then(|d| d.as_array()) else {
        return Vec::new();
    };
    let is_free = |m: &serde_json::Value| {
        let id = m.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let price = |dir: &str| {
            m.pointer(&format!("/cost/text/{dir}/price"))
                .and_then(|v| v.as_f64())
        };
        id.ends_with("-free") || (price("input") == Some(0.0) && price("output") == Some(0.0))
    };
    data.iter()
        .filter(|m| m.get("type").and_then(|v| v.as_str()).unwrap_or("chat") == "chat")
        .filter(|m| is_free(m))
        .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(str::to_string))
        .collect()
}

fn extract_kilo_free_ids(catalog: &serde_json::Value) -> Vec<String> {
    let Some(data) = catalog.get("data").and_then(|d| d.as_array()) else {
        return Vec::new();
    };
    let is_free = |m: &serde_json::Value| {
        let id = m.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if id.is_empty() {
            return false;
        }
        let price_nonzero = |field: &str| {
            m.pointer(&format!("/pricing/{field}")).is_some_and(|v| {
                if let Some(s) = v.as_str() {
                    s.parse::<f64>().map(|f| f > 0.0).unwrap_or(false)
                } else if let Some(n) = v.as_f64() {
                    n > 0.0
                } else {
                    false
                }
            })
        };
        if price_nonzero("prompt") || price_nonzero("completion") {
            return false;
        }
        id.ends_with(":free") && !id.starts_with("stealth/")
    };
    data.iter()
        .filter(|m| m.get("type").and_then(|v| v.as_str()).unwrap_or("chat") == "chat")
        .filter(|m| is_free(m))
        .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(str::to_string))
        .collect()
}

async fn check_opencode_catalog(client: &reqwest::Client) -> CatalogDrift {
    let url = format!("{OPENCODE_ZEN_BASE_URL}/models");
    let builtin: Vec<String> = opencode_static_models().into_iter().map(|m| m.id).collect();
    let res = client
        .get(&url)
        .timeout(Duration::from_secs(15))
        .send()
        .await;

    match res {
        Ok(resp) if resp.status().is_success() => match resp.json::<serde_json::Value>().await {
            Ok(catalog) => {
                let upstream_free = extract_opencode_free_ids(&catalog);
                let selected = opencode_free_models(&catalog)
                    .into_iter()
                    .map(|m| m.id)
                    .collect();
                let builtin_set: HashSet<_> = builtin.iter().cloned().collect();
                let upstream_set: HashSet<_> = upstream_free.iter().cloned().collect();

                let missing: Vec<String> = builtin
                    .iter()
                    .filter(|id| !upstream_set.contains(*id))
                    .cloned()
                    .collect();
                let not_builtin: Vec<String> = upstream_free
                    .iter()
                    .filter(|id| !builtin_set.contains(*id))
                    .cloned()
                    .collect();

                CatalogDrift {
                    provider: "OpenCode Zen",
                    catalog_url: url,
                    fetched: true,
                    error: None,
                    total_upstream_free: upstream_free.len(),
                    missing_from_upstream: missing,
                    upstream_not_builtin: not_builtin,
                    selected_by_refresh: selected,
                }
            }
            Err(e) => CatalogDrift {
                provider: "OpenCode Zen",
                catalog_url: url,
                fetched: false,
                error: Some(format!("Failed to parse catalog JSON: {e}")),
                total_upstream_free: 0,
                missing_from_upstream: Vec::new(),
                upstream_not_builtin: Vec::new(),
                selected_by_refresh: Vec::new(),
            },
        },
        Ok(resp) => CatalogDrift {
            provider: "OpenCode Zen",
            catalog_url: url,
            fetched: false,
            error: Some(format!("HTTP {}", resp.status())),
            total_upstream_free: 0,
            missing_from_upstream: Vec::new(),
            upstream_not_builtin: Vec::new(),
            selected_by_refresh: Vec::new(),
        },
        Err(e) => CatalogDrift {
            provider: "OpenCode Zen",
            catalog_url: url,
            fetched: false,
            error: Some(format!("Request error: {e}")),
            total_upstream_free: 0,
            missing_from_upstream: Vec::new(),
            upstream_not_builtin: Vec::new(),
            selected_by_refresh: Vec::new(),
        },
    }
}

async fn check_kilo_catalog(client: &reqwest::Client) -> CatalogDrift {
    let url = format!("{KILO_GATEWAY_BASE_URL}/models");
    let builtin: Vec<String> = kilo_static_models().into_iter().map(|m| m.id).collect();
    let res = client
        .get(&url)
        .timeout(Duration::from_secs(15))
        .send()
        .await;

    match res {
        Ok(resp) if resp.status().is_success() => match resp.json::<serde_json::Value>().await {
            Ok(catalog) => {
                let upstream_free = extract_kilo_free_ids(&catalog);
                let selected = kilo_free_models(&catalog)
                    .into_iter()
                    .map(|m| m.id)
                    .collect();
                let builtin_set: HashSet<_> = builtin.iter().cloned().collect();
                let upstream_set: HashSet<_> = upstream_free.iter().cloned().collect();

                let missing: Vec<String> = builtin
                    .iter()
                    .filter(|id| !upstream_set.contains(*id))
                    .cloned()
                    .collect();
                let not_builtin: Vec<String> = upstream_free
                    .iter()
                    .filter(|id| !builtin_set.contains(*id))
                    .cloned()
                    .collect();

                CatalogDrift {
                    provider: "Kilo AI Gateway",
                    catalog_url: url,
                    fetched: true,
                    error: None,
                    total_upstream_free: upstream_free.len(),
                    missing_from_upstream: missing,
                    upstream_not_builtin: not_builtin,
                    selected_by_refresh: selected,
                }
            }
            Err(e) => CatalogDrift {
                provider: "Kilo AI Gateway",
                catalog_url: url,
                fetched: false,
                error: Some(format!("Failed to parse catalog JSON: {e}")),
                total_upstream_free: 0,
                missing_from_upstream: Vec::new(),
                upstream_not_builtin: Vec::new(),
                selected_by_refresh: Vec::new(),
            },
        },
        Ok(resp) => CatalogDrift {
            provider: "Kilo AI Gateway",
            catalog_url: url,
            fetched: false,
            error: Some(format!("HTTP {}", resp.status())),
            total_upstream_free: 0,
            missing_from_upstream: Vec::new(),
            upstream_not_builtin: Vec::new(),
            selected_by_refresh: Vec::new(),
        },
        Err(e) => CatalogDrift {
            provider: "Kilo AI Gateway",
            catalog_url: url,
            fetched: false,
            error: Some(format!("Request error: {e}")),
            total_upstream_free: 0,
            missing_from_upstream: Vec::new(),
            upstream_not_builtin: Vec::new(),
            selected_by_refresh: Vec::new(),
        },
    }
}

fn generate_report(
    results: &[CheckResult],
    catalogs: &[CatalogDrift],
    answering_count: usize,
    total_count: usize,
) -> String {
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
    let verdict = if answering_count >= 2 {
        "PASSED"
    } else {
        "FAILED"
    };

    let mut doc = String::new();
    doc.push_str("# Weekly Free AI Models Check\n\n");
    doc.push_str(&format!(
        "**Run Date:** {now}  \n**Status:** **{verdict}** ({answering_count}/{total_count} built-in models answering; threshold >= 2)\n\n"
    ));

    doc.push_str("## Built-in Models Health\n\n");
    doc.push_str("| Provider | Model | Reachable | Status | Latency | Pong Reply | Answering |\n");
    doc.push_str("|---|---|---|---|---|---|---|\n");
    for r in results {
        let reachable_str = if r.reachable { "Yes" } else { "No" };
        let pong_str = if r.contains_pong { "Yes" } else { "No" };
        let answering_str = if r.answered { "**Yes**" } else { "No" };
        doc.push_str(&format!(
            "| {} | `{}` | {} | {} | {} ms | {} | {} |\n",
            r.provider,
            r.model_id,
            reachable_str,
            r.status_or_error,
            r.latency_ms,
            pong_str,
            answering_str
        ));
    }

    doc.push_str("\n### Notes & Error Details\n\n");
    for r in results {
        if !r.answered {
            doc.push_str(&format!(
                "- **{}** (`{}`): status `{}`, latency `{} ms`. Detail: {}\n",
                r.provider,
                r.model_id,
                r.status_or_error,
                r.latency_ms,
                if r.detail.is_empty() {
                    "none"
                } else {
                    &r.detail
                }
            ));
        } else {
            doc.push_str(&format!(
                "- **{}** (`{}`): OK, replied in `{} ms`.\n",
                r.provider, r.model_id, r.latency_ms
            ));
        }
    }

    doc.push_str("\n## Upstream Catalog Drift\n\n");
    for c in catalogs {
        doc.push_str(&format!("### {}\n\n", c.provider));
        doc.push_str(&format!("- **Catalog endpoint:** `{}`\n", c.catalog_url));
        if c.fetched {
            doc.push_str(&format!(
                "- **Live free models upstream:** {} models\n",
                c.total_upstream_free
            ));

            if c.missing_from_upstream.is_empty() {
                doc.push_str(
                    "- **Built-in models missing upstream:** None (all built-in models are listed)\n",
                );
            } else {
                doc.push_str(&format!(
                    "- **Built-in models missing upstream ({}):**\n",
                    c.missing_from_upstream.len()
                ));
                for m in &c.missing_from_upstream {
                    doc.push_str(&format!("  - `{m}`\n"));
                }
            }

            if c.upstream_not_builtin.is_empty() {
                doc.push_str("- **Upstream free models not built in:** None\n");
            } else {
                doc.push_str(&format!(
                    "- **Upstream free models not built in ({}):**\n",
                    c.upstream_not_builtin.len()
                ));
                for m in &c.upstream_not_builtin {
                    doc.push_str(&format!("  - `{m}`\n"));
                }
            }

            if !c.selected_by_refresh.is_empty() {
                doc.push_str(&format!(
                    "- **Dynamic refresh selection:** {}\n",
                    c.selected_by_refresh
                        .iter()
                        .map(|id| format!("`{id}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        } else {
            doc.push_str(&format!(
                "- **Catalog fetch status:** FAILED ({})\n",
                c.error.as_deref().unwrap_or("unknown error")
            ));
        }
        doc.push('\n');
    }

    doc.push_str("## Summary\n\n");
    doc.push_str(&format!(
        "- **Total built-in models checked:** {total_count}\n"
    ));
    doc.push_str(&format!(
        "- **Built-in models answering:** {answering_count} (threshold: >= 2)\n"
    ));
    doc.push_str(&format!("- **Weekly check verdict:** **{verdict}**\n"));

    doc
}

#[tokio::test]
#[ignore]
async fn model_check() {
    let client = http_client();

    let mut targets = Vec::new();
    for m in opencode_static_models() {
        targets.push(ModelTarget {
            provider: "OpenCode",
            base_url: OPENCODE_ZEN_BASE_URL,
            model_id: m.id,
        });
    }
    for m in kilo_static_models() {
        targets.push(ModelTarget {
            provider: "Kilo",
            base_url: KILO_GATEWAY_BASE_URL,
            model_id: m.id,
        });
    }

    let total_count = targets.len();
    let mut results = Vec::new();

    for target in &targets {
        let result = check_model(&client, target).await;
        results.push(result);
    }

    let answering_count = results.iter().filter(|r| r.answered).count();

    // Check catalogs
    let opencode_cat = check_opencode_catalog(&client).await;
    let kilo_cat = check_kilo_catalog(&client).await;
    let catalogs = vec![opencode_cat, kilo_cat];

    let report = generate_report(&results, &catalogs, answering_count, total_count);

    let report_path_str =
        std::env::var("MODEL_CHECK_REPORT").unwrap_or_else(|_| "target/model-check.md".to_string());
    let path = std::path::PathBuf::from(&report_path_str);

    let target_path = if path.is_relative() {
        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            let manifest_path = std::path::Path::new(&manifest_dir);
            if let Some(ws_root) = manifest_path.parent().and_then(|p| p.parent()) {
                ws_root.join(&path)
            } else {
                path
            }
        } else {
            path
        }
    } else {
        path
    };

    if let Some(parent) = target_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&target_path, &report).unwrap_or_else(|e| {
        eprintln!(
            "Warning: failed to write report to {}: {e}",
            target_path.display()
        );
    });

    println!("{report}");

    assert!(
        answering_count >= 2,
        "Weekly model check failed: fewer than two built-in models answered ({answering_count}/{total_count} answered, need >= 2). See report at {report_path_str}"
    );
}
