use crate::transport::singbox::SingBoxSidecar;
use crate::transport::singbox::socks5_connect;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::Mutex;
use tokio_rustls::{TlsConnector, client::TlsStream};
use vpn_types::Transport;

pub enum ActiveTransport {
    Udp {
        socket: UdpSocket,
        server_addr: SocketAddr,
    },
    Tcp {
        stream: TcpStream,
    },
    VlessTls {
        stream: tokio_rustls::client::TlsStream<TcpStream>,
    },
}
pub trait AsyncTransport {
    type Error: std::error::Error + Send + Sync + 'static;

    async fn send_frame(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error>;
    async fn recv_frame(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error>;
    fn transport_type(&self) -> &'static str;
}
impl AsyncTransport for ActiveTransport {
    type Error = std::io::Error;
    async fn send_frame(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
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
            ActiveTransport::VlessTls { stream } => Self::write_framed(stream, buf).await,
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
            ActiveTransport::VlessTls { stream } => Self::read_framed(stream, buf).await,
        }
    }
    fn transport_type(&self) -> &'static str {
        match self {
            ActiveTransport::Udp { .. } => "Using udp protocol for transport.",
            ActiveTransport::Tcp { .. } => "Using tcp protocol for transport",
            ActiveTransport::VlessTls { .. } => "Using VLESS/TLS protocol for transport",
        }
    }
}
impl ActiveTransport {
    pub async fn connect(
        host: &str,
        port: u16,
        mode: Transport,
        timeout: std::time::Duration,
        uuid: Option<&str>,
        sni: Option<String>,
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
            Transport::VlessTls => {
                let u = uuid.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "UUID required for VLESS")
                })?;

                Self::try_vless_tls(host, port, u, timeout, sni).await
            }
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
    async fn try_vless_tls(
        host: &str,
        port: u16,
        uuid: &str,
        timeout: std::time::Duration,
        sni: Option<String>,
    ) -> std::io::Result<Self> {
        tokio::time::timeout(timeout, async {
            // 1. TCP Connect
            let tcp = TcpStream::connect(format!("{host}:{port}")).await?;
            tcp.set_nodelay(true)?;

            // 2. TLS Config
            let mut root_store = rustls::RootCertStore::empty();
            root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            let config = std::sync::Arc::new(
                rustls::ClientConfig::builder()
                    .with_root_certificates(root_store)
                    .with_no_client_auth(),
            );

            // 3. TLS Handshake
            let connector = TlsConnector::from(config);
            let domain = rustls::pki_types::ServerName::try_from(sni.unwrap())
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
            let mut tls_stream = connector.connect(domain, tcp).await?;

            // 4. Send VLESS Header (после хендшейка, до моих фреймов)
            let header = build_vless_tcp_header(uuid, host, port);
            tls_stream.write_all(&header).await?;
            tls_stream.flush().await?;
            let typed_tls: tokio_rustls::client::TlsStream<TcpStream> = tls_stream;
            Ok(ActiveTransport::VlessTls { stream: typed_tls })
        })
        .await?
    }
    async fn write_framed<W>(stream: &mut W, buf: &[u8]) -> std::io::Result<usize>
    where
        W: AsyncWrite + Unpin,
    {
        let len = buf.len() as u32;
        if len > 65535 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Frame too large",
            ));
        }
        stream.write_all(&len.to_be_bytes()).await?;
        stream.write_all(buf).await?;
        Ok(buf.len())
    }

    async fn read_framed<R>(stream: &mut R, buf: &mut [u8]) -> std::io::Result<usize>
    where
        R: AsyncRead + Unpin,
    {
        let mut len_buf = [0u8; 4];
        let mut debug_buf = [0u8; 20];
        stream.read_exact(&mut debug_buf).await?;
        tracing::debug!("First 20 bytes of stream: {:02x?}", debug_buf);
        stream.read_exact(&mut len_buf).await?;
        let frame_len = u32::from_be_bytes(len_buf) as usize;

        if frame_len == 0 || frame_len > 65535 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Frame too large/malformed",
            ));
        }
        if buf.len() < frame_len {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Receive buffer too small",
            ));
        }
        stream.read_exact(&mut buf[..frame_len]).await?;
        Ok(frame_len)
    }
}
fn build_vless_tcp_header(uuid: &str, host: &str, port: u16) -> Vec<u8> {
    let mut buf = Vec::with_capacity(64);
    buf.push(0x00); // version
    buf.extend_from_slice(
        uuid::Uuid::parse_str(uuid)
            .expect("Invalid VLESS UUID")
            .as_bytes(),
    );
    buf.push(0x00); // addons length
    buf.push(0x01); // command: TCP
    buf.push(0x02); // address type: Domain
    let host_bytes = host.as_bytes();
    buf.push(host_bytes.len() as u8);
    buf.extend_from_slice(host_bytes);
    buf.extend_from_slice(&port.to_be_bytes());
    buf
}
async fn resolve_addr(
    host: &str,
    port: u16,
    timeout: std::time::Duration,
) -> std::io::Result<SocketAddr> {
    let addr_str = format!("{host}:{port}");
    tokio::time::timeout(timeout, async {
        if let Ok(ip) = addr_str.parse::<SocketAddr>() {
            return Ok(ip);
        }
        tokio::net::lookup_host(&addr_str)
            .await?
            .find(|a| a.is_ipv4())
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "DNS resolution failed")
            })
    })
    .await?
}
pub struct RealityTransport {
    pub stream: TcpStream,
    pub _sidecar: Arc<Mutex<Option<SingBoxSidecar>>>, // Держим sidecar живым
}

impl RealityTransport {
    pub async fn connect(
        remote_host: &str,
        port: u16,
        uuid: &str,
        sni: &str,
        pbk: &str,
        sid: &str,
        fp: &str,
        transport: Transport,
    ) -> std::io::Result<Self> {
        // 1. Запускаем sidecar
        let sidecar = SingBoxSidecar::start(transport, remote_host, port, uuid, sni, pbk, sid, fp)
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        let sidecar_arc = Arc::new(Mutex::new(Some(sidecar)));
        let local_port = sidecar_arc.lock().await.as_ref().unwrap().local_port();

        // 2. Подключаемся к локальному SOCKS
        let local_stream = TcpStream::connect(format!("127.0.0.1:{}", local_port)).await?;
        let target = format!("{remote_host}:{port}");
        let tunneled = socks5_connect(local_stream, &target)
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        tracing::info!(
            "Reality tunnel established (via sing-box local:{})",
            local_port
        );
        Ok(Self {
            stream: tunneled,
            _sidecar: sidecar_arc,
        })
    }
}
