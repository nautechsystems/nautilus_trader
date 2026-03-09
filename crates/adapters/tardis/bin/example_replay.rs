use std::{env, path::PathBuf};

use nautilus_tardis::replay::run_tardis_machine_replay_from_config;

// Run the following to start the tardis-machine server:
// docker run -p 8000:8000 -p 8001:8001 -e "TM_API_KEY=YOUR_API_KEY" -d tardisdev/tardis-machine

#[tokio::main]
async fn main() {
    nautilus_common::logging::ensure_logging_initialized();

    // Retrieve the config path from first argument, or use a default example config
    let config_filepath = env::args().nth(1).map_or_else(
        || {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("bin")
                .join("example_config.json")
        },
        PathBuf::from,
    );

    run_tardis_machine_replay_from_config(&config_filepath)
        .await
        .unwrap();
}
