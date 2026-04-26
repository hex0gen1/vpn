use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TunnelState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Error(String),
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Server {
    pub id: String,
    pub name: String,
    pub country: String,
    pub hostname: String,
    pub port: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AppMode {
    Easy,
    Pro,
}
pub struct AppSettings {
    pub mode: AppMode,
    pub launch_on_startup: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectRequest {
    pub server_id: String,
    pub mode: AppMode,
}
#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
pub enum Protocol {
    Vless,
    Unknown(String),
    None,
}

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
pub enum Security {
    Reality,
    Tls,
    None,
    Other(String),
}

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
pub enum Transport {
    Tcp,
    Udp,
    TcpUdp,
    Ws,
    Grpc,
    Other(String),
    Quic,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct VpnProfile {
    pub protocol: Protocol,
    pub uuid: String,
    pub host: String,
    pub port: u16,
    pub security: Option<Security>,
    pub transport: Option<Transport>,
    pub sni: Option<String>,
    pub fp: Option<String>,
    pub pbk: Option<String>,
    pub sid: Option<String>,
    pub spx: Option<String>,
    pub flow: Option<String>,
    pub tag: Option<String>,
}
impl VpnProfile {
    pub fn new() -> Self {
        Self {
            protocol: Protocol::None,
            uuid: String::new(),
            host: String::new(),
            port: 0,
            security: Some(Security::None),
            transport: Some(Transport::Udp),
            sni: Some(String::new()),
            fp: Some(String::new()),
            pbk: Some(String::new()),
            sid: Some(String::new()),
            spx: Some(String::new()),
            flow: Some(String::new()),
            tag: Some(String::new()),
        }
    }
    pub fn display_name(&self) -> String {
        let default = String::from("NT[no tag");
        let tag = self.tag.clone().unwrap_or(default);
        return tag;
    }
}
impl Default for VpnProfile {
    fn default() -> Self {
        Self::new()
    }
}
impl Security {
    pub fn as_str(&self) -> String {
        match self {
            Security::Reality => String::from("Reality"),
            Security::Tls => String::from("Tls"),
            Security::Other(text) => String::from(text),
            Security::None => String::new(),
        }
    }
}
impl Transport {
    pub fn as_str(&self) -> String {
        match self {
            Transport::Tcp => String::from("Tcp"),
            Transport::Udp => String::from("Udp"),
            Transport::TcpUdp => String::from("Tcp+Udp"),
            Transport::Ws => String::from("Wireshark"),
            Transport::Grpc => String::from("Grpc"),
            Transport::Quic => String::from("Quic"),
            Transport::Other(text) => String::from(text),
        }
    }
}
impl Protocol {
    pub fn as_str(&self) -> String {
        match self {
            Protocol::Vless => String::from("Vless"),
            Protocol::Unknown(text) => String::from(text),
            Protocol::None => String::new(),
        }
    }
}
