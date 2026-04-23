use crate::linux::tun::{TunInterface, create_interface};
use anyhow::{Context, Result, bail};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::{sleep, timeout};

fn require_root() {
    let euid = unsafe { libc::geteuid() };
    assert_eq!(
        euid, 0,
        "этот тест надо запускать от root: sudo cargo test --test tun_read -- --ignored --nocapture"
    );
}

async fn run_ok(program: &str, args: &[&str]) -> Result<()> {
    let output = Command::new(program)
        .args(args)
        .output()
        .await
        .with_context(|| format!("failed to run {program} {args:?}"))?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "command failed: {program} {args:?}\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            stdout,
            stderr
        );
    }

    Ok(())
}

#[tokio::test]
#[ignore = "requires root + Linux TUN + iproute2"]
async fn tun_reads_a_real_packet() -> Result<()> {
    require_root();

    let ifname_hint = format!("tt{}", std::process::id());

    let (fd, real_name) = create_interface(&ifname_hint).context("create_tun failed")?;

    let tun = TunInterface::new(fd, real_name.clone()).context("TunInterface::new failed")?;

    eprintln!("created tun interface: {}", tun.name());

    run_ok("ip", &["addr", "add", "10.77.0.1/24", "dev", tun.name()]).await?;
    run_ok("ip", &["link", "set", "dev", tun.name(), "up"]).await?;

    sleep(Duration::from_millis(100)).await;

    let mut buf = vec![0u8; 65535];

    let read_fut = async {
        let n = tun.read_packet(&mut buf).await?;
        Ok::<usize, anyhow::Error>(n)
    };

    let ping_fut = async {
        sleep(Duration::from_millis(100)).await;

        let _ = Command::new("ping")
            .args(["-c", "1", "-W", "1", "10.77.0.2"])
            .status()
            .await;
    };

    let (read_res, _) = tokio::join!(timeout(Duration::from_secs(3), read_fut), ping_fut);

    let n = read_res.context("timeout while waiting for packet from tun")??;

    eprintln!("got packet from {}: {} bytes", tun.name(), n);

    assert!(n > 0, "expected to read at least one packet from tun");
    Ok(())
}
