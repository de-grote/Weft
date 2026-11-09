//! Peer-to-peer transport over iroh: players are identified by a key (not an address), so VPNs,
//! restarts and port changes do not matter. Connections go direct when NAT allows and through a
//! relay otherwise. The bridge threads convert between QUIC streams and the engine's channels.

use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use crossbeam_channel::unbounded;
use iroh::endpoint::presets;
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, RelayUrl, SecretKey, TransportAddr};
use parking_lot::Mutex;

use super::codec;
use super::{ClientConn, Conn, ServerConn, ToClient, ToServer};
use crate::settings::Settings;

const ALPN: &[u8] = b"weft/1";
const PROTOCOL: u32 = 2;

fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("net")
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
}

/// This install's identity. Generated once, kept in the config directory. Servers key player
/// saves by it, so a key that cannot be read is never replaced and one that cannot be written is
/// an error: a new identity every launch would be a new player everywhere.
pub fn identity(run_dir: &Path) -> Result<SecretKey, String> {
    let path = crate::paths::config(run_dir).join("identity.key");
    match std::fs::read_to_string(&path) {
        Ok(s) => {
            return s
                .trim()
                .parse::<SecretKey>()
                .map_err(|_| format!("{} is not a key; fix or delete it", path.display()));
        }
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(format!("reading {}: {e}", path.display())),
        Err(_) => {}
    }
    let key = SecretKey::generate();
    let hex = crate::kv::hex(&key.to_bytes());
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    std::fs::write(&path, hex).map_err(|e| format!("saving the identity to {}: {e}", path.display()))?;
    Ok(key)
}

/// The player's identity for servers, or empty (keyed by name) when there is none.
pub fn player_key(run_dir: &Path) -> String {
    identity(run_dir)
        .map(|k| k.public().to_string())
        .unwrap_or_else(|e| {
            log::error!("{e}; this world keys the player by name");
            String::new()
        })
}

/// Hash over mods and every registered name: equal hash means equal runtime ids.
pub fn content_hash() -> u64 {
    let c = crate::content::content();
    let mut h = weft_noise::FNV;
    let mut eat = |s: &str| {
        for b in s.bytes().chain([0]) {
            h = weft_noise::fnv_step(h, b as u64);
        }
    };
    for m in &c.mods {
        eat(&m.id);
        eat(&m.version);
    }
    for b in &c.blocks {
        eat(&b.name);
        eat(&b.states.to_string());
    }
    for i in &c.items {
        eat(&i.name);
    }
    for e in &c.game.entities {
        eat(&e.name);
    }
    for m in &c.game.menus {
        eat(&m.name);
    }
    for k in &c.game.components {
        eat(&k.name);
    }
    // Everything else that crosses the wire by id.
    for n in c
        .biomes
        .iter()
        .map(|b| &b.name)
        .chain(c.dims.iter().map(|d| &d.name))
    {
        eat(n);
    }
    for n in c
        .game
        .sounds
        .iter()
        .map(|s| &s.name)
        .chain(c.game.particles.iter().map(|p| &p.name))
        .chain(c.game.configs.iter().map(|k| &k.name))
    {
        eat(n);
    }
    for a in &c.game.attributes {
        eat(&a.name);
    }
    h
}

fn mod_list() -> String {
    crate::content::content()
        .mods
        .iter()
        .map(|m| format!("{} {}", m.id, m.version))
        .collect::<Vec<_>>()
        .join(", ")
}

fn relay_mode() -> RelayMode {
    let custom: Vec<RelayUrl> = crate::settings::relays()
        .iter()
        .filter_map(|r| r.parse().ok())
        .collect();
    if custom.is_empty() { RelayMode::Default } else { RelayMode::custom(custom) }
}

async fn endpoint(settings: &Settings, accept: bool, port: Option<u16>) -> Result<Endpoint, String> {
    let mut b = Endpoint::builder(presets::N0)
        .secret_key(identity(&settings.run_dir)?)
        .relay_mode(relay_mode());
    if accept {
        b = b.alpns(vec![ALPN.to_vec()]);
    }
    if let Some(port) = port {
        b = b
            .bind_addr(std::net::SocketAddr::from((std::net::Ipv4Addr::UNSPECIFIED, port)))
            .map_err(|e| format!("network: {e}"))?;
    }
    b.bind().await.map_err(|e| format!("network: {e}"))
}

async fn write_frame(send: &mut iroh::endpoint::SendStream, data: &[u8]) -> Result<(), String> {
    let packed = if data.len() > 256 { lz4_flex::compress_prepend_size(data) } else { data.to_vec() };
    let flag = (data.len() > 256) as u32;
    let header = (packed.len() as u32) << 1 | flag;
    send.write_all(&header.to_le_bytes())
        .await
        .map_err(|e| e.to_string())?;
    send.write_all(&packed).await.map_err(|e| e.to_string())
}

