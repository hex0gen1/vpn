use vpn_types::{ConnectRequest, Server, TunnelState};
mod error;
mod state;
#[derive(Debug, Clone)]
pub struct CoreState {
    pub tunnel_state: TunnelState,
    pub current_server: Option<Server>,
    pub known_servers: Vec<Server>,
}

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("already connected")]
    AlreadyConnected,

    #[error("connection already in progress")]
    AlreadyConnecting,

    #[error("not connected")]
    NotConnected,

    #[error("server not found: {0}")]
    ServerNotFound(String),
}

#[derive(Debug)]
pub struct ConnectionManager {
    state: CoreState,
}

impl ConnectionManager {
    pub fn new(known_servers: Vec<Server>) -> Self {
        Self {
            state: CoreState {
                tunnel_state: TunnelState::Disconnected,
                current_server: None,
                known_servers,
            },
        }
    }

    pub fn state(&self) -> &CoreState {
        &self.state
    }

    pub fn connect(&mut self, request: ConnectRequest) -> Result<(), CoreError> {
        match self.state.tunnel_state {
            TunnelState::Connected => return Err(CoreError::AlreadyConnected),
            TunnelState::Connecting => return Err(CoreError::AlreadyConnecting),
            _ => {}
        }

        let server = self
            .state
            .known_servers
            .iter()
            .find(|s| s.id == request.server_id)
            .cloned()
            .ok_or_else(|| CoreError::ServerNotFound(request.server_id.clone()))?;

        self.state.tunnel_state = TunnelState::Connecting;
        self.state.current_server = Some(server);
        self.state.tunnel_state = TunnelState::Connected;

        Ok(())
    }

    pub fn disconnect(&mut self) -> Result<(), CoreError> {
        match self.state.tunnel_state {
            TunnelState::Disconnected => return Err(CoreError::NotConnected),
            _ => {}
        }

        self.state.current_server = None;
        self.state.tunnel_state = TunnelState::Disconnected;
        Ok(())
    }

    pub fn list_servers(&self) -> &[Server] {
        &self.state.known_servers
    }
}
