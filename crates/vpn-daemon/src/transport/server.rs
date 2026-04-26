use crate::linux::routing::{
    ClientNetworkConfig, RouteConfig, ServerNetworkConfig, TransportConfig,
};
use crate::linux::tun::{TunFd, TunInterface, create_interface};
use crate::transport::client::{HelloAckError, Token};
use crate::transport::frame::{DecodeError, DecodedFrame, FrameKind, decode_frame, encode_frame};
use std::collections::HashSet;
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use vpn_types::VpnProfile;
extern crate scopeguard;
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
#[derive(Clone)]
pub struct Peer {
    pub user_ip: Ipv4Addr,
    pub public_socket: std::net::SocketAddr,
    pub last_seen: std::time::Instant,
    pub crypto: Arc<CryptoState>,
    pub user_id: String,
    pub session_id: u64,
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
        Self {
            user_ip: ip,
            public_socket: sock,
            last_seen: time,
            crypto: crypto_cx,
            user_id: user_id_rx,
            session_id: session_id,
        }
    }
}
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
use tokio::sync::Mutex as TokioMutex;
pub struct ServerState {
    pub allocator: TokioMutex<IpAllocator>,
    pub peers: TokioMutex<peers_table>,
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
    pub async fn disconnect_peer(&mut self, socket: std::net::SocketAddr) -> bool {
        let mut peers = self.peers.lock().await;
        if let Some(peer) = peers.remove(&socket) {
            self.allocator.lock().await.release(peer.user_ip);
            true
        } else {
            false
        }
    }
    pub fn new(subnet: Ipv4Addr, pool_size: u32) -> Self {
        Self {
            allocator: tokio::sync::Mutex::new(IpAllocator::new(subnet, pool_size)),
            peers: tokio::sync::Mutex::new(peers_table::new()),
        }
    }
}
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
pub async fn handle_hello(
    socket: &UdpSocket,
    server_token: &Token,
    mut peers: std::sync::Arc<TokioMutex<peers_table>>,
    allocator: &Arc<tokio::sync::Mutex<IpAllocator>>,
    state: &Arc<ServerState>,
) -> Result<(), HelloAckError> {
    let mut buf = vec![0u8; 2048];
    let (res, peer_adress) = socket.recv_from(&mut buf).await?;
    let ready = decode_frame(&buf[..res])?;
    if ready.kind != FrameKind::HELLO {
        return Err(HelloAckError::UnexpectedKind(ready.kind));
    }
    let payload_token =
        std::str::from_utf8(&ready.payload).map_err(|c| HelloAckError::InvalidTokenEncoding)?;
    if payload_token != server_token.token {
        return Err(HelloAckError::TokenMismatch(payload_token.to_string()));
    }
    let user_id = payload_token.to_string();
    state.connect_peer(peer_adress, user_id);
    send_helloack(socket, peer_adress, peers);
    Ok(())
}

pub fn generate_crypto_state() -> std::io::Result<Arc<CryptoState>> {
    let mut key = [0u8; 32];
    getrandom::fill(&mut key);

    Ok(Arc::new(CryptoState {
        key: key,
        tx_nonce: std::sync::atomic::AtomicU64::new(0),
        rx_last_nonce: Arc::new(Mutex::new(0 as u64)),
        cipher_type: crate::transport::frame::CipherAlg::AesGcm,
    }))
}
pub async fn send_helloack(
    socket: &UdpSocket,
    peer_addr: std::net::SocketAddr,
    peers: Arc<TokioMutex<peers_table>>,
) -> std::io::Result<()> {
    let kind = FrameKind::HELLOACK;
    let peer = peers.lock().await.get_by_addr(&peer_addr).unwrap();
    let session_id = peer.session_id;
    let mut payload = vec![];
    let mut frame = encode_frame(kind, session_id, &mut payload);
    socket.send_to(&mut frame, peer_addr).await?;
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
) {
    let mut raw_buf = vec![0u8; 2048];
    let idle_timeout = std::time::Duration::from_secs(120);
    if let peer = peers.lock().await {
        peer.by_user_public_socket.get(&peer_addr);
    }

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
                    Ok(v) => v,
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
pub async fn tun_write_all(
    mut rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    tun: Arc<std::sync::Mutex<TunInterface>>,
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

                    tokio::task::spawn_blocking(move || {
                            let guard = tun_arc
                            .lock()
                            .expect("Tun poisoned");
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
    tun: Arc<TunInterface>,
    peers: Arc<TokioMutex<peers_table>>,
    socket: tokio::net::UdpSocket,
    cancel: tokio_util::sync::CancellationToken,
) {
    let mut buf = vec![0u8; 1500];
    info!("TUN reader started");

    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,

            res = tun.read_packet(&mut buf) => {
                let n = match res {
                    Ok(len) => match len{
                        0 => {
                            tracing::warn!("Tun device returned EOF(interface is closed"); break;}
                        n => n,
                    },
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
                    match peers.lock().await.get_by_ip(&dst_ip) {
                        Some(p) => (p.crypto.clone(), p.public_socket, p),
                        None => continue,
                    }
                };

                let frame = encode_frame(FrameKind::DATA, peer.session_id, raw_ip);
                let encrypted = match encrypt_frame(&frame, peer_crypto).await {
                    Ok(e) => e,
                    Err(_) => continue,
                };

                let _ = socket.send_to(&encrypted, peer_addr).await;
            }
        }
    }
}