async fn read_frame(recv: &mut iroh::endpoint::RecvStream) -> Result<Vec<u8>, String> {
    let mut h = [0u8; 4];
    recv.read_exact(&mut h).await.map_err(|e| e.to_string())?;
    let header = u32::from_le_bytes(h);
    let len = (header >> 1) as usize;
    if len > super::MAX_MESSAGE {
        return Err("frame too large".into());
    }
    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf).await.map_err(|e| e.to_string())?;
    if header & 1 == 1 { lz4_flex::decompress_size_prepended(&buf).map_err(|e| e.to_string()) } else { Ok(buf) }
}

/// Ticket: `id;relay;ip:port,ip:port`. The id alone is enough with n0 address lookup; the
/// relay and direct addresses make it work on a LAN or without DNS.
pub fn ticket(addr: &EndpointAddr) -> String {
    let relay = addr
        .relay_urls()
        .next()
        .map(|r| r.to_string())
        .unwrap_or_default();
    let ips: Vec<String> = addr.ip_addrs().map(|a| a.to_string()).collect();
    format!("{};{};{}", addr.id, relay, ips.join(","))
}

/// `host` or `host:port`, as opposed to a ticket or a bare id.
fn is_address(t: &str) -> bool {
    let t = t.trim();
    !t.is_empty() && !t.contains(';') && t.parse::<EndpointId>().is_err()
}

/// Where to connect for an address, with the default port if it names none.
fn with_port(address: &str, port: u16) -> String {
    let a = address.trim();
    match a.parse::<std::net::IpAddr>() {
        Ok(ip) => std::net::SocketAddr::new(ip, port).to_string(),
        Err(_) if a.contains(':') => a.to_string(),
        Err(_) => format!("{a}:{port}"),
    }
}

/// Asks what is at an address for its ticket, and tries that very address first: what it says
/// of itself may be private addresses behind a router.
async fn ask_ticket(address: &str, port: u16) -> Result<EndpointAddr, String> {
    use tokio::io::AsyncReadExt;
    let target = with_port(address, port);
    let wait = Duration::from_secs(5);
    let stream = tokio::time::timeout(wait, tokio::net::TcpStream::connect(&target))
        .await
        .map_err(|_| format!("no answer from {target}"))?;
    let mut stream = stream.map_err(|e| format!("{target}: {e}"))?;
    let peer = stream.peer_addr().map_err(|e| e.to_string())?;
    let mut text = String::new();
    let mut limited = (&mut stream).take(4096);
    tokio::time::timeout(wait, limited.read_to_string(&mut text))
        .await
        .map_err(|_| format!("no answer from {target}"))?
        .map_err(|e| e.to_string())?;
    let addr = parse_ticket(&text).map_err(|_| format!("{target} is not a Weft game"))?;
    let mut all = vec![TransportAddr::Ip(peer)];
    all.extend(addr.addrs.iter().cloned());
    Ok(EndpointAddr::from_parts(addr.id, all))
}

fn parse_ticket(t: &str) -> Result<EndpointAddr, String> {
    let mut parts = t.trim().split(';');
    let id: EndpointId = parts
        .next()
        .unwrap_or("")
        .parse()
        .map_err(|_| "invalid ticket".to_string())?;
    let mut addrs = Vec::new();
    if let Some(r) = parts
        .next()
        .filter(|s| !s.is_empty())
        .and_then(|r| r.parse::<RelayUrl>().ok())
    {
        addrs.push(TransportAddr::Relay(r));
    }
    for ip in parts
        .next()
        .unwrap_or("")
        .split(',')
        .filter_map(|s| s.parse().ok())
    {
        addrs.push(TransportAddr::Ip(ip));
    }
    Ok(EndpointAddr::from_parts(id, addrs))
}

pub struct Hosting {
    pub ticket: Option<String>,
    /// Everything listening for players, stopped with the hosting.
    task: Option<tokio::task::AbortHandle>,
}

pub static HOSTING: Mutex<Option<Hosting>> = Mutex::new(None);

/// Starts accepting players for the running server. The ticket appears in `HOSTING` once the
/// endpoint is online. On the settings' port as well, where a client that only knows the address
/// asks for the ticket (TCP) and then connects like any other (UDP): forward both.
pub fn host(connect: crossbeam_channel::Sender<ServerConn>, settings: &Settings) {
    if HOSTING.lock().is_some() {
        return;
    }
    *HOSTING.lock() = Some(Hosting { ticket: None, task: None });
    let settings = settings.clone();
    let task = runtime().spawn(async move {
        let port = crate::settings::server_port();
        let ep = match endpoint(&settings, true, Some(port)).await {
            Ok(e) => e,
            // The ticket still works without it, say with a second game on this machine.
            Err(e) => {
                log::warn!("port {port}: {e}; only tickets will reach this game");
                match endpoint(&settings, true, None).await {
                    Ok(e) => e,
                    Err(e) => {
                        log::error!("{e}");
                        return;
                    }
                }
            }
        };
        tokio::select! {
            _ = serve_ticket(ep.clone(), port) => {}
            _ = accept_players(ep, connect) => {}
        }
    });
    if let Some(h) = HOSTING.lock().as_mut() {
        h.task = Some(task.abort_handle());
    }
}

