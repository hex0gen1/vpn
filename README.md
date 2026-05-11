XTVPN is a high-performance, transport-agnostic Layer 3 VPN framework written in Rust. 
It features custom binary framing, AEAD encryption with replay protection, async I/O via tokio, and seamless integration with advanced obfuscation
layers (VLESS+TLS, Reality via sing-box sidecar).

* better to read in edit mode


# ARCHITECTURE OVERALL
1. Client/Server connection through mpsc, watcher channels.
2. TUN bridge (async fd <-> mpsc).
3. Framing of packets, with XT frame-structure.
4. Cryptography [encoding/decoding] with nonce replay defense. Currently supported cipher: AES-256-GCM
5. Transport [send,recv,send_to] - UDP, TCP, VLESS+TLS, Reality.
6. Session control [version, ip allocator, peer table].
7.TUI with logs, parser, profiles list, profiles details.

Core components breakdown 

# DAEMON 
vpn_daemon::transport::frame - Binary framing.(encode_frame,decode_frame,FrameKind)
vpn_daemon::transport::transport - Transport abstraction for available protocols(currently - UPD,TCP,VlessTls, Reality+TCP)
vpn_daemon::linux::tun - Tun interface creation, read/write functions, blocking FD handling.(TunInterface, read_paclet, write_packet, create_interface, AsyncFd)
vpn_daemon::parser - Parsing of users config link(host, port, sni, pbk, etc..)
vpn_daemon::transport::singbox - Singbox integration for reality obfuscation(you need to install singbox in terms of using this vpn).
[in the future planning on replacing it with my own realization)
vpn_daemon::client - Client data handling, tokenization of payload(send_data, send_keepalive, recv_helloack, send_hello)
vpn_daemon::server - The most big file in transport sub-crate. Contains main transport logic, helloack handling,
peer|peer_table abstractions, server state, cryptography, tun write/read loops and a lot of helpers methods
like generate crypto state, put user ip and socket, get by ip, get by addr ...

# TUI
vpn_tui::main - Used to handle input events(like y to switch to parser screen), main| run loops for data gathering
from backend handle.(used in future to update metrics/status bar))
vpn_tui::screens::* - Constains render functions for each screen. They-re all have same style, that controlled 
by writed constants, and have similar layout splitting.
vpn_tui::backend - Used mainly to handle TUI -> CORE communication(for example, connect cmd from tui handles in here). Main functions - establish connection,handle command,
trigger_reconnect, metrics_reporter, client_data_loop)
vpn_tui::ui - Used to control all render-stuff(matching screens from input -> render those screens), render status bar and splitting area to sub-functions.

# TYPES
vpn_types::lib - Contains main types, that used by every crate. Easy to maintain, add features, fix something, because they're all in the same place.

## ARCHITECTURE OF EVERY COMPONENT

