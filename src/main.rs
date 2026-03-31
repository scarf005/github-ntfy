use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use github_ntfy_agent::{App, LoadedConfig};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "github-ntfy-agent")]
#[command(about = "Self-hosted GitHub notification agent for ntfy")]
struct Cli {
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run,
    Once,
    Check,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let loaded = LoadedConfig::load(cli.config)?;
    init_tracing(&loaded.config.app.log_level)?;
    let app = App::new(loaded).await?;

    match cli.command {
        Command::Run => app.run_loop().await,
        Command::Once => {
            app.poll_once().await?;
            Ok(())
        }
        Command::Check => app.check().await,
    }
}

fn init_tracing(default_level: &str) -> Result<()> {
    let env_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(default_level))
        .context("failed to configure logging")?;
    tracing_subscriber::fmt().with_env_filter(env_filter).init();
    Ok(())
}
