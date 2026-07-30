use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::contract::{Capability, CapabilityRequest, CapabilityResult, CorrelationId, ExecutionTelemetry, Plugin, PluginDescriptor, PluginHealth, PluginStatus, ProviderProfile};
use crate::utils;

/// Filesystem Plugin — reads, writes, and lists the local filesystem.
///
/// Supports: ReadFile, WriteFile, ListDirectory
pub struct FilesystemPlugin {
    descriptor: PluginDescriptor,
}

impl Default for FilesystemPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl FilesystemPlugin {
    pub fn new() -> Self {
        Self {
            descriptor: PluginDescriptor {
                id: "filesystem".into(),
                name: "Filesystem Plugin".into(),
                version: "1.0.0".into(),
                author: "FRIDAY Core".into(),
                capabilities: vec![
                    Capability::ReadFile,
                    Capability::WriteFile,
                    Capability::ListDirectory,
                ],
                permissions: vec!["fs.read".into(), "fs.write".into()],
                profile: ProviderProfile {
                    plugin_id: "filesystem".into(),
                    deterministic: true,
                    offline_capable: true,
                    streaming: false,
                    has_side_effects: true,
                    requires_network: false,
                    max_concurrency: 4,
                    preferred_contexts: vec!["local".into(), "filesystem".into()],
                },
            },
        }
    }
}

impl Plugin for FilesystemPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        self.descriptor.clone()
    }

    fn initialize(&mut self) {}

    fn execute(&self, request: CapabilityRequest) -> CapabilityResult {
        let started_at_ms = utils::now_ms();
        let cid = request.correlation_id.clone();

        match &request.capability {
            Capability::ReadFile => self.execute_read(request, started_at_ms, cid),
            Capability::WriteFile => self.execute_write(request, started_at_ms, cid),
            Capability::ListDirectory => self.execute_list(request, started_at_ms, cid),
            other => {
                let mut metadata = HashMap::new();
                metadata.insert("error".into(), format!("unsupported capability: {}", other.as_str()));
                CapabilityResult {
                    success: false,
                    output: String::new(),
                    metadata,
                    telemetry: ExecutionTelemetry {
                        started_at_ms,
                        finished_at_ms: utils::now_ms(),
                        plugin_id: self.descriptor.id.clone(),
                        success: false,
                        correlation_id: cid,
                    },
                }
            }
        }
    }

    fn health(&self) -> PluginHealth {
        PluginHealth {
            plugin_id: self.descriptor.id.clone(),
            status: PluginStatus::Healthy,
            availability: 1.0,
            reliability: 1.0,
            average_latency_ms: 0.0,
            success_rate: 1.0,
            total_executions: 0,
            failure_count: 0,
            last_error: None,
            last_execution_ms: None,
        }
    }

    fn shutdown(&mut self) {}
}

impl FilesystemPlugin {
    fn execute_read(&self, request: CapabilityRequest, started_at_ms: u64, cid: CorrelationId) -> CapabilityResult {
        let path = request.parameter("path").cloned().unwrap_or_default();
        let mut metadata = HashMap::new();

        if path.is_empty() {
            metadata.insert("error".into(), "path parameter is required".into());
            metadata.insert("parameter".into(), "path".into());
            return CapabilityResult {
                success: false,
                output: String::new(),
                metadata,
                telemetry: ExecutionTelemetry {
                    started_at_ms,
                    finished_at_ms: utils::now_ms(),
                    plugin_id: self.descriptor.id.clone(),
                    success: false,
                    correlation_id: cid,
                },
            };
        }

        metadata.insert("path".into(), path.clone());

        match fs::read_to_string(&path) {
            Ok(content) => {
                let line_count = content.lines().count();
                let byte_count = content.len();
                metadata.insert("line_count".into(), line_count.to_string());
                metadata.insert("byte_count".into(), byte_count.to_string());

                CapabilityResult {
                    success: true,
                    output: content,
                    metadata,
                    telemetry: ExecutionTelemetry {
                        started_at_ms,
                        finished_at_ms: utils::now_ms(),
                        plugin_id: self.descriptor.id.clone(),
                        success: true,
                        correlation_id: cid,
                    },
                }
            }
            Err(e) => {
                metadata.insert("error".into(), e.to_string());
                CapabilityResult {
                    success: false,
                    output: String::new(),
                    metadata,
                    telemetry: ExecutionTelemetry {
                        started_at_ms,
                        finished_at_ms: utils::now_ms(),
                        plugin_id: self.descriptor.id.clone(),
                        success: false,
                        correlation_id: cid,
                    },
                }
            }
        }
    }