async fn accept_players(ep: Endpoint, connect: crossbeam_channel::Sender<ServerConn>) {
    let _ = tokio::time::timeout(Duration::from_secs(8), ep.online()).await;
    let t = ticket(&ep.addr());
    log::info!("hosting, ticket: {t}");
    if let Some(h) = HOSTING.lock().as_mut() {
        h.ticket = Some(t);
    }
    while let Some(incoming) = ep.accept().await {
        let connect = connect.clone();
        tokio::spawn(async move {
            if let Err(e) = accept(incoming, connect).await {
                log::warn!("incoming player: {e}");
            }
        });
    }
}

/// Tells whoever connects the ticket, as it is at that moment.
async fn serve_ticket(ep: Endpoint, port: u16) {
    use tokio::io::AsyncWriteExt;
    let listener = match tokio::net::TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, port)).await {
        Ok(l) => l,
        Err(e) => {
            log::warn!("port {port}: {e}; only tickets will reach this game");
            return std::future::pending().await;
        }
    };
    log::info!("players can connect to port {port}");
    while let Ok((mut stream, _)) = listener.accept().await {
        let t = ticket(&ep.addr());
        tokio::spawn(async move {
            let _ = stream.write_all(t.as_bytes()).await;
        });
    }
}

pub fn stop_hosting() {
    if let Some(task) = HOSTING.lock().take().and_then(|h| h.task) {
        task.abort();
    }
}

async fn accept(incoming: iroh::endpoint::Incoming, connect: crossbeam_channel::Sender<ServerConn>) -> Result<(), String> {
    let conn = incoming.await.map_err(|e| e.to_string())?;
    let peer = conn.remote_id();
    let (mut send, mut recv) = conn.accept_bi().await.map_err(|e| e.to_string())?;
    let hello = read_frame(&mut recv).await?;
    let (proto, hash) = <(u32, u64) as codec::Wire>::get(&mut codec::R(&hello)).ok_or("bad hello")?;
    let reply = if proto != PROTOCOL {
        format!("protocol mismatch: server {PROTOCOL}, client {proto}")
    } else if hash != content_hash() {
        format!("different mods; server runs: {}", mod_list())
    } else {
        String::new()
    };
    write_frame(&mut send, reply.as_bytes()).await?;
    if !reply.is_empty() {
        return Err(reply);
    }
    log::info!("player {peer} connected");
    let (to_server_tx, to_server_rx) = unbounded::<ToServer>();
    let (to_client_tx, to_client_rx) = unbounded::<ToClient>();
    connect
        .send(Conn::new(to_client_tx, to_server_rx))
        .map_err(|_| "server stopped")?;
    let peer_key = peer.to_string();
    // Incoming: the authenticated key replaces whatever key the client claims.
    let reader = tokio::spawn(async move {
        while let Ok(frame) = read_frame(&mut recv).await {
            let Some(mut msg) = codec::decode_server(&frame) else { break };
            if let ToServer::Join { key, .. } = &mut msg {
                *key = peer_key.clone();
            }
            if to_server_tx.send(msg).is_err() {
                break;
            }
        }
        let _ = to_server_tx.send(ToServer::Leave);
    });
    pump_out(to_client_rx, send, codec::encode_client).await;
    reader.abort();
    conn.close(0u32.into(), b"bye");
    Ok(())
}

