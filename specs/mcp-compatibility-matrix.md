# MCP Server Compatibility Matrix

Tracks which Model Context Protocol servers FRIDAY's runtime has been validated
against. The runtime is server-agnostic: every server is just another
`ActionAdapter` registered under `mcp:<server>`, dispatched through the unified
`AgentKernel` (permission, timeout, cancellation, retry, audit, telemetry all
shared). This matrix records real (`FRIDAY_REAL_MCP=1`) validation, not just
hermetic fake servers.

## Legend

- ✅ **Validated** — connected to the real server, executed tools, asserted
  behavior end-to-end (against disk / process, not a mock).
- 🔌 **Hermetic only** — exercised via `FakeMcpServer`; protocol path proven but
  no real server run.
- ⏳ **Planned** — on the roadmap, not yet wired.
- 🚧 **Blocked** — dependency or environment issue preventing validation.

## Servers

| Server            | Transport | Status      | Validated tools                                   | Notes |
|-------------------|-----------|-------------|---------------------------------------------------|-------|
| `@modelcontextprotocol/server-filesystem` (v? ) | stdio | ✅ Validated (M9) | `read_text_file`, `write_file`, `edit_file`, `create_directory`, `list_directory` | Real Node server via `npx`. Sandbox enforced by server; out-of-sandbox writes refused (asserted on disk). `read_file` deprecated. |
| `mcp-server-git` (Python, via `uvx`) | stdio | ✅ Validated (M10) | `git_status`, `git_add`, `git_commit`, `git_diff`, `git_diff_unstaged`, `git_log`, `git_branch`, `git_checkout`, `git_create_branch` | Real Python server via `uvx`. Full local workflow validated: init → add → commit → diff → log → branch. Git-specific metadata (`commit_hash`, `branch`, `files_affected`) annotated by adapter; `git.commit.created` / `git.branch.changed` / `git.diff.generated` events emitted by kernel. |
| Memory MCP         | stdio     | ⏳ Planned   | — | Backs M12 memory/reflection. |
| Fetch MCP          | stdio     | ⏳ Planned   | — | Web retrieval for planner grounding. |
| SQLite MCP         | stdio     | ⏳ Planned   | — | Structured state / query backing. |

## Validation discipline

- Real-server tests live behind `FRIDAY_REAL_MCP=1` so hermetic CI stays clean.
- A validated server must prove: (1) connect + initialize + capability
  discovery, (2) at least one artifact-producing write, (3) a negative case
  (sandbox/permission rejected) asserted on real side-effects.
- All assertions go through `AgentKernel` — no MCP-specific branching in tests
  beyond constructing the transport.

## Roadmap

- **M9** (done): real Filesystem MCP — connect, write/read/create/list,
  sandbox rejection, artifact tracking (`artifact.created`), unified permission
  gating.
- **M10** (done): Git MCP — real local workflow (init/add/commit/diff/log/branch),
  Git-specific metadata extraction (`commit_hash`, `branch`, `files_affected`),
  domain events (`git.commit.created`, `git.branch.changed`, `git.diff.generated`).
- **M11**: workflow recorder/replay on top of validated servers.
- **M12**: memory + reflection consuming `artifact.created` and exec events.
