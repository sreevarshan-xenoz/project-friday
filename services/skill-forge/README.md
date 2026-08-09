# Skill Forge (M7)

The creator/validator/publisher that turns FRIDAY into a platform: third
parties build skills without touching the core.

```
friday create-skill minecraft --template minecraft
friday validate
friday publish
```

## Commands

| Command | What it does |
|---------|--------------|
| `create-skill <id> [--template <t>] [--dir <path>]` | Scaffolds `<dir>/<id>/skill.json` + `README.md` from a template (`generic`, `git`, `docker`, `minecraft`, `server`, `python`). All templates use canonical capability ids. |
| `validate [<path>]` | Loads skills with the real loader and qualifies them against the native registry. Exit 1 on failure. |
| `publish <id-or-path> [--dest <path>] [--force]` | Validates, then installs into `~/.friday/skills/<id>/` (`FRIDAY_SKILLS_DIR` overrides the root). Refuses broken or overwriting installs. |

## Design

- Skills are pure data: a published skill is the same directory the loader
  already reads (`<id>/skill.json` + optional `README.md`/`examples/`).
- `validate` reuses `friday-skills`' loader + `qualify()` against the real
  native registry, so a skill that validates here resolves at runtime.
  Limitation: only native capabilities are known — a skill targeting a
  future MCP/plugin capability soft-declares it and shows a failed check.
- The command logic is a library (`lib.rs`); `main.rs` is a thin argv shim.

## Tests

`cargo test -p friday-skill-forge` — template validity, scaffold→validate
loop, corrupted-manifest rejection, publish install + overwrite rules.
