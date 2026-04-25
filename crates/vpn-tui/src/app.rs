use crate::ui::{SB, SK};
use serde::{Deserialize, Serialize};
use vpn_core::{CoreError, CoreState};
use vpn_types::{Protocol, Security, Transport, VpnProfile};
#[derive(Debug, Clone)]
pub struct Profile {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub protocol: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Home,
    Profiles,
    Logs,
    Parser,
    ProfilesDetail,
}

impl Screen {
    pub fn as_str(&self) -> &'static str {
        match self {
            Screen::Home => "Home",
            Screen::Profiles => "Profiles",
            Screen::Logs => "Logs",
            Screen::Parser => "Parser",
            Screen::ProfilesDetail => "PDetails",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Failed,
}

impl ConnectionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConnectionState::Disconnected => "Disconnected",
            ConnectionState::Connecting => "Connecting",
            ConnectionState::Connected => "Connected",
            ConnectionState::Failed => "Failed",
        }
    }
}
#[derive(Debug, Eq, PartialEq)]
pub enum Popup {
    None,
    ConfirmDelete,
    ParserResult,
    ConfirmQuit,
    Connect,
    Error(String),
}
impl Popup {
    pub fn as_str(&self) -> &'static str {
        match self {
            Popup::ConfirmQuit => "ConfirmExit",
            Popup::ConfirmDelete => "ConfirmDelete",
            Popup::ParserResult => "ParserResult",
            Popup::None => "NoPopup",
            Popup::Connect => "Connect",
            Popup::Error(error) => "Error",
        }
    }
}
#[derive(Debug)]
pub struct App {
    pub screen: Screen,
    pub profiles: Vec<VpnProfile>,
    pub selected_profile: usize,
    pub logs: Vec<String>,
    pub connection_state: ConnectionState,
    pub should_quit: bool,
    pub input_str: String,
    pub mode: Mode,
    pub status: SB,
    pub popup: Popup,
    pub current_detail: u64,
    pub detail_groups: Vec<DetailGroup>,
    pub details_on: bool,
}
#[derive(Debug)]
pub enum Mode {
    Input,
    Normal,
    Popup(Popup),
}
#[derive(Debug)]
pub enum DGName {
    General,
    Identity,
    Security,
    Transport,
    Metadata,
    Reality,
}
#[derive(Debug)]
pub enum DName {
    Protocol,
    Uuid,
    Host,
    Port,
    Security,
    Transport,
    Sni,
    Fp,
    Pbk,
    Sid,
    Spx,
    Flow,
    Tag,
    Enabled,
    Source,
    ImportedAt,
    Status,
}
#[derive(Debug)]
pub enum Source {
    Link,
    QrCode,
    Created,
}
impl DGName {
    pub fn as_str(&self) -> String {
        match self {
            DGName::Transport => String::from("Transport"),
            DGName::General => String::from("General"),
            DGName::Identity => String::from("Identity"),
            DGName::Security => String::from("Security"),
            DGName::Reality => String::from("Reality"),
            DGName::Metadata => String::from("Metadata"),
        }
    }
}
impl DName {
    pub fn description(&self) -> &'static str {
        match self {
            DName::Protocol => "VPN protocol used by this profile.",
            DName::Uuid => "Unique client identifier used for authentication.",
            DName::Host => "Remote server hostname or IP address.",
            DName::Port => "Remote server port.",
            DName::Security => "Transport security mode used by the profile.",
            DName::Transport => "Transport type used to carry VPN traffic.",
            DName::Sni => "Server Name Indication used during TLS or REALITY handshake.",
            DName::Fp => "TLS fingerprint used to imitate a specific client type.",
            DName::Pbk => "Public key used by REALITY.",
            DName::Sid => "Short ID used by REALITY.",
            DName::Spx => "SpiderX path used by REALITY.",
            DName::Flow => "Flow mode used by the connection.",
            DName::Tag => "Human-readable profile label.",
            DName::Enabled => "Whether this profile is currently enabled for use.",
            DName::Source => "Where this profile came from.",
            DName::ImportedAt => "Time when this profile was imported into the application.",
            DName::Status => "Current application-level state of this profile.",
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            DName::Protocol => "Protocol",
            DName::Uuid => "UUID",
            DName::Host => "Host",
            DName::Port => "Port",
            DName::Security => "Security",
            DName::Transport => "Transport",
            DName::Sni => "SNI",
            DName::Fp => "Fingerprint",
            DName::Pbk => "Public Key",
            DName::Sid => "Short ID",
            DName::Spx => "SpiderX",
            DName::Flow => "Flow",
            DName::Tag => "Tag",
            DName::Enabled => "Enabled",
            DName::Source => "Source",
            DName::ImportedAt => "Imported At",
            DName::Status => "Status",
        }
    }
}
#[derive(Debug)]
pub struct DetailField {
    pub name: DName,
    pub description: &'static str,
    pub data: String,
}
#[derive(Debug)]
pub struct DetailGroup {
    pub name: DGName,
    pub data: Vec<DetailField>,
}
impl DetailField {
    pub fn new(field_name: DName, value: impl Into<String>) -> Self {
        Self {
            description: field_name.description(),
            name: field_name,
            data: value.into(),
        }
    }
}

