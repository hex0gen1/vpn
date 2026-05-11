pub mod daemon;
pub mod linux;
pub mod parser;
pub mod stats;
pub mod tests;
pub mod transport;
use std::sync::Arc;
use tokio::sync::mpsc;
use vpn_daemon::linux::tun::{TunInterface, create_interface};
use vpn_daemon::transport::client::Token;
use vpn_daemon::transport::server::tun_write_all;
use vpn_daemon::transport::server::{
    ServerState, TrafficCounters, run_tcp_server, tun_reader_loop,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    tracing::info!("Starting daemon");
    let tcp_addr = "0.0.0.0:443".parse()?;
    let udp_addr: std::net::SocketAddr = "0.0.0.0:11949".parse()?;
    let subnet = "10.8.0.0".parse()?;
    let server_token = Token::new_with("8d29950e-2fee-48df-b9d2-6475e929f01e");

    // Состояние
    let state = Arc::new(ServerState::new(subnet, 254));

    let traffic = Arc::new(TrafficCounters::new());
    let (owned_fd, name) = create_interface("xtvpn0")?;
    let tun = Arc::new(tokio::sync::Mutex::new(TunInterface::new(owned_fd, name)?));
    // Каналы
    let (tx_to_tun, rx_to_tun) = mpsc::channel(1024);

    // TUN reader (один на всех)
    let state_clone = state.clone();
    let traffic_clone = traffic.clone();
    let cancel_token_reader = tokio_util::sync::CancellationToken::new();
    let cancel_token_writer = tokio_util::sync::CancellationToken::new();
    let udp_socket = tokio::net::UdpSocket::bind(udp_addr).await?;
    let tun_writer = tun.clone();
    let tun_reader = tun.clone();
    tokio::spawn(async move {
        tun_reader_loop(
            tun_reader,
            state_clone,
            udp_socket,
            cancel_token_reader,
            traffic_clone,
        )
        .await;
    }); // tun_write_all ждёт std::sync::Mutex
    let tun_writer_handle = tokio::spawn(tun_write_all(rx_to_tun, tun_writer, cancel_token_writer));
    // TCP сервер
    let tx_clone = tx_to_tun.clone();
    let traffic_tcp = traffic.clone();
    tokio::spawn(async move {
        run_tcp_server(tcp_addr, state, tx_clone, traffic_tcp).await;
    });

    // Ждём сигнала
    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutdown");
    Ok(())
}