# XT protocol specification
0-5 bytes - Magic(b"XTVPN)
6 byte - FrameKind
 0x01 - HELLO, 
 0x02 - ERROR,
 0x03 - DATA, 
 0x04 - HELLOACK,
 0x05 - KEEPALIVE.
7..14 bytes - session_id(u64, BE)
15..N bytes - payload(raw packets, may be empty in HELLO frames)

# Cryptography
Algorithm - AES-256-GCM(AEAD)
KeySize - 32 bytes
Nonce - 12 bytes [4b fixed prefics|8b counter]
Counter - AtomicU64(used everywhere in project) starting at 1.
Replay protection - rx_last_nonce tracking highest seen nonce. if packet_nonce <= last_nonce -> drop.

# HANDSHAKE SEQUENCE(required to set up connection)

Client -> Server: [HELLO][session=0][token = "uuid"]
Server -> Client: [HELLOACK][session_id][payload = 0x]

After handshake = 
  1.Both sides inialize CryptoState with new generated key.
  2.tx_nonce = 1, rx_last_nonce = 0.
  3.Server allocates ip from pool(10.8.0.X)
  4.TUN bridge activates
  5.Data exchange begins(DATA frames)

Data flow:
  Client parses VpnProfile from link.
  ActiveTransport::connect() resolves host, selects transport mode.
  If Reality: spawns sing-box sidecar → generates config → waits for SOCKS5 port → CONNECT tunnel.
  TCP/TLS socket established
  client_handshake() sends encrypted HELLO with UUID token
  Server validates token → allocates IP → generates session crypto → replies HELLOACK
  Client receives HELLOACK → syncs nonce → activates TUN bridge

Data flow with ping example: 

# OUTBOUND

[App] → ping 8.8.8.8
  ↓
[OS Routing] → routes to tun5 (10.8.0.2/24)
  ↓
[TUN Interface] → raw IPv4 packet enters /dev/net/tun
  ↓
[tun_reader_loop (Client)] → reads packet → pushes to mpsc channel
  ↓
[client_data_loop] → receives packet → encode_frame(DATA, session_id, packet)
  ↓
[Crypto] → encrypt_frame_sync(frame, crypto) → AES-GCM + nonce
  ↓
[Transport] → write_framed(): [4B len BE][encrypted_payload]
  ↓
[Network] → TCP/TLS/Reality → Server IP:443

# SERVER-PROCESSING

[Server Socket] → accept connection → spawn per-peer task
  ↓
[read_framed()] → read_exact(4) → read_exact(len)
  ↓
[Decrypt] → decrypt_frame_sync(ciphertext, crypto) → verify nonce > last_nonce
  ↓
[Decode] → decode_frame() → extract payload (raw IPv4)
  ↓
[Anti-Spoof] → parse_ipv4_src() → assert src == assigned_ip (10.8.0.X)
  ↓
[TUN Write] → push packet to global TUN interface (xtvpn0)
  ↓
[Linux Kernel] → ip_forward=1 → route to eth0 → internet

# INBOUND-PROCESSING

[Internet] → 8.8.8.8 replies to Server Public IP
  ↓
[iptables MASQUERADE] → rewrites dst → 10.8.0.X → routes to xtvpn0(TUNINTERFACE)
  ↓
[tun_reader_loop (Server)] → reads packet → parse_ipv4_dst() → finds peer
  ↓
[Encode/Crypto] → frame → encrypt → [4B len][encrypted]
  ↓
[Transport] → stream.write_all() → TCP/TLS/Reality
  ↓
[Client] → recv_frame → decrypt → decode → write to TUN
  ↓
[OS] → delivers to ping process → 0% loss

# Transport protocols specification

1. Udp -> Socket -> DatagramBoundaries(1 packet = 1 transmission)
2. Tcp -> Tcp Stream -> Length-prefix([4b][payload])
3. VlessTls -> Tcp + TLS(tokio_rustls) -> Length prefix after TLS
4. Reality -> Tcp + Reality(singbox integrated[socks5) -> Handled by SingboxSidecar

// Write
stream.write_all(&(buf.len() as u32).to_be_bytes()).await?;
stream.write_all(buf).await?;

// Read
let mut len_buf = [0u8; 4];
stream.read_exact(&mut len_buf).await?;
let frame_len = u32::from_be_bytes(len_buf) as usize;
stream.read_exact(&mut buf[..frame_len]).await?;

Max frame: 65535 bytes
Read buffer: vec![0u8; 65536]
Prevents TCP stream fragmentation issues with length prefix

# Server & Client Logic

# Server Architecture

    Listener: TcpListener::bind() + tokio::spawn per connection
    State: Arc<ServerState> with TokioMutex<IpAllocator> + TokioMutex<peers_table>
    Per-Peer Loop: handle_data_loop_tcp → select! on read_framed + idle_timeout + cancel_token
    Global TUN Reader: Single tun_reader_loop reads xtvpn0 → routes to peers by dst_ip
    Cleanup: disconnect_peer() releases IP → removes from hashmaps → logs graceful exit

# Client Architecture

    Connection: establish_connection() → resolves transport → runs handshake
    Data Loop: client_data_loop multiplexes:
        rx_from_tun.recv() → encrypt → transport send
        transport.recv_frame() → decrypt → tx_to_tun.send()
        cancel.cancelled() → break
    TUN Setup: ip link set up + ip addr add via std::process::Command
    Async Bridge: start_tun_bridge() runs two parallel spawn tasks for TUN ↔ mpsc sync

# Security model 

Threat | Instrument | Implementation

Eavesdropping | AEAD encryption | AES-256-GCM, 32B key, unique nonce per frame
	
Replay Attacks | Monotonic nonce check | rx_last_nonce atomic, if nonce <= last → drop
	
IP Spoofing | Source validation | parse_ipv4_src() ↔ assigned_ip mismatch → disconnect
	
Man-in-the-Middle | TLS/Reality validation | rustls cert chain verify, sing-box short_id check
	
DoS / Idle Zombies | Timeout + backoff | idle_timeout=120s, CancellationToken, graceful cleanup
	
Token Leakage | Auth at handshake | UUID validated once, crypto state reset per session


# INSTRUCTIONS ON USAGE

# 1. Enable forwarding
sudo sysctl -w net.ipv4.ip_forward=1
echo "net.ipv4.ip_forward=1" | sudo tee -a /etc/sysctl.conf

# 2. NAT / Masquerade
sudo iptables -t nat -A POSTROUTING -s 10.8.0.0/24 -o eth0 -j MASQUERADE
sudo iptables -A FORWARD -i xtvpn0 -o eth0 -j ACCEPT
sudo iptables -A FORWARD -i eth0 -o xtvpn0 -m state --state RELATED,ESTABLISHED -j ACCEPT

# 3. Run daemon
sudo ./target/release/vpn-daemon

Then paste your link in parsers, new profile will apears in profile screen. go to that screen and connect/disconnect.
Logs showed on the home page.

# 4. Debugging
There is logs in app, but for more in-depth search and educational purposes, you can use tail -f vpn-deubg.log in crates/vpn-tui

License: MIT
Core Dependencies: tokio, rustls, aes-gcm, getrandom, tracing, sing-box (sidecar)
Inspired by: WireGuard (simplicity), Xray (transport flexibility), OpenVPN (TUN routing)
    
    XTVPN is designed for research, education, and secure private networking. 
    Always comply with local regulations regarding encryption and tunneling technologies.

