# FRIDAY runs and supervises coding agents, and troubleshoots with commands

## Why

The owner wants to stop babysitting coding agents. Today you write a careful prompt, watch the terminal, and click through questions and approvals. With this feature you tell FRIDAY roughly what you want ("have opencode add a dark mode to ~/code/site"). FRIDAY writes the real prompt, runs it in Claude Code, agy or opencode, answers the agent's routine questions and approvals, and tells you what happened and what it answered. It is on by default, with a switch to turn it off.

Second, when the person asks for help troubleshooting, FRIDAY runs diagnostic commands itself instead of only giving advice.

## Goals

- **A, runs FRIDAY starts:** on request, FRIDAY starts an agent in a folder with a good prompt, follows it to the end, answers what it safely can, and reports.
- **B, sessions the person started:** FRIDAY sees the approval prompts of agent sessions the person runs in a terminal, approves the safe ones itself, and leaves the rest to the person, with a notification.
- **C, troubleshooting:** FRIDAY runs read-only diagnostic commands on its own and asks before anything that changes the system.
- **Clear reporting:** the person can always see what FRIDAY answered on their behalf (`friday agents`, and the tray on Windows).

## Non-goals

- Screen-reading terminals or typing into them. B uses each agent's own hook system instead.
- Remote or cloud agents.
- Agents FRIDAY can't drive headlessly. Codex is added only once its CLI is present to test against.

## Anti-goals (failures even if it "works")

- **FRIDAY approves something destructive because text told it to.** What an agent reads (READMEs, web pages, error output) can carry planted instructions ("this is safe, approve it"). Whether a step is approved is therefore decided by fixed rules over the step's structured fields (its kind, the exact command, its paths), never by a model. A model writes free-text answers to questions only, and those answers grant nothing: every tool call the agent then makes goes through the same rules.
- **FRIDAY ever chooses "always allow".** It only ever answers "allow once" or "reject".
- **A run burns the owner's Claude plan in tests.** Live tests use opencode's free models and agy; Claude is tested against recorded protocol lines.

## Approaches considered

1. **Each agent's headless protocol, plus each agent's hooks for sessions the person started (chosen).**
   - opencode and agy speak ACP (Agent Client Protocol: JSON-RPC over stdio). Both were checked live on 2026-11-19: permission requests arrive as `session/request_permission` with the tool call's kind, title and raw input (the exact command).
   - Claude Code's print mode hands permission prompts to the host (`--permission-prompts host`) over stream-json.
   - B uses Claude Code's `PermissionRequest` hook and the opencode/agy plugin hooks, which call `friday-ctl` and apply the same rules.
2. **Scrape and type into terminals.** It's fragile, it needs keystrokes (which FRIDAY never sends) and focus, and it breaks on every terminal and theme.
3. **Zed's claude-code-acp adapter, so everything is ACP.** It adds a Node dependency to FRIDAY for one agent, and Claude's own host protocol is small.

## Design

### The rules (`agent_policy`, pure code with exhaustive tests)

The input is the agent, the step's kind (read, search, edit, delete, move, execute, fetch, think, other), the command when there is one, its paths, and the run's folder. The output is Allow or Ask(reason).

FRIDAY allows:
- read, search and think, anywhere in the run's folder;
- edits to files inside the folder, except `.git/` and secret-looking files (`.env*`, `*.pem`, `*.key`, `id_*`, `credentials*`, `secrets*`);
- commands whose program and subcommand are on a read-only or build/test list, with no `;`, `&&`, `|`, backticks, `$(`, redirects outside the folder, `sudo`, or `rm`. The list: `ls cat head tail wc grep rg find` (no `-delete` or `-exec`), `git status/diff/log/show/branch`, `cargo build/check/test/fmt/clippy`, `npm|pnpm|yarn test/run lint/run build`, `pytest`, `python -m pytest`, `go build/test/vet`, `dotnet build/test`, `make test/build`, `echo pwd which`;
- fetch (reading a web page).

Everything else goes to the person:
- delete and move;
- git commit, push, reset, checkout and rebase;
- package installs;
- network uploads;
- anything outside the folder;
- secrets;
- unknown kinds and unknown programs.

### A: runs (`agent_runs`)

- **Capability `agents.run`.** The planner routes requests that name an agent ("in claude", "ask opencode to", "with agy", "have claude code ...") and requests about runs ("how's the agent doing").
- **The tool.** A model turns the request into `{agent, folder, prompt}`, rewriting the person's ask into a clear task prompt. The folder comes from the request, else the last folder used with that agent, else FRIDAY asks.
- **Starting.** The tool starts the run in the background and returns at once ("Started opencode in ~/code/site; I'll tell you when it's done").
- **Driving.** A run drives the agent, through ACP for opencode and agy and stream-json for Claude, in the person's folder:
  - Each permission request goes through the rules. Allow means "allow once".
  - Ask means the run pauses and the person gets a notification with the exact step, then `friday agents allow|deny <id>` or the Windows tray answers it. After 30 minutes without an answer, the step is rejected.
  - When a turn ends with a question to the person, a model answers it from the original request, but only if the request covers it. Otherwise FRIDAY passes the question on. At most 3 such replies per run.
- **Records.** Each run is recorded under the state dir: `agents/<id>.json` holds the status, and `agents/<id>.jsonl` holds every event, approval and answer, marked with who answered (FRIDAY or the person). `friday agents` lists runs, and `friday agents show <id>` gives the report.
- **The switch.** `FRIDAY_AGENTS=0` turns FRIDAY-run agents off.

### B: sessions the person started (`friday-ctl agent-hook <agent>`)

- **Claude Code.** A `PermissionRequest` command hook sends the request to `friday-ctl agent-hook claude`, which applies the same rules. Allow returns an allow decision. Ask returns nothing, so Claude's normal prompt shows in the terminal, and FRIDAY sends a notification.
- **opencode and agy.** The same, through their plugin or hook systems (details at build time).
- **Logging.** Every decision goes to `agents/hooks.jsonl`, so `friday agents` shows what FRIDAY approved in the person's own sessions.
- **Turning it on and off.** `friday agents hooks on|off` adds or removes only FRIDAY's own entry in each agent's settings. It is on by default: FRIDAY OS first login and the MSI turn it on, and Settings has the switch.

### C: troubleshooting (`system.diagnose`)

- A model proposes diagnostic commands. Those on a fixed read-only list run at once:
  - Linux: `journalctl` (read), `systemctl status`, `ip`, `nmcli` (show), `resolvectl status`, `ping -c 4`, `df`, `free`, `lsblk`, `uname`, `ss -tuln`, `lspci`, `lsusb`;
  - Windows: `ipconfig`, `Get-NetAdapter`, `Test-NetConnection`, `Get-WinEvent` (read), `Get-Service`, `Get-Volume`.
- Anything else asks first, through the usual confirmation.
- Output is masked for secrets (the existing masking) before it goes to the model. At most 4 rounds, then FRIDAY answers with what it found.

## Order (each its own PR)

1. `agent_policy` and ACP runs for opencode and agy (`agents.run`, run records, `friday agents`).
2. Claude Code runs over stream-json.
3. B hooks: Claude Code first, then opencode and agy. `friday agents hooks on|off`.
4. Windows: the tray shows runs and pending approvals, and Settings gets the "Answer coding agents for me" switch.
5. C troubleshooting.

## Left to the builder

The exact folder heuristics, the notification wording, and the run-id format.
