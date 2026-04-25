use crate::linux::tun::{TunInterface, create_interface};
#[derive(Debug)]
pub enum DecodeError {
    TooShort,
    BadMagic,
    UnsupportedVersion(u8),
    UnknownKind(u8),
    HelloAckError,
}
#[derive(Debug, Clone)]
pub struct Session {
    pub session_id: u64,
    pub authentificated: bool,
    pub peer_addr: Option<std::net::SocketAddr>,
}
impl Session {
    pub fn new() -> Self {
        Self {
            session_id: 0,
            authentificated: false,
            peer_addr: None,
        }
    }
}
#[derive(Debug, Clone)]
pub struct DecodedFrame {
    pub kind: FrameKind,
    pub session_id: u64,
    pub payload: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameKind {
    HELLO = 1,
    ERROR = 2,
    DATA = 3,
    HELLOACK = 4,
    KEEPALIVE = 5,
}
pub fn encode_frame(kind: FrameKind, session_id: u64, payload: &[u8]) -> Vec<u8> {
    let session_en = session_id.to_be_bytes();
    let mut payload_en = Vec::new();
    payload_en.extend_from_slice(b"XTVPN");
    payload_en.push(1);
    payload_en.push(kind as u8);
    payload_en.extend_from_slice(&session_en);
    payload_en.extend_from_slice(payload);
    payload_en
}
const MAGIC: &[u8; 5] = b"XTVPN";
const VERSION: u8 = 1;
const HEADER_LEN: usize = 15;
pub fn decode_frame(buf: &[u8]) -> Result<DecodedFrame, DecodeError> {
    if buf.len() < HEADER_LEN {
        return Err(DecodeError::TooShort);
    }

    if &buf[..5] != MAGIC {
        return Err(DecodeError::BadMagic);
    }

    let version = buf[5];
    if version != VERSION {
        return Err(DecodeError::UnsupportedVersion(version));
    }

    let kind = match buf[6] {
        1 => FrameKind::HELLO,
        2 => FrameKind::ERROR,
        3 => FrameKind::DATA,
        4 => FrameKind::HELLOACK,
        5 => FrameKind::KEEPALIVE,
        other => return Err(DecodeError::UnknownKind(other)),
    };

    let session_id = u64::from_be_bytes([
        buf[7], buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14],
    ]);
    let payload = buf[HEADER_LEN..].to_vec();

    Ok(DecodedFrame {
        session_id: session_id,
        payload: payload,
        kind: kind,
    })
}
