use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use vpn_types::Transport;
pub enum ActiveTransport {
    Udp {
        socket: UdpSocket,
        server_addr: SocketAddr,
    },
    Tcp {
        stream: TcpStream,
    },
}
pub trait AsyncTransport {
    type Error: std::error::Error + Send + Sync + 'static;

    async fn send_frame(&mut self, buf: &[u8]) -> Result<usize, Self::Error>;
    async fn recv_frame(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error>;
    fn transport_type(&self) -> &'static str;
}
impl AsyncTransport for ActiveTransport {
    type Error = std::io::Error;
    async fn send_frame(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        match self {
            ActiveTransport::Udp {
                socket,
                server_addr,
            } => match socket.send_to(buf, *server_addr).await {
                Ok(n) => Ok(n),
                Err(e) => {
                    tracing::error!("Udp send failed: {}", e);
                    Err(e)
                }
            },
            ActiveTransport::Tcp { stream } => {
                let len = buf.len() as u32;
                stream.write_all(&len.to_be_bytes()).await?;
                stream.write_all(buf).await?;
                Ok(buf.len())
            }
        }
    }
    async fn recv_frame(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        match self {
            ActiveTransport::Udp { socket, .. } => match socket.recv(buf).await {
                Ok(n) => Ok(n),
                Err(e) => {
                    tracing::error!("Udp recv failed: {}", e);
                    Err(e)
                }
            },
            ActiveTransport::Tcp { stream } => {
                let mut len_buf = [0u8; 4];
                stream.read_exact(&mut len_buf).await?;
                let frame_len = u32::from_be_bytes(len_buf) as usize;
                if frame_len > 65535 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Frame too large",
                    ));
                }
                if frame_len > buf.len() {
                    return Err(std::io::ErrorKind::InvalidData.into());
                }
                stream.read_exact(&mut buf[..frame_len]).await?;
                Ok(frame_len)
            }
        }
    }
    fn transport_type(&self) -> &'static str {
        match self {
            ActiveTransport::Udp { .. } => "Using udp protocol for transport.",
            ActiveTransport::Tcp { .. } => "Using tcp protocol for transport",
        }
    }
}
impl ActiveTransport {
    pub async fn connect(
        host: &str,
        port: u16,
        mode: Transport,
        timeout: std::time::Duration,
    ) -> Result<Self, std::io::Error> {
        let addr_str = format!("{host}:{port}");
        let server_addr = tokio::time::timeout(timeout, async {
            if let Ok(ip) = addr_str.parse::<SocketAddr>() {
                return Ok(ip);
            }
            tokio::net::lookup_host(&addr_str)
                .await?
                .find(|a| a.is_ipv4())
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "DNS Failed"))
        })
        .await??;

        match mode {
            Transport::Udp => Self::try_udp(server_addr, timeout).await,
            Transport::Tcp => Self::try_tcp(server_addr, timeout).await,
            Transport::Auto => match Self::try_udp(server_addr, timeout).await {
                Ok(t) => Ok(t),
                Err(e) => {
                    tracing::warn!("Udp protocol failed to conduct: {}. Trying use TCP.", e);
                    Self::try_tcp(server_addr, timeout).await
                }
            },
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Unavailable protocol",
            )),
        }
    }
    async fn try_udp(
        server_addr: SocketAddr,
        timeout: std::time::Duration,
    ) -> tokio::io::Result<Self> {
        let socket: UdpSocket = tokio::time::timeout(timeout, async {
            let s = UdpSocket::bind("0.0.0.0:0").await.unwrap();
            s.connect(server_addr).await.unwrap();
            Ok::<_, std::io::Error>(s)
        })
        .await??;
        Ok(ActiveTransport::Udp {
            socket,
            server_addr,
        })
    }
    async fn try_tcp(
        server_addr: SocketAddr,
        duration: std::time::Duration,
    ) -> std::io::Result<Self> {
        let stream = tokio::time::timeout(duration, TcpStream::connect(server_addr)).await??;
        stream.set_nodelay(true)?;
        Ok(ActiveTransport::Tcp { stream })
    }
}
