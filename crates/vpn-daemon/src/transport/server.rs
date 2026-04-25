use crate::linux::routing::{
    ClientNetworkConfig, RouteConfig, ServerNetworkConfig, TransportConfig,
};
use crate::linux::tun::{TunFd, TunInterface, create_interface};
use crate::transport::client::{HelloAckError, Token};
use crate::transport::frame::{
    DecodeError, DecodedFrame, FrameKind, Session, decode_frame, encode_frame,
};
use std::collections::HashSet;
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};
use tokio::net::UdpSocket;
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
#[derive(Eq, Hash, PartialEq, Clone, Copy)]
pub struct Peer {
    pub user_ip: Ipv4Addr,
    pub public_socket: std::net::SocketAddr,
    pub last_seen: std::time::Instant,
}
impl Peer {
    pub fn new(ip: Ipv4Addr, sock: std::net::SocketAddr, time: std::time::Instant) -> Self {
        Self {
            user_ip: ip,
            public_socket: sock,
            last_seen: time,
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
pub struct ServerState {
    pub allocator: Mutex<IpAllocator>,
    pub peers: peers_table,
}
impl ServerState {
    pub async fn connect_peer(&mut self, socket: std::net::SocketAddr) -> Option<Ipv4Addr> {
        let ip = self.allocator.lock().unwrap().allocate()?;
        let peer = Peer::new(ip, socket, std::time::Instant::now());
        self.peers.put_peer_and_socket(socket, peer);
        Some(ip)
    }
    pub async fn disconnect_peer(&mut self, socket: std::net::SocketAddr) -> Option<()> {
        if let peer = self.peers.by_user_public_socket.get(&socket) {
            self.allocator
                .lock()
                .unwrap()
                .release(peer.unwrap().user_ip);
        }
        Some(())
    }
}
pub struct peers_table {
    pub by_user_ip: std::collections::HashMap<std::net::Ipv4Addr, Peer>,
    pub by_user_public_socket: std::collections::HashMap<std::net::SocketAddr, Peer>,
}
impl peers_table {
    pub fn put_peer_and_ip(&mut self, ip: std::net::Ipv4Addr, peer: Peer) {
        self.by_user_ip.insert(ip, peer);
    }
    pub fn put_peer_and_socket(&mut self, socket: std::net::SocketAddr, peer: Peer) {
        self.by_user_public_socket.insert(socket, peer);
    }
    pub fn new() -> Self {
        Self {
            by_user_ip: std::collections::HashMap::new(),
            by_user_public_socket: std::collections::HashMap::new(),
        }
    }
    pub fn remove(&mut self, socket: std::net::SocketAddr) -> std::io::Result<Peer> {
        let peer = self.by_user_public_socket.remove(&socket);
        Ok(peer.unwrap())
    }
}
pub async fn handle_hello(
    socket: &UdpSocket,
    token: Token,
    session: &mut Session,
    mut peers: peers_table,
) -> Result<(), HelloAckError> {
    let mut buf = vec![0u8; 2048];
    let (res, peer_adress) = socket.recv_from(&mut buf).await?;
    let ready = decode_frame(&buf[..res])?;
    if ready.kind != FrameKind::HELLO {
        return Err(HelloAckError::UnexpectedKind(ready.kind));
    }
    let payload_token =
        std::str::from_utf8(&ready.payload).map_err(|c| HelloAckError::InvalidTokenEncoding)?;
    if payload_token != token.token {
        return Err(HelloAckError::TokenMismatch(payload_token.to_string()));
    }

    session.session_id = ready.session_id;
    session.authentificated = true;
    session.peer_addr = Some(peer_adress);
    let ip = parse_ipv4_src(&mut buf);
    let peer = Peer::new(ip.unwrap(), peer_adress, std::time::Instant::now());
    peers.put_peer_and_socket(peer_adress, peer);
    Ok(())
}
use std::sync::mpsc;
pub async fn send_helloack(socket: &UdpSocket, session: &Session) -> std::io::Result<()> {
    let kind = FrameKind::HELLOACK;
    let session_id = session.session_id;
    let mut frame = encode_frame(kind, session_id, &mut vec![]);
    socket.send(&mut frame).await;
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
pub fn encrypt_frame(
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

pub fn decrypt_frame(
    key: &Key<Aes256Gcm>,
    encrypted: &[u8],
    aad: Option<&[u8]>,
) -> anyhow::Result<Vec<u8>> {
    if encrypted.len() < 12 {
        anyhow::bail!("Ciphertext too short");
    }
    let (nonce_bytes, ciphertext) = encrypted.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);

    let cipher = AesCipher::new(key);
    let payload = aead::Payload {
        msg: ciphertext,
        aad: aad.unwrap_or(&[]),
    };

    Ok(cipher.decrypt(nonce, payload)?)
}

pub async fn handle_data_loop(
    socket: UdpSocket,
    peer_addr: std::net::SocketAddr,
    assigned_ip: Ipv4Addr,
    tx_to_tun: mpsc::Sender<Vec<u8>>,
    mut peers: peers_table,
    allocator: Arc<tokio::sync::Mutex<IpAllocator>>,
    cancel: tokio_util::sync::CancellationToken,
    interface: TunInterface,
) {
    let mut raw_buf = vec![0u8; 2048];
    let mut frame_buf: Vec<u8> = Vec::with_capacity(2048);
    let idle_timeout = std::time::Duration::from_secs(120);

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
                let frame = match decode_frame(&raw_buf[..len]){
                    Ok(f) => f,
                    Err(e) => {
                        warn!("frame decode failed from {} : {:?}", peer_addr, e);
                        break;
                    }
                };

                if frame.kind != FrameKind::DATA {
                    warn!("frame kind {:?} does not match DATA ", frame.kind);
                    break;
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
                if tx_to_tun.send(frame.payload).is_err() {
                    info!("TUN channel closed, stopping {}", peer_addr);
                    break;
                }

                // ... decode_frame → decrypt → anti-spoof → tx_to_tun.send() ...
            }
        }
    }
    tracing::info!("Cleaning up peer {}", peer_addr);
    if let peer = peers.remove(peer_addr).unwrap() {
        allocator.lock().await.release(peer.user_ip);
        tracing::info!("IP {} returned to pool", peer.user_ip);
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
