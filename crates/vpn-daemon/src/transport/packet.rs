use anyhow::{Result, anyhow};

pub const MAGIC: [u8; 4] = *b"XVPN";
pub const VERSION: u8 = 1;
pub const HEADER_LEN: usize = 4 + 1 + 2;

pub fn encode_packet(payload: &[u8]) -> Result<Vec<u8>> {
    if payload.len() > u16::MAX as usize {
        return Err(anyhow!("payload too large"));
    }

    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    out.extend_from_slice(&MAGIC);
    out.push(VERSION);

    out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    out.extend_from_slice(payload);
    Ok(out)
}
pub fn decode_packet(buf: &[u8]) -> Result<&[u8]> {
    if buf.len() < HEADER_LEN {
        return Err(anyhow!("packet too short"));
    }

    if buf[0..4] != MAGIC {
        return Err(anyhow!("bad magic"));
    }

    if buf[4] != VERSION {
        return Err(anyhow!("bad version"));
    }

    let len = u16::from_be_bytes([buf[5], buf[6]]) as usize;
    if buf.len() != HEADER_LEN + len {
        return Err(anyhow!("bad length"));
    }

    Ok(&buf[HEADER_LEN..])
}
