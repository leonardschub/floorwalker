//! Prints the agent tree the registry builds from your real `~/.claude`, read-only, and
//! how long the build took: a quick check against the installed Claude Code version
//! when its format drifts. `cargo run -p agents --example snapshot`

use std::time::Instant;

use agents::{ClaudeAgentsCommand, Registry, Sources, SystemClock};

fn main() {
    let home = std::env::var_os("HOME").expect("HOME is set");
    let home = std::path::Path::new(&home);
    let path = std::env::var_os("PATH");
    let registry = Registry::new(
        Sources::in_home(home),
        Box::new(SystemClock),
        Box::new(ClaudeAgentsCommand::resolve(path.as_deref(), home)),
    );
    let started = Instant::now();
    registry.build();
    let took = started.elapsed();
    let tree = registry.snapshot();
    println!("{}", serde_json::to_string_pretty(&tree).unwrap());
    eprintln!("built in {took:?}");
}
