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
