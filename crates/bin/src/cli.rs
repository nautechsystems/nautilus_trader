use clap::Parser;

#[derive(Parser, Debug)]
#[command(version = "...", long_about = None)]
pub struct Args {
    #[arg(short, long)]
    pub config_path: String,
}