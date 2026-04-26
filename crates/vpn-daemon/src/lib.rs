//mod config;
pub mod daemon;
//mod ipc;
pub mod linux;
pub mod parser;
pub mod stats;
pub mod tests;
pub mod transport;
use anyhow::Result;

pub async fn run() -> anyhow::Result<()> {
    Ok(())
}