    fn execute_write(&self, request: CapabilityRequest, started_at_ms: u64, cid: CorrelationId) -> CapabilityResult {
        let path = request.parameter("path").cloned().unwrap_or_default();
        let content = request.parameter("content").cloned().unwrap_or_default();
        let mut metadata = HashMap::new();

        if path.is_empty() {
            metadata.insert("error".into(), "path parameter is required".into());
            return CapabilityResult {
                success: false,
                output: String::new(),
                metadata,
                telemetry: ExecutionTelemetry {
                    started_at_ms,
                    finished_at_ms: utils::now_ms(),
                    plugin_id: self.descriptor.id.clone(),
                    success: false,
                    correlation_id: cid,
                },
            };
        }

        let path_obj = Path::new(&path);
        if let Some(parent) = path_obj.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                let _ = fs::create_dir_all(parent);
            }
        }

        metadata.insert("path".into(), path.clone());

        match fs::write(&path, &content) {
            Ok(()) => {
                let byte_count = content.len();
                metadata.insert("byte_count".into(), byte_count.to_string());

                CapabilityResult {
                    success: true,
                    output: format!("wrote {} bytes: {}", byte_count, content),
                    metadata,
                    telemetry: ExecutionTelemetry {
                        started_at_ms,
                        finished_at_ms: utils::now_ms(),
                        plugin_id: self.descriptor.id.clone(),
                        success: true,
                        correlation_id: cid,
                    },
                }
            }
            Err(e) => {
                metadata.insert("error".into(), e.to_string());
                CapabilityResult {
                    success: false,
                    output: String::new(),
                    metadata,
                    telemetry: ExecutionTelemetry {
                        started_at_ms,
                        finished_at_ms: utils::now_ms(),
                        plugin_id: self.descriptor.id.clone(),
                        success: false,
                        correlation_id: cid,
                    },
                }
            }
        }
    }

    fn execute_list(&self, request: CapabilityRequest, started_at_ms: u64, cid: CorrelationId) -> CapabilityResult {
        let path = request.parameter("path").cloned().unwrap_or_else(|| ".".into());
        let mut metadata = HashMap::new();

        metadata.insert("path".into(), path.clone());

        match fs::read_dir(&path) {
            Ok(entries) => {
                let mut items: Vec<String> = entries
                    .filter_map(|e| e.ok())
                    .map(|e| {
                        let entry_path = e.path();
                        let is_dir = entry_path.is_dir();
                        let name = entry_path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        let size = if is_dir {
                            String::new()
                        } else {
                            e.metadata().ok().map(|m| m.len().to_string()).unwrap_or_default()
                        };
                        let kind = if is_dir { "dir" } else { "file" };
                        if size.is_empty() {
                            format!("{} [{}]", name, kind)
                        } else {
                            format!("{} [{}] {} bytes", name, kind, size)
                        }
                    })
                    .collect();
                items.sort();
                let output = items.join("\n");
                let count = items.len();
                metadata.insert("entry_count".into(), count.to_string());

                CapabilityResult {
                    success: true,
                    output,
                    metadata,
                    telemetry: ExecutionTelemetry {
                        started_at_ms,
                        finished_at_ms: utils::now_ms(),
                        plugin_id: self.descriptor.id.clone(),
                        success: true,
                        correlation_id: cid,
                    },
                }
            }
            Err(e) => {
                metadata.insert("error".into(), e.to_string());
                CapabilityResult {
                    success: false,
                    output: String::new(),
                    metadata,
                    telemetry: ExecutionTelemetry {
                        started_at_ms,
                        finished_at_ms: utils::now_ms(),
                        plugin_id: self.descriptor.id.clone(),
                        success: false,
                        correlation_id: cid,
                    },
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn read_file_returns_content() {
        let plugin = FilesystemPlugin::new();
        let path = std::env::temp_dir().join("friday_plugin_test_read.txt");
        let test_content = "hello from filesystem plugin";
        {
            let mut f = fs::File::create(&path).unwrap();
            f.write_all(test_content.as_bytes()).unwrap();
        }

        let request = CapabilityRequest::new(Capability::ReadFile)
            .with_parameter("path", path.to_string_lossy().to_string());
        let result = plugin.execute(request);

        assert!(result.success);
        assert_eq!(result.output, test_content);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn read_file_missing_path_returns_error() {
        let plugin = FilesystemPlugin::new();
        let request = CapabilityRequest::new(Capability::ReadFile);
        let result = plugin.execute(request);
        assert!(!result.success);
        assert!(result.metadata.get("error").unwrap().contains("required"));
    }

    #[test]
    fn read_file_nonexistent_returns_error() {
        let plugin = FilesystemPlugin::new();
        let request = CapabilityRequest::new(Capability::ReadFile)
            .with_parameter("path", "/nonexistent/path/file.txt");
        let result = plugin.execute(request);
        assert!(!result.success);
    }

    #[test]
    fn write_file_creates_content() {
        let plugin = FilesystemPlugin::new();
        let path = std::env::temp_dir().join("friday_plugin_test_write.txt");

        let request = CapabilityRequest::new(Capability::WriteFile)
            .with_parameter("path", path.to_string_lossy().to_string())
            .with_parameter("content", "written by plugin test");
        let result = plugin.execute(request);

        assert!(result.success);
        assert!(result.output.contains("written"));

        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "written by plugin test");

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn list_directory_returns_entries() {
        let plugin = FilesystemPlugin::new();
        let dir = std::env::temp_dir().join("friday_plugin_test_list");
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join("a.txt"), "a").unwrap();
        fs::write(dir.join("b.txt"), "bb").unwrap();

        let request = CapabilityRequest::new(Capability::ListDirectory)
            .with_parameter("path", dir.to_string_lossy().to_string());
        let result = plugin.execute(request);

        assert!(result.success);
        assert!(result.output.contains("a.txt"));
        assert!(result.output.contains("b.txt"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unsupported_capability_returns_error() {
        let plugin = FilesystemPlugin::new();
        let request = CapabilityRequest::new(Capability::OpenBrowser);
        let result = plugin.execute(request);
        assert!(!result.success);
        assert!(result.metadata.get("error").unwrap().contains("unsupported"));
    }
}
