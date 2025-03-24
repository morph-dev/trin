use clap::Parser;
use fast_sync::{Args, FastSync};
use trin_utils::log::init_tracing_logger;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing_logger();

    let args = Args::parse();
    FastSync::run(args).await?;

    Ok(())
}
