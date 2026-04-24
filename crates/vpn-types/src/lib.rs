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
#[derive(Debug, Serialize, Deserialize)]
pub enum Protocol {
    Vless,
    Unknown(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Security {
    Reality,
    Tls,
    None,
    Other(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Transport {
    Tcp,
    Ws,
    Grpc,
    Other(String),
}

#[derive(Debug, Serialize, Deserialize)]
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
