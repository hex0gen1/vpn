use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::json;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::process::{Child, Command};
#[derive(Serialize)]
struct SingBoxConfig {
    log: LogConfig,
    dns: DnsConfig,
    inbounds: Vec<SocksInbound>,
    outbounds: Vec<VlessRealityOutbound>,
    route: RouteConfig,
}

#[derive(Serialize)]
struct LogConfig {
    level: String,
}
#[derive(Serialize)]
struct DnsConfig {
    servers: Vec<String>,
}
#[derive(Serialize)]
struct RouteConfig {
    rules: Vec<RouteRule>,
}
#[derive(Serialize)]
struct RouteRule {
    inbound: Vec<String>,
    outbound: String,
}

#[derive(Serialize)]
struct SocksInbound {
    #[serde(rename = "type")]
    kind: String,
    tag: String,
    listen: String,
    listen_port: u16,
}

#[derive(Serialize)]
struct VlessRealityOutbound {
    #[serde(rename = "type")]
    kind: String,
    tag: String,
    server: String,
    server_port: u16,
    uuid: String,
    packet_encoding: String,
    tls: TlsReality,
}

#[derive(Serialize)]
struct TlsReality {
    enabled: bool,
    server_name: String,
    reality: RealityParams,
}

#[derive(Serialize)]
struct RealityParams {
    enabled: bool,
    public_key: String,
    short_id: String,
}

pub struct SingBoxSidecar {
    child: Child,
    local_port: u16,
}

impl SingBoxSidecar {
    pub async fn start(
        transport: vpn_types::Transport,
        remote_host: &str,
        port: u16,
        uuid: &str,
        sni: &str,
        pbk: &str,
        sid: &str,
        fp: &str,
    ) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let local_port = listener.local_addr()?.port();
        drop(listener);
        let transport_f = transport.as_str();
        let config_json = json!({
        "log": {
        "level": "info",
        "timestamp": true
        },
        "dns": {
        "servers": [
        {
        "type":transport_f,
        "tag": "proxy-dns",
        "server": "8.8.8.8",
        "server_port": 53,
        },
        {
        "type": transport_f,
        "tag": "direct-dns",
        "server": "77.88.8.8",
        "server_port": 53,
        },
        {
        "type": "local",
        "tag": "local"
        }
        ],
        "rules": [
        {
        "domain_suffix": [".ru", ".su"],
        "server": "direct-dns"
        }
        ],
        "strategy": "prefer_ipv4"
        },
        "inbounds": [
        {
        "type": "tun",
        "tag": "tun-in",
        "mtu": 1280,
        "address": ["172.19.0.1/30"],
        "auto_route": true,
        "strict_route": true,
        "stack": "system"
        }
        ],
        "outbounds": [
        {
        "type": "vless",
        "tag": "proxy",
        "server": remote_host,
        "server_port": port,
        "uuid": uuid,
        "flow": "xtls-rprx-vision",
        "tls": {
        "enabled": true,
        "server_name": sni,
        "utls": {
        "enabled": true,
        "fingerprint": fp
        },
        "reality": {
        "enabled": true,
        "public_key": pbk,
        "short_id": sid
        }
        }
        },
        {
        "type": "direct",
        "tag": "direct"
        }
        ],
        "route": {
        "rules": [
        {
        "action": "sniff"
        },
        {
        "protocol": "dns",
        "action": "hijack-dns"
        },
        {
        "ip_is_private": true,
        "outbound": "direct"
        },
        {
        "domain_suffix": [
        ".ru", ".su",
        ".yandex.com", ".vk.com", ".mail.ru",
        ".gosuslugi.ru", ".sberbank.ru"
        ],
        "outbound": "direct"
        }
        ],
        "auto_detect_interface": true,
        "default_domain_resolver": "local"
        }
        });
        let config = SingBoxConfig {
            log: LogConfig {
                level: "warn".into(),
            },
            dns: DnsConfig {
                servers: vec!["local".into()],
            },
            inbounds: vec![SocksInbound {
                kind: "socks".into(),
                tag: "xtvpn-local".into(),
                listen: "127.0.0.1".into(),
                listen_port: local_port,
            }],
            outbounds: vec![VlessRealityOutbound {
                kind: "vless".into(),
                tag: "reality-out".into(),
                server: remote_host.to_string(),
                server_port: port,
                uuid: uuid.to_string(),
                packet_encoding: "xudp".into(),
                tls: TlsReality {
                    enabled: true,
                    server_name: sni.to_string(),
                    reality: RealityParams {
                        enabled: true,
                        public_key: pbk.to_string(),
                        short_id: sid.to_string(),
                    },
                },
            }],
            route: RouteConfig {
                rules: vec![RouteRule {
                    inbound: vec!["xtvpn-local".into()],
                    outbound: "reality-out".into(),
                }],
            },
        };

        let config_path = tempfile::NamedTempFile::new()?.into_temp_path();
        serde_json::to_writer_pretty(std::fs::File::create(&config_path)?, &config_json)?;

        let mut child = Command::new("sing-box")
            .env("ENABLE_DEPRECATED_LEGACY_DNS_SERVERS", "true")
            .arg("run")
            .arg("-c")
            .arg(config_path.to_str().context("Invalid path")?)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("Failed to spawn sing-box. Is it installed?")?;
        let stderr = child.stderr.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        tokio::spawn(async move {
            let mut reader = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                tracing::warn!("sing-box stderr: {}", line);
            }
            let mut reader_out = tokio::io::BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader_out.next_line().await {
                tracing::debug!("sing-box stdout: {}", line);
            }
        });
        for _ in 0..30 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if TcpListener::bind(format!("127.0.0.1:{}", local_port))
                .await
                .is_ok()
            {
                tracing::info!("sing-box ready on 127.0.0.1:{}", local_port);
                return Ok(Self { child, local_port });
            }
        }
        bail!("sing-box failed to start within 3 seconds");
    }

    pub fn local_port(&self) -> u16 {
        self.local_port
    }

    pub async fn stop(mut self) {
        if let Err(e) = self.child.kill().await {
            tracing::warn!("Failed to kill sing-box: {}", e);
        }
        let _ = self.child.wait().await;
    }
}
pub async fn socks5_connect(
    mut stream: tokio::net::TcpStream,
    target: &str,
) -> Result<tokio::net::TcpStream> {
    // 1. Аутентификация (NO AUTH)
    stream.write_all(&[0x05, 0x01, 0x00]).await?;
    let mut resp = [0u8; 2];
    stream.read_exact(&mut resp).await?;
    if resp != [0x05, 0x00] {
        bail!("SOCKS5 auth failed")
    }

    // 2. CONNECT команда
    let (host, port) = target.split_once(':').unwrap_or((target, "80"));
    let port = port.parse::<u16>().unwrap_or(80);

    let mut cmd = vec![0x05, 0x01, 0x00, 0x03, host.len() as u8];
    cmd.extend_from_slice(host.as_bytes());
    cmd.extend_from_slice(&port.to_be_bytes());

    stream.write_all(&cmd).await?;
    let mut resp = [0u8; 10];
    stream.read_exact(&mut resp).await?;
    if resp[1] != 0x00 {
        bail!("SOCKS5 CONNECT failed: status {}", resp[1])
    }

    Ok(stream)
}
