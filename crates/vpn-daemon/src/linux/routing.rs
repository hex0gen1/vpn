#[derive(Debug, Clone)]
pub struct ClientNetworkConfig {
    pub tun_name: String,
    pub client_addr: String,
    pub client_prefix_len: u8,
    pub server_addr: std::net::IpAddr,
    routes: Vec<RouteConfig>,
}
pub struct ServerNetworkConfig {
    pub tun_name: String,
    pub server_addr: std::net::IpAddr,
    pub server_port: u16,
    pub server_prefix_len: u8,
}
#[derive(Debug, Clone)]
pub struct RouteConfig {
    pub destination: std::net::IpAddr,
    pub prefix_len: u8,
}
pub struct TransportConfig {
    pub server_host: String,
    pub server_port: u16,
    pub protocol: vpn_types::Protocol,
    pub token: crate::transport::client::Token,
}
