//! The `friday` CLI — Skill Forge (M7). Thin argv shim over
//! `friday_skill_forge`; all command logic lives in the library so it is
//! testable without spawning the binary.
//!
//! ```text
//! friday create-skill <id> [--template git|docker|minecraft|server|python|generic] [--dir <path>]
//! friday validate [<path>]
//! friday publish <id-or-path> [--dest <path>] [--force]
//! ```

use std::path::PathBuf;

use friday_skill_forge::templates::TemplateKind;
use friday_skill_forge::{create_skill, default_skill_dir, publish, validate_path};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(&args));
}

fn run(args: &[String]) -> i32 {
    let Some(cmd) = args.first().map(String::as_str) else {
        print_usage();
        return 2;
    };
    match cmd {
        "create-skill" => cmd_create(&args[1..]),
        "validate" => cmd_validate(&args[1..]),
        "publish" => cmd_publish(&args[1..]),
        "help" | "--help" | "-h" => {
            print_usage();
            0
        }
        "version" | "--version" | "-V" => {
            println!("friday {} (Skill Forge)", env!("CARGO_PKG_VERSION"));
            0
        }
        other => {
            eprintln!("error: unknown command '{other}'");
            print_usage();
            2
        }
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn has(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn cmd_create(args: &[String]) -> i32 {
    let Some(id) = args.first() else {
        eprintln!("error: create-skill requires a skill id, e.g. `friday create-skill minecraft`");
        return 2;
    };
    let template = match flag(args, "--template") {
        Some(name) => match TemplateKind::parse(&name) {
            Some(kind) => kind,
            None => {
                eprintln!(
                    "error: unknown template '{name}' (known: {})",
                    TemplateKind::all()
                        .iter()
                        .map(|k| k.label())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                return 2;
            }
        },
        None => TemplateKind::Generic,
    };
    let dir = flag(args, "--dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    match create_skill(id, template, &dir) {
        Ok(report) => {
            println!(
                "✔ scaffolded '{}' (template: {}) at {}",
                report.skill_id,
                template.label(),
                report.dir.display()
            );
            for f in &report.files {
                println!("  {}", f.display());
            }
            0
        }
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn cmd_validate(args: &[String]) -> i32 {
    let path = args
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    match validate_path(&path) {
        Ok(report) => {
            println!("validating: {}", report.source.display());
            for failure in &report.load_failures {
                println!("✘ {failure}");
            }
            for skill in &report.skills {
                let mark = if skill.passed { "✔" } else { "✘" };
                println!(
                    "{mark} {} v{} — qualification {}",
                    skill.skill_id,
                    skill.version,
                    if skill.passed { "PASS" } else { "FAIL" }
                );
                for check in &skill.checks {
                    let c = if check.ok { "  ✓" } else { "  ✗" };
                    println!("{c} {}: {}", check.name, check.detail);
                }
            }
            if report.skills.is_empty() && report.load_failures.is_empty() {
                // Validating nothing must not look like a green pass.
                eprintln!("⚠ no skills found at {}", report.source.display());
                1
            } else if report.passed() {
                println!(
                    "✔ {} skill(s) validated, no load failures",
                    report.skills.len()
                );
                0
            } else {
                eprintln!(
                    "✘ validation failed ({} load error(s))",
                    report.load_failures.len()
                );
                1
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            2
        }
    }
}

fn cmd_publish(args: &[String]) -> i32 {
    let Some(source) = args.first().map(PathBuf::from) else {
        eprintln!("error: publish requires a skill id or path, e.g. `friday publish minecraft`");
        return 2;
    };
    let dest_root = flag(args, "--dest")
        .map(PathBuf::from)
        .unwrap_or_else(default_skill_dir);
    let force = has(args, "--force");
    match publish(&source, &dest_root, force) {
        Ok(report) => {
            println!(
                "✔ published '{}' → {} ({} file(s))",
                report.skill_id,
                report.dest.display(),
                report.files_copied
            );
            println!("  loadable from ~/.friday/skills at the next boot");
            0
        }
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn print_usage() {
    println!(
        "FRIDAY Skill Forge (M7)\n\
         \n\
         Usage:\n\
         \x20 friday create-skill <id> [--template git|docker|minecraft|server|python|generic] [--dir <path>]\n\
         \x20 friday validate [<path>]\n\
         \x20 friday publish <id-or-path> [--dest <path>] [--force]\n\
         \x20 friday help | version\n"
    );
}
