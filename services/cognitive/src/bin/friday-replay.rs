use friday_cognitive::recorder::EventRecorder;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: friday-replay <events-dir> [--recent N] [--topic PREFIX]");
        eprintln!();
        eprintln!("Examples:");
        eprintln!("  friday-replay data/events");
        eprintln!("  friday-replay data/events --recent 20");
        eprintln!("  friday-replay data/events --topic sensor.window");
        std::process::exit(1);
    }

    let dir = PathBuf::from(&args[1]);
    let mut n: usize = 50;
    let mut topic_filter: Option<String> = None;

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--recent" => {
                i += 1;
                if i < args.len() {
                    n = args[i].parse().expect("expected a number for --recent");
                }
            }
            "--topic" => {
                i += 1;
                if i < args.len() {
                    topic_filter = Some(args[i].clone());
                }
            }
            other => {
                eprintln!("Unknown argument: {}", other);
                std::process::exit(1);
            }
        }
        i += 1;
    }

    let recorder = match EventRecorder::new(&dir) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to open event log at {:?}: {}", dir, e);
            std::process::exit(1);
        }
    };

    let events = match recorder.read_recent(n) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Failed to read events: {}", e);
            std::process::exit(1);
        }
    };

    let mut count = 0;
    for event in &events {
        if let Some(ref prefix) = topic_filter {
            if !event.topic.starts_with(prefix) {
                continue;
            }
        }

        let time = chrono::DateTime::from_timestamp_millis(event.timestamp_ms as i64)
            .map(|dt| dt.format("%H:%M:%S%.3f").to_string())
            .unwrap_or_else(|| format!("???ms:{}", event.timestamp_ms));

        println!(
            "{}  {:<40}  source={:<15}  priority={:?}",
            time, &event.topic, &event.source, event.priority
        );
        count += 1;
    }

    eprintln!();
    eprintln!(
        "{} events shown (of {} total, filtered by {:?})",
        count,
        events.len(),
        topic_filter
    );
}