/// Forwards a crossbeam channel into a stream. The channel is read on a blocking thread because
/// the engine side is synchronous.
async fn pump_out<T: Send + crate::net::Message + 'static>(
    rx: crossbeam_channel::Receiver<T>,
    mut send: iroh::endpoint::SendStream,
    encode: fn(&T) -> Vec<u8>,
) {
    let (tx, mut arx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    std::thread::Builder::new()
        .name("net-out".into())
        .spawn(move || {
            while let Ok(m) = rx.recv() {
                let bytes = encode(&m);
                crate::diag::net_message(T::TO_SERVER, m.kind(), 0, bytes.len());
                if tx.send(bytes).is_err() {
                    break;
                }
            }
        })
        .ok();
    while let Some(frame) = arx.recv().await {
        if write_frame(&mut send, &frame).await.is_err() {
            break;
        }
    }
}

/// Connects to `addr`: every address at once first; if the handshake does not finish soon, again
/// from a new endpoint through the relay alone. With several network adapters (VPNs, virtual
/// switches) the host may take the handshake's first packet from an address its answers never
/// get back to, and the attempt hangs on it whatever else the client tries (about one in six
/// on a machine with Radmin VPN and Hyper-V). The relay always gets through; iroh moves the
/// connection to a direct path afterwards when one works.
async fn connect(settings: &Settings, addr: EndpointAddr) -> Result<(Endpoint, iroh::endpoint::Connection), String> {
    let relayed = EndpointAddr::from_parts(
        addr.id,
        addr.relay_urls()
            .cloned()
            .map(TransportAddr::Relay)
            .collect::<Vec<_>>(),
    );
    let attempts = [(addr, Duration::from_secs(3)), (relayed, Duration::from_secs(20))];
    let mut last = String::new();
    for (i, (addr, wait)) in attempts.into_iter().enumerate() {
        let ep = endpoint(settings, false, None).await?;
        match tokio::time::timeout(wait, ep.connect(addr, ALPN)).await {
            Ok(Ok(conn)) => return Ok((ep, conn)),
            Ok(Err(e)) => last = e.to_string(),
            Err(_) => last = "timed out".into(),
        }
        if i == 0 {
            log::info!("connecting directly failed ({last}), trying through the relay");
        }
        ep.close().await;
    }
    Err(last)
}

/// Connects to a host. Blocks up to 30 seconds.
pub fn join(ticket: &str, settings: &Settings) -> Result<ClientConn, String> {
    let ticket = ticket.to_string();
    let settings = settings.clone();
    let (ready_tx, ready_rx) = crossbeam_channel::bounded::<Result<ClientConn, String>>(1);
    runtime().spawn(async move {
        let result = async {
            let addr =
                if is_address(&ticket) { ask_ticket(&ticket, crate::settings::server_port()).await? } else { parse_ticket(&ticket)? };
            let (ep, conn) = connect(&settings, addr).await?;
            let (mut send, mut recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
            let mut hello = codec::W::default();
            codec::Wire::put(&(PROTOCOL, content_hash()), &mut hello);
            write_frame(&mut send, &hello.0).await?;
            let reply = read_frame(&mut recv).await?;
            if !reply.is_empty() {
                return Err(String::from_utf8_lossy(&reply).into_owned());
            }
            Ok((ep, conn, send, recv))
        }
        .await;
        match result {
            Ok((ep, conn, send, mut recv)) => {
                let (to_server_tx, to_server_rx) = unbounded::<ToServer>();
                let (to_client_tx, to_client_rx) = unbounded::<ToClient>();
                let _ = ready_tx.send(Ok(Conn::new(to_server_tx, to_client_rx)));
                let reader = tokio::spawn(async move {
                    while let Ok(frame) = read_frame(&mut recv).await {
                        let Some(msg) = codec::decode_client(&frame) else { break };
                        if to_client_tx.send(msg).is_err() {
                            break;
                        }
                    }
                    let _ = to_client_tx.send(ToClient::Disconnect("connection closed".into()));
                });
                pump_out(to_server_rx, send, codec::encode_server).await;
                reader.abort();
                conn.close(0u32.into(), b"bye");
                ep.close().await;
            }
            Err(e) => {
                let _ = ready_tx.send(Err(e));
            }
        }
    });
    ready_rx
        .recv_timeout(Duration::from_secs(30))
        .map_err(|_| "timed out".to_string())?
}

#[cfg(test)]
mod tests {
    #[test]
    fn addresses_and_tickets_tell_apart() {
        let id = "0".repeat(64);
        for a in ["127.0.0.1", "127.0.0.1:27182", "play.example.org", "play.example.org:25", "::1", "[::1]:25"] {
            assert!(super::is_address(a), "{a}");
        }
        assert!(!super::is_address(""));
        assert!(!super::is_address(&format!("{id};;")), "a ticket");
    }

    #[test]
    fn an_address_gets_the_default_port() {
        assert_eq!(super::with_port("10.0.0.1", 7), "10.0.0.1:7");
        assert_eq!(super::with_port("10.0.0.1:8", 7), "10.0.0.1:8");
        assert_eq!(super::with_port("play.example.org", 7), "play.example.org:7");
        assert_eq!(super::with_port("::1", 7), "[::1]:7");
        assert_eq!(super::with_port("[::1]:8", 7), "[::1]:8");
    }

    #[test]
    fn identity_is_kept_and_never_replaced() {
        let dir = std::env::temp_dir().join(format!("weft-identity-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let a = super::identity(&dir).expect("a new identity");
        assert_eq!(super::identity(&dir).expect("read again").public(), a.public());
        let file = dir.join("config").join("identity.key");
        std::fs::write(&file, "not a key").unwrap();
        assert!(super::identity(&dir).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "not a key");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
