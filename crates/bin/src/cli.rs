use clap::Parser;

#[derive(Parser, Debug)]
#[command(version = "...", long_about = None)]
pub struct Args {
    #[arg(short, long, default_value = "config.toml")]
    pub config_path: String,
}
