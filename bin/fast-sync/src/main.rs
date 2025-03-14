use clap::Parser;
use fast_sync::{Args, FastSync};
use trin_utils::log::init_tracing_logger;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing_logger();

    let args = Args::parse();
    let fast_sync = FastSync::start(args).await?;

    tokio::signal::ctrl_c()
        .await
        .expect("failed to pause until ctrl-c");

    drop(fast_sync);
    Ok(())
}
