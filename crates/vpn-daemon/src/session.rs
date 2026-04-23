use anyhow;
use tokio::task;
use tokio_util::sync;

#[derive(Debug, Clone)]
pub struct SessionSupervisor {
    profile_id: String,
    tasks: Vec<tokio::task::JoinHandle<anyhow::Result<()>>>,
    stop: tokio_util::sync::CancellationToken,
}
