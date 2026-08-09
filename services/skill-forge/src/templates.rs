//! Skill Forge templates — the starting manifests `friday create-skill`
//! scaffolds. Every template uses **canonical** capability ids
//! (`terminal.exec`, `filesystem.read_file`, ...) so a freshly created skill
//! validates against the native registry on the first run. Skills are pure
//! data: the template is the manifest plus a generated README, nothing else.

use serde_json::{json, Value};

/// The scaffold kinds `friday create-skill --template <t>` accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateKind {
    Generic,
    Git,
    Docker,
    Minecraft,
    Server,
    Python,
}

impl TemplateKind {
    /// Parse a template name; `None` for unknown names.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "generic" => Some(Self::Generic),
            "git" => Some(Self::Git),
            "docker" => Some(Self::Docker),
            "minecraft" => Some(Self::Minecraft),
            "server" => Some(Self::Server),
            "python" => Some(Self::Python),
            _ => None,
        }
    }

    /// Every known template, for `--help` and tests.
    pub fn all() -> Vec<Self> {
        vec![
            Self::Generic,
            Self::Git,
            Self::Docker,
            Self::Minecraft,
            Self::Server,
            Self::Python,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::Git => "git",
            Self::Docker => "docker",
            Self::Minecraft => "minecraft",
            Self::Server => "server",
            Self::Python => "python",
        }
    }
}

/// Build the scaffolded `skill.json` for `id`/`name` from a template.
pub fn manifest(kind: TemplateKind, id: &str, name: &str) -> Value {
    let mut value = match kind {
        TemplateKind::Generic => json!({
            "id": id,
            "name": name,
            "version": "0.1.0",
            "description": format!("{name} skill — edit this description."),
            "author": "FRIDAY",
            "category": "productivity",
            "icon": "✦",
            "capabilities": ["terminal.exec"],
            "actions": [format!("use {id}")],
            "permissions": [],
            "readonly": false,
            "dependencies": [],
            "platform": "Any",
            "min_core_version": "0.1.0",
            "enabled": true,
            "memory": { "reads": [], "writes": [] }
        }),
        TemplateKind::Git => base(
            id,
            name,
            "developer",
            "⎇",
            &[
                "git.status",
                "git.commit",
                "git.push",
                "git.pull",
                "git.diff",
                "terminal.exec",
            ],
            &["git.commit", "terminal.exec"],
            &[
                "commit changes",
                "check git status",
                "view git diff",
                "push to remote",
                "pull latest changes",
                "create a branch",
            ],
        ),
        TemplateKind::Docker => base(
            id,
            name,
            "developer",
            "🐳",
            &[
                "terminal.exec",
                "filesystem.read_file",
                "filesystem.write_file",
            ],
            &["terminal.exec", "filesystem.write_file"],
            &[
                "build docker image",
                "docker compose up",
                "list docker containers",
                "view docker logs",
                "prune docker images",
            ],
        ),
        TemplateKind::Minecraft => base(
            id,
            name,
            "gaming",
            "◼",
            &[
                "terminal.exec",
                "filesystem.read_file",
                "filesystem.write_file",
            ],
            &["terminal.exec", "filesystem.write_file"],
            &[
                "start minecraft server",
                "stop minecraft server",
                "check server status",
                "list online players",
                "backup world",
                "install mod",
            ],
        ),
        TemplateKind::Server => base(
            id,
            name,
            "developer",
            "▶",
            &[
                "terminal.exec",
                "filesystem.read_file",
                "filesystem.write_file",
            ],
            &["terminal.exec", "filesystem.write_file"],
            &[
                "check server health",
                "deploy to server",
                "restart service on server",
                "view server logs",
                "check disk usage",
            ],
        ),
        TemplateKind::Python => base(
            id,
            name,
            "developer",
            "🐍",
            &[
                "terminal.exec",
                "filesystem.read_file",
                "filesystem.write_file",
            ],
            &["terminal.exec", "filesystem.write_file"],
            &[
                "run python script",
                "install python package",
                "check python version",
            ],
        ),
    };

    // The Minecraft template is the M8 demo: it declares its memory keys.
    if kind == TemplateKind::Minecraft {
        value["memory"] = json!({
            "reads": ["minecraft.server_state"],
            "writes": ["minecraft.server_state"]
        });
    }
    value
}

/// Shared scaffold builder: category/icon/capabilities/permissions/actions.
fn base(
    id: &str,
    name: &str,
    category: &str,
    icon: &str,
    capabilities: &[&str],
    permissions: &[&str],
    actions: &[&str],
) -> Value {
    json!({
        "id": id,
        "name": name,
        "version": "0.1.0",
        "description": format!("{name} skill — edit this description."),
        "author": "FRIDAY",
        "category": category,
        "icon": icon,
        "capabilities": capabilities,
        "actions": actions,
        "permissions": permissions,
        "readonly": false,
        "dependencies": [],
        "platform": "Any",
        "min_core_version": "0.1.0",
        "enabled": true,
        "memory": { "reads": [], "writes": [] }
    })
}

/// The generated `README.md` for a scaffolded skill. The manifest carries
/// everything the README documents (capabilities, memory keys), so `kind` is
/// not needed here.
pub fn readme(id: &str, name: &str, manifest: &Value) -> String {
    let caps = manifest["capabilities"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|c| format!("- `{}`", c.as_str().unwrap_or("")))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    let reads = manifest["memory"]["reads"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|c| format!("`{}`", c.as_str().unwrap_or("")))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let writes = manifest["memory"]["writes"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|c| format!("`{}`", c.as_str().unwrap_or("")))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    format!(
        "# {name}\n\n\
         {desc}\n\n\
         ## Capabilities\n\
         {caps}\n\n\
         ## Memory keys (M8)\n\
         Skills stay pure data. Declared keys are the ONLY memory this skill \
         may read (`memory.reads`) or write (`memory.writes`) through the \
         gated `SkillMemory` handle.\n\
         - reads: {reads}\n\
         - writes: {writes}\n\n\
         ## Develop\n\
         ```bash\n\
         # validate against the native capability registry\n\
         friday validate {id}\n\
         # install into ~/.friday/skills (loaded at next boot)\n\
         friday publish {id}\n\
         ```\n",
        desc = manifest["description"].as_str().unwrap_or("")
    )
}
