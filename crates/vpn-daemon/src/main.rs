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
    let server_token = Token::new_with("57e8d456-e6aa-40f8-ac9c-174a8276aeac");

    let state = Arc::new(ServerState::new(subnet, 254));

    let traffic = Arc::new(TrafficCounters::new());
    let (owned_fd, name) = create_interface("xtvpn0")?;
    let tun = Arc::new(tokio::sync::Mutex::new(TunInterface::new(owned_fd, name)?));

    let (tx_to_tun, rx_to_tun) = mpsc::channel(1024);

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
    });
    let tun_writer_handle = tokio::spawn(tun_write_all(rx_to_tun, tun_writer, cancel_token_writer));
    let tx_clone = tx_to_tun.clone();
    let traffic_tcp = traffic.clone();
    tokio::spawn(async move {
        run_tcp_server(tcp_addr, state, tx_clone, traffic_tcp).await;
    });
    let db: sqlx::SqlitePool =
        sqlx::SqlitePool::connect("sqlite:/home/voice01/projects/tgbot_python/xtvpn_bot.db")
            .await?;
    tokio::spawn(async {
        let app = axum::Router::new()
            .route(
                "/api/v1/configs/generate",
                axum::routing::post(vpn_daemon::daemon::link::generate_config),
            )
            .route(
                "/api/v1/servers/add",
                axum::routing::post(vpn_daemon::daemon::link::insert_server),
            )
            .route(
                "/api/v1/servers/public",
                axum::routing::get(vpn_daemon::daemon::link::list_public_servers),
            )
            .with_state(db);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:8050")
            .await
            .expect("TcpListener failed to bind api port 8050");
        tracing::info!("API listening on http://127.0.0.1:8050");
        axum::serve(listener, app).await;
    });
    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutdown");
    Ok(())
}