impl DetailGroup {
    pub fn new(group_name: DGName) -> Self {
        Self {
            name: group_name,
            data: Vec::new(),
        }
    }
    pub fn push_field(&mut self, field: DetailField) {
        self.data.push(field);
    }
}
impl App {
    pub fn new() -> Self {
        Self {
            screen: Screen::Home,
            profiles: vec![VpnProfile::new()],
            selected_profile: 0,
            logs: vec!["App started".to_string(), "Mock backend ready".to_string()],
            connection_state: ConnectionState::Disconnected,
            should_quit: false,
            mode: Mode::Normal,
            input_str: String::from(""),
            status: SB {
                message: String::from("ready"),
                sk: SK::Info,
            },
            popup: Popup::None,
            detail_groups: Vec::new(),
            current_detail: 0,
            details_on: false,
        }
    }
    pub fn build_general_group(profile: &VpnProfile) -> DetailGroup {
        let mut group = DetailGroup::new(DGName::General);

        group.push_field(DetailField::new(DName::Protocol, profile.protocol.as_str()));

        group.push_field(DetailField::new(DName::Host, profile.host.as_str()));

        group.push_field(DetailField::new(DName::Port, profile.port.to_string()));

        group.push_field(DetailField::new(DName::Uuid, profile.uuid.as_str()));

        group.push_field(DetailField::new(
            DName::Tag,
            profile.tag.as_deref().unwrap_or("-"),
        ));

        group
    }
    pub fn build_reality_group(profile: &VpnProfile) -> DetailGroup {
        let mut group = DetailGroup::new(DGName::Reality);

        group.push_field(DetailField::new(
            DName::Sid,
            profile.sid.as_deref().unwrap_or("-"),
        ));
        group.push_field(DetailField::new(
            DName::Pbk,
            profile.pbk.as_deref().unwrap_or("-"),
        ));
        group.push_field(DetailField::new(
            DName::Spx,
            profile.spx.as_deref().unwrap_or("-"),
        ));

        group.push_field(DetailField::new(
            DName::Flow,
            profile.flow.as_deref().unwrap_or("-"),
        ));
        group
    }
    pub fn build_identity_group(profile: &VpnProfile) -> DetailGroup {
        let mut group = DetailGroup::new(DGName::Identity);

        group.push_field(DetailField::new(DName::Uuid, profile.uuid.as_str()));

        group
    }
    pub fn build_transport_group(profile: &VpnProfile) -> DetailGroup {
        let mut group = DetailGroup::new(DGName::Transport);

        group.push_field(DetailField::new(
            DName::Transport,
            profile.transport.as_ref().unwrap().as_str(),
        ));

        group
    }
    pub fn build_metadata_group(&self) -> DetailGroup {
        let mut group = DetailGroup::new(DGName::Metadata);

        group.push_field(DetailField::new(
            DName::Enabled,
            self.connection_state.as_str(),
        ));
        group
    }
    pub fn build_security_group(profile: &VpnProfile) -> DetailGroup {
        let mut group = DetailGroup::new(DGName::Security);

        group.push_field(DetailField::new(
            DName::Security,
            profile.security.as_ref().unwrap().as_str(),
        ));

        group.push_field(DetailField::new(
            DName::Sni,
            profile.sni.as_deref().unwrap_or("-"),
        ));

        group.push_field(DetailField::new(
            DName::Fp,
            profile.fp.as_deref().unwrap_or("-"),
        ));

        group
    }
    pub fn next_profile(&mut self) {
        if self.selected_profile + 1 < self.profiles.len() {
            self.selected_profile += 1;
        }
    }
    pub fn delete_profile(&mut self) {
        self.profiles.remove(self.selected_profile);
    }
    pub fn prev_profile(&mut self) {
        if self.selected_profile > 0 {
            self.selected_profile -= 1;
        }
    }
    pub fn set_status_info(&mut self, info: String) {
        self.status.sk = SK::Info;
        self.status.message = info;
    }
    pub fn set_status_coreerror(&mut self, error: vpn_core::CoreError) {
        self.status.sk = SK::Error;
        match error {
            CoreError::AlreadyConnected => {
                self.status.message = String::from(
                    "already connected.
                you can use different profile or server",
                );
            }
            CoreError::AlreadyConnecting => {
                self.status.message =
                    String::from("connection pending, wait for daemon to integrate.");
            }

            CoreError::NotConnected => {
                self.status.message =
                    String::from("Failed to connect. Check logs for further explanation.");
            }
            CoreError::ServerNotFound(string) => {
                self.status.message = String::from("Server not founded.");
                self.status.message.push_str(string.as_str());
            }
        }
    }

    pub fn set_status_success(&mut self, message: String) {
        self.status.sk = SK::Success;
        self.status.message = message;
    }
    pub fn current_profile(&self) -> Option<&VpnProfile> {
        self.profiles.get(self.selected_profile)
    }

    pub fn go_home(&mut self) {
        self.screen = Screen::Home;
    }

    pub fn go_profiles(&mut self) {
        self.screen = Screen::Profiles;
    }

    pub fn go_logs(&mut self) {
        self.screen = Screen::Logs;
    }
    pub fn go_parser(&mut self) {
        self.screen = Screen::Parser;
    }
    pub fn connect(&mut self) {
        self.connection_state = ConnectionState::Connected;
        self.logs.push("Connected to selected profile".to_string());
    }
    pub fn delete_selected_profile(&mut self) {
        if self.profiles.is_empty() {
            return;
        }

        self.profiles.remove(self.selected_profile);

        if (self.selected_profile > 0 && self.selected_profile >= self.profiles.len()) {
            self.selected_profile -= 1;
        }
    }
    pub fn disconnect(&mut self) {
        self.connection_state = ConnectionState::Disconnected;
        self.logs.push("Disconnected".to_string());
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
    }
}
