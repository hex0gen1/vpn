use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use url::Url;
use vpn_types::{Protocol, Security, Transport, VpnProfile};

pub fn parse_vless_link(link: &str) -> Result<VpnProfile> {
    let url = Url::parse(link).context("Invalid VLESS URI syntax")?;
    if url.scheme() != "vless" {
        bail!("Expected vless:// scheme, got {}", url.scheme());
    }

    let uuid = url.username().to_string();
    if uuid.is_empty() {
        bail!("Missing UUID in VLESS link");
    }

    let host = url.host_str().context("Missing host")?.to_string();
    let port = url.port().unwrap_or(443);

    let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
    let get = |k: &str| query.get(k).cloned();

    let protocol = Protocol::Vless;

    let security = match get("security").as_deref() {
        Some("tls") => Some(Security::Tls),
        Some("reality") => Some(Security::Reality),
        Some("none") | None => None,
        _ => bail!("Unknown security type"),
    };

    let transport = match get("type").as_deref() {
        Some("tcp") | Some("raw") => Some(Transport::Tcp),
        Some("grpc") => Some(Transport::Grpc),
        Some("udp") => Some(Transport::Udp),
        Some("quic") => Some(Transport::Quic),
        None => None,
        _ => bail!("Unknown transport type"),
    };

    Ok(VpnProfile {
        protocol,
        uuid,
        host,
        port,
        security,
        transport,
        sni: get("sni"),
        fp: get("fp"),
        pbk: get("pbk"),
        sid: get("sid"),
        spx: get("spx"),
        flow: get("flow"),
        tag: url.fragment().map(String::from),
    })
}

fn config_dir() -> Result<std::path::PathBuf> {
    let dirs = directories::ProjectDirs::from("com", "xtvpn", "xtvpn-client")
        .context("Cannot identificate config directory")?;
    let path = dirs.config_dir().to_path_buf();
    std::fs::create_dir_all(&path).with_context(|| {
        format!(
            "Couldn't create a directory
        : {}",
            path.display()
        )
    })?;
    Ok(path)
}
fn sanitize_tag(tag: &str) -> String {
    tag.replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "_")
        .trim()
        .to_string()
}
pub fn save_profile(profile: &VpnProfile, tag: &str) -> Result<std::path::PathBuf> {
    if tag.trim().is_empty() {
        bail!("Profile tag cannot  be empty.");
    }
    let dir = config_dir()?;
    let filename = format!("{}.toml", sanitize_tag(tag));
    let path = dir.join(&filename);

    let content = toml::to_string_pretty(profile).context("Error with serialization")?;
    std::fs::write(&path, content)
        .with_context(|| format!("Couldn't read file : {}", path.display()))?;

    Ok(path)
}
pub fn load_profile(tag: &str) -> Result<VpnProfile> {
    let dir = config_dir()?;
    let filename = format!("{}.toml", sanitize_tag(tag));
    let path = dir.join(&filename);

    if !path.exists() {
        bail!("profile '{}' not found in  {}", tag, dir.display());
    }

    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Couldnt read file: {}", path.display()))?;
    let profile: VpnProfile = toml::from_str(&content).context("TOML serialization error")?;
    Ok(profile)
}
pub fn list_profiles() -> Result<Vec<std::path::PathBuf>> {
    let dir = config_dir()?;
    let mut files = Vec::new();

    if dir.exists() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "toml") {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}
