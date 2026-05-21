use crate::linux::routing::{
    ClientNetworkConfig, RouteConfig, ServerNetworkConfig, TransportConfig,
};
use crate::linux::tun::{TunFd, TunInterface, create_interface};
use crate::transport::client::{HelloAckError, Token};
use crate::transport::frame::{DecodeError, DecodedFrame, FrameKind, decode_frame, encode_frame};
use hkdf::Hkdf;
use sha2::Sha256;
use std::collections::HashSet;
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use vpn_types::{
    VpnProfile,
    error::{ErrorLevel, VpnError},
};
use x25519_dalek::{EphemeralSecret, PublicKey};
extern crate scopeguard;
use tokio::io::{AsyncRead, AsyncWrite};
use tracing::{info, warn};
pub async fn bind_server(config: ServerNetworkConfig) -> std::io::Result<UdpSocket> {
    let addr = std::net::SocketAddr::new(config.server_addr, config.server_port);
    let socket = UdpSocket::bind(addr).await?;
    Ok(socket)
}
pub async fn bind_token(profile: VpnProfile) -> Token {
    let mut token = Token::new();
    token.fill_token_vless(&profile);
    token
}
pub trait AsyncServerTransport: Unpin + Send + 'static {
    fn peer_addr(&self) -> std::net::SocketAddr;
    fn current_transport(&self) -> &'static str;
    async fn recv_frame(&mut self, buf: &mut [u8]) -> std::io::Result<usize>;
    async fn send_frame(&mut self, buf: &[u8]) -> std::io::Result<usize>;
}
pub struct UdpTransport {
    socket: tokio::net::UdpSocket,
    peer: std::net::SocketAddr,
}
impl AsyncServerTransport for UdpTransport {
    fn peer_addr(&self) -> std::net::SocketAddr {
        self.peer
    }
    fn current_transport(&self) -> &'static str {
        "udp"
    }
    async fn recv_frame(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let socket = &self.socket;
        let len = socket.recv(buf).await?;
        Ok(len)
    }
    async fn send_frame(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let socket = &self.socket;
        let len = socket.send(buf).await?;
        Ok(len)
    }
}
impl AsyncServerTransport for TcpTransport {
    fn peer_addr(&self) -> std::net::SocketAddr {
        self.peer
    }
    fn current_transport(&self) -> &'static str {
        "tcp"
    }
    async fn recv_frame(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut len = [0u8; 4];
        self.stream.read_exact(&mut len).await?;
        let frame_len = u32::from_be_bytes(len) as usize;
        if frame_len == 0 || frame_len > buf.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Too large frame.",
            ));
        }
        self.stream.read_exact(&mut buf[..frame_len]).await?;
        Ok(frame_len)
    }
    async fn send_frame(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let len = buf.len() as u32;
        self.stream.write_all(&len.to_be_bytes()).await?;
        self.stream.write_all(buf).await?;
        Ok(buf.len())
    }
}
#[derive(Clone, Debug)]
pub struct Peer {
    pub user_ip: Ipv4Addr,
    pub public_socket: std::net::SocketAddr,
    pub last_seen: std::time::Instant,
    pub crypto: Arc<CryptoState>,
    pub user_id: String,
    pub session_id: u64,
    pub tx_reply: Option<mpsc::Sender<Vec<u8>>>,
}
pub struct TcpTransport {
    stream: tokio::net::TcpStream,
    peer: std::net::SocketAddr,
}
impl Peer {
    pub fn new(
        ip: Ipv4Addr,
        sock: std::net::SocketAddr,
        time: std::time::Instant,
        crypto_cx: Arc<CryptoState>,
        user_id_rx: String,
        session_id: u64,
    ) -> Self {
        let (tx_send, _) = mpsc::channel(2048);
        Self {
            user_ip: ip,
            public_socket: sock,
            last_seen: time,
            crypto: crypto_cx,
            user_id: user_id_rx,
            session_id,
            tx_reply: Some(tx_send),
        }
    }
}
#[derive(Debug)]
pub struct IpAllocator {
    pub base: u32,
    pub next: u32,
    pub max: u32,
    pub used: HashSet<u32>,
}
impl IpAllocator {
    pub fn new(subnet: Ipv4Addr, size: u32) -> Self {
        Self {
            base: u32::from(subnet) & 0xFFFFFF00,
            next: 2,
            max: size,
            used: HashSet::with_capacity(size as usize),
        }
    }
    pub fn allocate(&mut self) -> Option<Ipv4Addr> {
        let start = self.next;

        loop {
            if self.next > self.max {
                self.next = 2;
            }
            let host = self.next;

            if !self.used.contains(&host) {
                self.used.insert(host);
                let ip = Ipv4Addr::from(self.base | host);
                self.next += 1;
                return Some(ip);
            };
            if self.next == start && self.used.contains(&self.next) {
                return None;
            }
            self.next += 1;
        }
    }
    pub fn release(&mut self, ip: Ipv4Addr) {
        let host = u32::from(ip) & 0xFF;
        self.used.remove(&host);
    }
    pub fn available(&self) -> usize {
        (self.max - 1) as usize - self.used.len()
    }
}
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::Mutex as TokioMutex;

