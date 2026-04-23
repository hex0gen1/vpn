#![allow(unusued_variables)]

use anyhow::Result;
#[tokio::main]
async fn main() -> Result<()> {
    vpn_daemon::run().await
}
