use anyhow::{Context, Result};
use clap::Parser;
use tracing_subscriber::{EnvFilter, prelude::*};

use nixcp::push::Push;
use nixcp::store::Store;
use nixcp::{Cli, Commands};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    init_logging(cli.tokio_console);

    match &cli.command {
        Commands::Push(cli) => {
            let store = Store::connect()?;
            let push = Box::leak(Box::new(Push::new(cli, store).await?));
            push.add_paths(cli.paths.clone())
                .await
                .context("add paths to push")?;
            push.run().await.context("nixcp run")?;
        }
    }

    Ok(())
}

fn init_logging(tokio_console: bool) {
    let env_filter = EnvFilter::from_default_env();
    let fmt_layer = tracing_subscriber::fmt::layer().with_filter(env_filter);

    let console_layer = if tokio_console {
        Some(console_subscriber::spawn())
    } else {
        None
    };

    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(console_layer)
        .init();

    if tokio_console {
        println!("tokio-console is enabled");
    }
}