#[derive(Debug, Default)]
pub struct TrafficCounters {
    pub bytes_rx: AtomicU64,
    pub bytes_tx: AtomicU64,
    pub packets_rx: AtomicU64,
    pub packets_tx: AtomicU64,
}
#[derive(Clone, Debug, Default)]
pub struct TrafficSnapshot {
    pub bytes_rx: u64,
    pub bytes_tx: u64,
    pub packets_rx: u64,
    pub packets_tx: u64,
}
impl TrafficSnapshot {
    pub fn new() -> Self {
        Self {
            bytes_rx: 0,
            bytes_tx: 0,
            packets_rx: 0,
            packets_tx: 0,
        }
    }
}
impl TrafficCounters {
    pub fn new() -> Self {
        Self {
            bytes_tx: AtomicU64::new(0),
            bytes_rx: AtomicU64::new(0),
            packets_rx: AtomicU64::new(0),
            packets_tx: AtomicU64::new(0),
        }
    }
    pub fn add_rx(&self, len: u64) {
        self.bytes_rx.fetch_add(len, Ordering::Relaxed);
        self.packets_rx.fetch_add(1, Ordering::Relaxed);
    }
    pub fn add_tx(&self, len: u64) {
        self.bytes_tx.fetch_add(len, Ordering::Relaxed);
        self.packets_tx.fetch_add(1, Ordering::Relaxed);
    }
    pub fn snapshot(&self) -> TrafficSnapshot {
        TrafficSnapshot {
            bytes_rx: self.bytes_rx.load(Ordering::Relaxed),
            bytes_tx: self.bytes_tx.load(Ordering::Relaxed),
            packets_rx: self.packets_rx.load(Ordering::Relaxed),
            packets_tx: self.packets_tx.load(Ordering::Relaxed),
        }
    }
}
#[derive(Debug)]
pub struct ServerState {
    pub allocator: TokioMutex<IpAllocator>,
    pub peers: TokioMutex<peers_table>,
    pub server_crypto: Arc<CryptoState>,
}
pub fn generate_session_id() -> u64 {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("OS entropy failed");
    u64::from_be_bytes(bytes)
}
impl ServerState {
    pub async fn connect_peer(
        &self,
        socket: std::net::SocketAddr,
        user_id: String,
    ) -> Option<Ipv4Addr> {
        let ip = self
            .allocator
            .lock()
            .await
            .allocate()
            .ok_or_else(|| anyhow::anyhow!("Ip pool exhausted!"))
            .ok()?;
        let crypto = generate_crypto_state()
            .map_err(|e| anyhow::anyhow!("CryptoGeneration failed! {}", e))
            .ok()?;
        let peer = Arc::new(Peer::new(
            ip,
            socket,
            std::time::Instant::now(),
            crypto,
            user_id,
            generate_session_id(),
        ));
        self.peers.lock().await.insert(peer);
        Some(ip)
    }
    pub async fn disconnect_peer(&self, socket: std::net::SocketAddr) -> bool {
        let mut peers = self.peers.lock().await;
        if let Some(peer) = peers.remove(&socket) {
            self.allocator.lock().await.release(peer.user_ip);
            true
        } else {
            false
        }
    }
    pub fn new(subnet: Ipv4Addr, pool_size: u32) -> Self {
        let state = generate_crypto_state()
            .map_err(|e| VpnError::CryptoError(e.to_string()))
            .unwrap();
        Self {
            allocator: tokio::sync::Mutex::new(IpAllocator::new(subnet, pool_size)),
            peers: tokio::sync::Mutex::new(peers_table::new()),
            server_crypto: state,
        }
    }
    pub async fn peer_count(&self) -> usize {
        self.peers.lock().await.by_user_ip.len()
    }
}
#[derive(Debug)]
pub struct peers_table {
    pub by_user_ip: std::collections::HashMap<std::net::Ipv4Addr, Arc<Peer>>,
    pub by_user_public_socket: std::collections::HashMap<std::net::SocketAddr, Arc<Peer>>,
    pub by_user_id: std::collections::HashMap<String, Arc<Peer>>,
}
impl peers_table {
    pub fn put_peer_and_ip(&mut self, ip: std::net::Ipv4Addr, peer: Arc<Peer>) {
        self.by_user_ip.insert(ip, peer);
    }
    pub fn put_peer_and_socket(&mut self, socket: std::net::SocketAddr, peer: Arc<Peer>) {
        self.by_user_public_socket.insert(socket, peer);
    }
    pub fn new() -> Self {
        Self {
            by_user_ip: std::collections::HashMap::new(),
            by_user_public_socket: std::collections::HashMap::new(),
            by_user_id: std::collections::HashMap::new(),
        }
    }
    pub fn remove(&mut self, socket: &std::net::SocketAddr) -> Option<Arc<Peer>> {
        if let Some(peer) = self.by_user_public_socket.remove(socket) {
            self.by_user_ip.remove(&peer.user_ip);
            Some(peer)
        } else {
            None
        }
    }
    pub fn insert(&mut self, peer: Arc<Peer>) {
        self.by_user_public_socket
            .insert(peer.public_socket, peer.clone());
        self.by_user_ip.insert(peer.user_ip, peer.clone());
        self.by_user_id.insert(peer.user_id.clone(), peer);
    }
    pub fn put_user_id_and_socket(
        &mut self,
        user_id: String,
        peer: Arc<Peer>,
    ) -> Option<Arc<Peer>> {
        self.by_user_id.insert(user_id, peer)
    }
    pub fn get_by_ip(&self, ip: &std::net::Ipv4Addr) -> Option<Arc<Peer>> {
        self.by_user_ip.get(ip).cloned()
    }
    pub fn get_by_addr(&self, socket: &std::net::SocketAddr) -> Option<Arc<Peer>> {
        self.by_user_public_socket.get(socket).cloned()
    }
    pub fn get_by_id(&self, id: &str) -> Option<Arc<Peer>> {
        self.by_user_id.get(id).cloned()
    }
}
use tokio::io::AsyncReadExt;
pub async fn handle_hello_udp(
    socket: &UdpSocket,
    server_token: &Token,
    peers: Arc<TokioMutex<peers_table>>,
    state: &Arc<ServerState>,
) -> Result<(Arc<Peer>, Arc<CryptoState>), HelloAckError> {
    let mut buf = vec![0u8; 2048];
    let (len, peer_addr) = socket
        .recv_from(&mut buf)
        .await
        .map_err(HelloAckError::Io)?;

    let frame = decode_frame(&buf[..len])?;
    if frame.kind != FrameKind::HELLO {
        return Err(HelloAckError::UnexpectedKind(frame.kind));
    }

    let token_str =
        std::str::from_utf8(&frame.payload).map_err(|_| HelloAckError::InvalidTokenEncoding)?;
    if token_str != server_token.token {
        return Err(HelloAckError::TokenMismatch(token_str.to_string()));
    }

    let user_id = token_str.to_string();
    let ip = state
        .connect_peer(peer_addr, user_id.clone())
        .await
        .ok_or(HelloAckError::IpPoolExhausted)?;

    let crypto = generate_crypto_state()?;
    let peer = state
        .peers
        .lock()
        .await
        .get_by_addr(&peer_addr)
        .clone()
        .ok_or(HelloAckError::PeerNotFound)?;

    // Отправка ответа
    send_helloack(socket, peer_addr, peers, crypto.clone()).await?;
    Ok((peer, crypto))
}
pub async fn handle_hello_tcp(
    stream: &mut tokio::net::TcpStream,
    peers: Arc<TokioMutex<peers_table>>,
    state: Arc<ServerState>,
    server_token: &Token,
    crypto: &CryptoState,
    peer_addr: std::net::SocketAddr,
) -> Result<(Arc<Peer>, Arc<CryptoState>), VpnError> {
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(len_buf.as_mut_slice())
        .await
        .map_err(|e| VpnError::Io(e));
    let frame_len = u32::from_be_bytes(len_buf) as usize;
    if frame_len > 65536 {
        return Err(VpnError::FrameTooLarge(frame_len.to_string()));
    }

    let mut encrypted = vec![0u8; frame_len];
    stream
        .read_exact(&mut encrypted)
        .await
        .map_err(|e| VpnError::Io(e));

    let mut plaintext =
        decrypt_frame_sync(&encrypted, crypto).map_err(|e| VpnError::CryptoError(e.to_string()))?;
    let frame = decode_frame(&mut plaintext).unwrap();
    if frame.kind != FrameKind::HELLO {
        return Err(VpnError::InvalidFrame(frame.kind.as_str()));
    }

    let token_str =
        std::str::from_utf8(&frame.payload).map_err(|e| VpnError::CryptoError(e.to_string()))?;
    if token_str != server_token.token {
        return Err(VpnError::CryptoError(token_str.to_string()));
    }

    let user_id = token_str.to_string();
    let ip = state
        .connect_peer(peer_addr, user_id.clone())
        .await
        .ok_or(VpnError::Timeout)?;

    let crypto = generate_crypto_state().map_err(|e| VpnError::CryptoError(e.to_string()))?;
    let peer = state
        .peers
        .lock()
        .await
        .get_by_addr(&peer_addr)
        .ok_or(VpnError::Timeout)?;

    send_helloack_tcp(stream, &peer, crypto.clone()).await?;
    Ok((peer, crypto))
}
pub async fn send_helloack_tcp(
    stream: &mut tokio::net::TcpStream,
    peer: &Arc<Peer>,
    crypto: Arc<CryptoState>,
) -> Result<(), std::io::Error> {
    let mut frame = encode_frame(FrameKind::HELLOACK, peer.session_id, &[]);
    let encrypted = encrypt_frame(&mut frame, crypto)
        .await
        .map_err(|e| VpnError::CryptoError(e.to_string()))
        .unwrap();
    let len = encrypted.len() as u32;
    stream.write_all(&len.to_be_bytes()).await?;
    stream.write_all(&encrypted).await?;
    stream.flush().await?;
    Ok(())
}
pub fn generate_crypto_state() -> std::io::Result<Arc<CryptoState>> {
    let mut key = [0u8; 32];
    getrandom::fill(&mut key);

    Ok(Arc::new(CryptoState {
        key: key,
        tx_nonce: std::sync::atomic::AtomicU64::new(1),
        rx_last_nonce: Arc::new(Mutex::new(0 as u64)),
        cipher_type: crate::transport::frame::CipherAlg::AesGcm,
    }))
}
pub async fn send_helloack(
    socket: &UdpSocket,
    peer_addr: std::net::SocketAddr,
    peers: Arc<TokioMutex<peers_table>>,
    crypto: Arc<CryptoState>,
) -> std::io::Result<()> {
    let kind = FrameKind::HELLOACK;
    let peer = peers.lock().await.get_by_addr(&peer_addr).unwrap();
    let session_id = peer.session_id;
    let mut payload = vec![];
    let mut frame = encode_frame(kind, session_id, &mut payload);
    let encrypted = encrypt_frame(&frame, crypto.clone()).await.unwrap();
    socket.send_to(&encrypted, peer_addr).await?;
    Ok(())
}
//pub async fn handle_data(socket: &UdpSocket, session: &mut Session) -> Result<(), anyhow::Error> {
//    let mut buf = vec![0u8; 2048];
//    let res = socket.recv(&mut buf).await?;
//    let frame = decode_frame(&mut buf[..res]).unwrap();
//
//    let src =
//        parse_ipv4_src(&frame.payload).ok_or(anyhow::Error::new("bad packet formation".into()));
//    if session.authentificated {
//        let (owned, name) = create_interface("tun0")?;
//        let tun = TunInterface::new(owned, name)?;
//        tun.write_packet()
//    }
//}
use aead::{Aead, Key, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use getrandom::fill;
type AesCipher = Aes256Gcm;
pub async fn run_tcp_listener(
    addr: std::net::SocketAddr,
    state: Arc<ServerState>,
    tx_to_tun: mpsc::Sender<Vec<u8>>,
    traffic: Arc<TrafficCounters>,
) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("TCP server listening on {}", addr);

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        let state = state.clone();
        let tun = tx_to_tun.clone();
        let traffic = traffic.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_tcp_peer(stream, peer_addr, state, tun, traffic).await {
                warn!("TCP peer {} error: {}", peer_addr, e);
            }
        });
    }
}
use tokio::io::AsyncWriteExt;
async fn handle_tcp_peer(
    mut stream: tokio::net::TcpStream,
    peer_addr: std::net::SocketAddr,
    state: Arc<ServerState>,
    tx_to_tun: mpsc::Sender<Vec<u8>>,
    traffic: Arc<TrafficCounters>,
) -> anyhow::Result<()> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;

    let frame = decode_frame(&buf).unwrap();
    let session_id = frame.session_id;
    let token = std::str::from_utf8(&frame.payload)?;
    let user_id = token.to_string();
    let ip = state.connect_peer(peer_addr, user_id).await.unwrap();
    let crypto = generate_crypto_state()?;

    let ack_frame = encode_frame(FrameKind::HELLOACK, session_id, &[]);
    let encrypted = encrypt_frame(&ack_frame, crypto.clone()).await?;
    let ack_len = encrypted.len() as u32;
    stream.write_all(&ack_len.to_be_bytes()).await?;
    stream.write_all(&encrypted).await?;
    handle_data_loop_tcp(
        stream,
        peer_addr,
        ip,
        crypto,
        state,
        tx_to_tun,
        traffic,
        tokio_util::sync::CancellationToken::new(),
    )
    .await
}
pub fn encrypt_frame_deprecated(
    frame: Vec<u8>,
    key: &[u8; 32],
    aad: Option<&[u8]>,
) -> anyhow::Result<Vec<u8>> {
    let cipher = AesCipher::new_from_slice(key)?;
    let mut nonce_bytes = [0u8; 12];
    fill(&mut nonce_bytes);
    let nonce = Nonce::from(nonce_bytes);

    let payload = aead::Payload {
        msg: &frame,
        aad: aad.unwrap_or(&[]),
    };

    let ciphertext = cipher.encrypt(&nonce, payload)?;

    // Конкатенируем nonce + ciphertext для простоты передачи
    let mut out = Vec::with_capacity(12 + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

pub fn decrypt_frame_deprecated(
    key: &crate::transport::frame::CryptoState,
    encrypted: &[u8],
    aad: Option<&[u8]>,
) -> anyhow::Result<Vec<u8>> {
    if encrypted.len() < 12 {
        anyhow::bail!("Ciphertext too short");
    }
    let (nonce_bytes, ciphertext) = encrypted.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);

    let cipher = AesCipher::new_from_slice(&key.key)?;
    let payload = aead::Payload {
        msg: ciphertext,
        aad: aad.unwrap_or(&[]),
    };

    Ok(cipher.decrypt(nonce, payload)?)
}

const NONCE_LEN: usize = 12;
//CRYPTO FUNCTIONS

pub fn encrypt_frame_sync(
    plaintext: &[u8],
    state: &crate::transport::frame::CryptoState,
) -> anyhow::Result<Vec<u8>> {
    let cipher = Aes256Gcm::new_from_slice(&state.key)?;
    let counter = state
        .tx_nonce
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut nonce_bytes = [0u8; NONCE_LEN];
    nonce_bytes[4..].copy_from_slice(&counter.to_be_bytes());
    let nonce = Nonce::from(nonce_bytes);
    let ciphertext = cipher.encrypt(&nonce, plaintext)?;
    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

pub fn decrypt_frame_sync(
    ciphertext: &[u8],
    state: &crate::transport::frame::CryptoState,
) -> anyhow::Result<Vec<u8>> {
    if ciphertext.len() < NONCE_LEN {
        anyhow::bail!("Ciphertext too short");
    }
    let (nonce_bytes, ct_with_tag) = ciphertext.split_at(NONCE_LEN);
    let nonce = Nonce::from_slice(nonce_bytes);
    let mut counter_bytes = [0u8; 8];
    counter_bytes.copy_from_slice(&nonce_bytes[4..]);
    let packet_nonce = u64::from_be_bytes(counter_bytes);
    let mut last_seen = state.rx_last_nonce.lock().unwrap();
    if packet_nonce <= *last_seen {
        anyhow::bail!("Replay detected");
    }
    let cipher = Aes256Gcm::new_from_slice(&state.key)?;
    let plaintext = cipher.decrypt(nonce, ct_with_tag)?;
    *last_seen = packet_nonce;
    Ok(plaintext)
}

pub async fn encrypt_frame(
    plaintext: &[u8],
    state: Arc<crate::transport::frame::CryptoState>,
) -> anyhow::Result<Vec<u8>> {
    let state = state.clone();
    let data = plaintext.to_vec();
    tokio::task::spawn_blocking(move || encrypt_frame_sync(&data, &state)).await?
}

pub async fn decrypt_frame(
    ciphertext: &[u8],
    state: Arc<crate::transport::frame::CryptoState>,
) -> anyhow::Result<Vec<u8>> {
    let state = state.clone();
    let data = ciphertext.to_vec();
    tokio::task::spawn_blocking(move || decrypt_frame_sync(&data, &state)).await?
}

use crate::transport::frame::CryptoState;
pub async fn handle_data_loop(
    socket: UdpSocket,
    peer_addr: std::net::SocketAddr,
    assigned_ip: Ipv4Addr,
    tx_to_tun: mpsc::Sender<Vec<u8>>,
    mut peers: Arc<TokioMutex<peers_table>>,
    allocator: Arc<tokio::sync::Mutex<IpAllocator>>,
    cancel: tokio_util::sync::CancellationToken,
    crypto: Arc<CryptoState>,
    state: Arc<std::sync::Mutex<ServerState>>,
    traffic: Arc<TrafficCounters>,
) {
    let mut raw_buf = vec![0u8; 2048];
    let idle_timeout = std::time::Duration::from_secs(120);
    //if let peer = peers.lock().await {
    //    peer.by_user_public_socket.get(&peer_addr);
    //}

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!("Data loop closed for {} client", peer_addr);
                break;
            }

            res = tokio::time::timeout(idle_timeout, socket.recv_from(&mut raw_buf)) => {
                let (len, src) = match res {
                    Ok(Ok(tup)) => tup ,
                    Ok(Err(e)) => {
                        tracing::warn!("UDP recv failed for {}: {}", peer_addr, e);
                        break;
                    }
                    Err(_) => {
                    tracing::warn!("Idle timeout for {}", peer_addr);
                    break;
                    }
                };
                if src != peer_addr { continue; }

                let frame_bytes = match decrypt_frame(&raw_buf[..len], crypto.clone()).await{
                    Ok(v) => {
                        traffic.add_rx(v.len() as u64);
                        v
                    }
                    Err(_) => continue               };

                let frame = match decode_frame(&frame_bytes){
                    Ok(f) => f,
                    Err(e) => {
                        warn!("frame decode failed from {} : {:?}", peer_addr, e);
                        continue;
                    }
                };
                if frame.kind != FrameKind::DATA {
                    continue;
                }


                let pkt_src_ip = match parse_ipv4_src(&frame.payload) {
                    Some(ip) => ip,
                    None => {
                        warn!("Malformed/non-IPv4 packet from {}", peer_addr);
                        continue;
                    }
                };
                if pkt_src_ip != assigned_ip {
                    warn!("spoofing detected!! from {}: src={} expected={}",
                          peer_addr, pkt_src_ip, assigned_ip);
                    break;
                }
                if tx_to_tun.send(frame.payload).await.is_err() {
                    info!("TUN channel closed, stopping {}", peer_addr);
                    break;
                }

                // ... decode_frame → decrypt → anti-spoof → tx_to_tun.send() ...
            }
        }
    }
    if state.lock().unwrap().disconnect_peer(peer_addr).await {
        info!(
            "Peer {} disconnected, Ip {} released to pool.",
            peer_addr, assigned_ip
        )
    } else {
        info!("Peer {} already removed from the table.", peer_addr)
    }
}
async fn read_tcp_frame(
    stream: &mut tokio::net::TcpStream,
    buf: &mut [u8],
) -> std::io::Result<usize> {
    // 1. Читаем ровно 4 байта длины
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let frame_len = u32::from_be_bytes(len_buf) as usize;

    // 2. Защита от OOM / DoS
    if frame_len == 0 || frame_len > buf.len() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Frame too large or malformed",
        ));
    }

    // 3. Читаем ровно payload
    stream.read_exact(&mut buf[..frame_len]).await?;
    Ok(frame_len)
}
pub async fn handle_data_loop_tcp(
    mut stream: tokio::net::TcpStream,
    peer_addr: std::net::SocketAddr,
    assigned_ip: Ipv4Addr,
    crypto: Arc<crate::transport::frame::CryptoState>,
    state: Arc<ServerState>,
    tx_to_tun: mpsc::Sender<Vec<u8>>,
    traffic: Arc<TrafficCounters>,
    cancel: tokio_util::sync::CancellationToken,
) -> anyhow::Result<()> {
    let mut buf = vec![0u8; 65536];
    let idle_timeout = std::time::Duration::from_secs(120);

    tracing::info!(
        "TCP data loop started for {} (IP: {})",
        peer_addr,
        assigned_ip
    );

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                tracing::info!("TCP data loop cancelled for {}", peer_addr);
                break;
            }

            // Чтение одного полного фрейма с таймаутом простоя
            res = tokio::time::timeout(idle_timeout, read_tcp_frame(&mut stream, &mut buf)) => {
                match res {
                    Ok(Ok(frame_len)) => {
                        traffic.add_rx(frame_len as u64);

                        // 1. Расшифровка
                        let plaintext = match decrypt_frame(&buf[..frame_len], crypto.clone()).await {
                            Ok(p) => p,
                            Err(e) => { tracing::warn!("Decrypt failed for {}: {}", peer_addr, e); continue; }
                        };

                        // 2. Декодирование фрейма
                        let frame = match decode_frame(&plaintext) {
                            Ok(f) => f,
                            Err(e) => { tracing::warn!("Decode failed for {}: {:?}", peer_addr, e); continue; }
                        };

                        // 3. Фильтр: обрабатываем только DATA
                        if frame.kind != FrameKind::DATA {
                            continue;
                        }

                        // 4. Анти-спуфинг: проверяем, что src_ip == выданный IP
                        let pkt_src = match parse_ipv4_src(&frame.payload) {
                            Some(ip) => ip,
                            None => { tracing::warn!("Malformed/non-IPv4 packet from {}", peer_addr); continue; }
                        };
                        if pkt_src != assigned_ip {
                            tracing::warn!("SPOOFING DETECTED from {}! Expected {}, got {}", peer_addr, assigned_ip, pkt_src);
                            break; // Принудительный разрыв
                        }

                        // 5. Отправка в TUN (для маршрутизации в интернет)
                        if tx_to_tun.send(frame.payload).await.is_err() {
                            tracing::info!("TUN channel closed, stopping {}", peer_addr);
                            break;
                        }
                    }

                    Ok(Err(e)) => {
                        tracing::warn!("TCP read error for {}: {}", peer_addr, e);
                        break;
                    }

                    Err(_) => {
                        tracing::warn!("Idle timeout for {} ({}s)", peer_addr, idle_timeout.as_secs());
                        break;
                    }
                }
            }
        }
    }
    if state.disconnect_peer(peer_addr).await {
        tracing::info!(
            "Peer {} disconnected, IP {} released to pool.",
            peer_addr,
            assigned_ip
        );
    }
    Ok(())
}
pub async fn tun_write_all(
    mut rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    tun: Arc<tokio::sync::Mutex<TunInterface>>,
    cancel: tokio_util::sync::CancellationToken,
) {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                break;
            },

            pkt = rx.recv() => match pkt {
                Some(mut packet) => {
                    let tun_arc = tun.clone();

                    tokio::task::spawn_blocking(move|| {
                            let guard = tun_arc
                            .blocking_lock();
                        guard.write_packet(packet.as_mut_slice());
                    }).await.unwrap_or_else(|e| tracing::error!("Tun write panicked {}", e));
                }
                None => break,
            }
        }
    }
}
//pub async fn tun_to_udp_loop(
//    interface: TunInterface,
//    socket: &UdpSocket,
//    session: &Session,
//    mut rx: mpsc::Receiver<Vec<u8>>,
//    cancel: tokio_util::sync::CancellationToken,
//) -> anyhow::Result<()> {
//    let mut buf = vec![0u8; 2048];
//    let packet = interface.read_packet(&mut buf).await?;
//}
fn parse_ipv4_src(buf: &[u8]) -> Option<std::net::Ipv4Addr> {
    if buf.len() >= 20 && (buf[0] >> 4) == 4 {
        return Some(std::net::Ipv4Addr::new(buf[12], buf[13], buf[14], buf[15]));
    } else {
        None
    }
}
fn parse_ipv4_dst(buf: &[u8]) -> Option<std::net::Ipv4Addr> {
    if buf.len() >= 20 && (buf[0] >> 4) == 4 {
        return Some(std::net::Ipv4Addr::new(buf[16], buf[17], buf[18], buf[19]));
    } else {
        return None;
    }
}
pub async fn tun_reader_loop(
    tun: Arc<TokioMutex<TunInterface>>,
    //peers: Arc<TokioMutex<peers_table>>,
    state: Arc<ServerState>,
    socket: tokio::net::UdpSocket,
    cancel: tokio_util::sync::CancellationToken,
    traffic: Arc<TrafficCounters>,
) {
    let mut buf = vec![0u8; 1500];
    info!("TUN reader started");
    let guard = tun.lock().await;
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            res = guard.read_packet(&mut buf) => {
                let n = match res {
                    Ok(len) => match len{
                        0 => {
                            tracing::warn!("Tun device returned EOF(interface is closed"); break;}
                        n => {
                            tracing::debug!("Tun read {} bytes, first byte: {}", len, buf[0]);
                            n
                        }
                    }
                    Ok(0) => {
                        tracing::warn!("TUN EOF (interface closed)");
                        break;
                    }
                    Err(e) => {
                        tracing::warn!("TUN read error: {}", e);
                        continue;
                    }
            };

                let raw_ip = &buf[..n];

                let dst_ip = match parse_ipv4_dst(raw_ip) {
                    Some(ip) => ip,
                    None => continue,
                };

                let (peer_crypto, peer_addr, peer) = {
                    match state.peers.lock().await.get_by_ip(&dst_ip) {
                        Some(p) => (p.crypto.clone(), p.public_socket, p),
                        None => continue,
                    }
                };

                let frame = encode_frame(FrameKind::DATA, peer.session_id, raw_ip);
                let encrypted = encrypt_frame(&frame, peer_crypto).await.unwrap();
                match encrypted {
                    enc => {
                        traffic.add_tx(enc.len() as u64);
                        //Отправляем через канал, если это TCP-пир
                        if let Some(tx) = &peer.tx_reply {
                            let _ = tx.send(enc).await;
                        } else {
                        //Или через UDP-сокет
                            let _ = socket.send_to(&enc, peer_addr).await;
                        }
                    }
                }


            }
        }
    }
}
pub async fn run_tcp_server(
    listen_addr: std::net::SocketAddr,
    state: Arc<ServerState>,
    tx_to_tun: mpsc::Sender<Vec<u8>>,
    traffic: Arc<TrafficCounters>,
) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(listen_addr).await?;
    info!("TCP server listening on {}", listen_addr);

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        stream.set_nodelay(true)?;
        let tun = tx_to_tun.clone();
        let state = state.clone();
        let traffic = traffic.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_tcp_peer(stream, peer_addr, state, tun, traffic).await {
                warn!("TCP peer {} error: {}", peer_addr, e);
            }
        });
    }
}
