use crate::transport::frame::{DecodeError, DecodedFrame, FrameKind, decode_frame, encode_frame};
use tokio::net::UdpSocket;
use vpn_types::VpnProfile;
#[derive(Debug)]
pub struct Token {
    pub token: String,
}
impl Token {
    pub fn fill_token_vless(&mut self, profile: &VpnProfile) {
        self.token = profile.uuid.clone();
    }
    pub fn new() -> Self {
        Self {
            token: String::from(""),
        }
    }
}
pub async fn connect_udp(server_addr: String) -> std::io::Result<tokio::net::UdpSocket> {
    let local_port = "0.0.0.0:0";
    let socket = UdpSocket::bind(local_port).await?;
    socket.connect(server_addr).await?;
    Ok(socket)
}

#[derive(Debug)]
pub enum HelloAckError {
    Io(std::io::Error),
    Decode(DecodeError),
    UnexpectedKind(FrameKind),
    BadSessionId(u64),
    TokenMismatch(String),
    InvalidTokenEncoding,
}

impl From<std::io::Error> for HelloAckError {
    fn from(err: std::io::Error) -> Self {
        HelloAckError::Io(err)
    }
}

impl From<DecodeError> for HelloAckError {
    fn from(err: DecodeError) -> Self {
        HelloAckError::Decode(err)
    }
}
pub async fn send_hello(socket: &UdpSocket, token: &Token, session_id: u64) -> std::io::Result<()> {
    let kind = FrameKind::HELLO;
    let token = token.token.as_bytes();
    let payload: &[u8] = token;
    let frame = encode_frame(kind, session_id, payload);
    let frame_s = frame.as_slice();
    socket.send(frame_s).await?;
    Ok(())
}
pub async fn recv_hello_ack(socket: &UdpSocket, session_id: u64) -> Result<(), HelloAckError> {
    let mut buf = vec![0u8; 2048];

    let res = socket.recv(&mut buf).await?;
    let frame = decode_frame(&buf[..res])?;

    if frame.kind != FrameKind::HELLOACK {
        return Err(HelloAckError::UnexpectedKind(frame.kind));
    }

    if frame.session_id != session_id {
        return Err(HelloAckError::BadSessionId(frame.session_id));
    }

    Ok(())
}
pub async fn send_data(socket: &UdpSocket, data: Vec<u8>, session_id: u64) -> std::io::Result<()> {
    let kind = FrameKind::DATA;
    let payload = data.as_slice();
    let frame = encode_frame(kind, session_id, payload);
    socket.send(&frame).await?;
    Ok(())
}
pub async fn send_keepalive(socket: &UdpSocket, session_id: u64) -> std::io::Result<()> {
    let kind = FrameKind::KEEPALIVE;
    let payload = vec![0u8; 0];
    let frame = encode_frame(kind, session_id, &payload);
    socket.send(&frame).await?;
    Ok(())
}
