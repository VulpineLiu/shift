//! LAN "AirDrop" for Shift: UDP beacons build a live peer list, and a small
//! length-prefixed TCP protocol moves the file bytes. Std-only, so the macOS and
//! Windows builds keep their existing dependency tree.

use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket},
    path::{Path, PathBuf},
    sync::mpsc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

/// Multicast group and port every Shift copy beacons on. The port is shared by
/// every copy (SO_REUSEPORT) so several instances on one machine all hear the
/// same beacons; file transfers use the ephemeral TCP port carried in the beacon.
const GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 77, 88);
const BEACON_PORT: u16 = 47_888;
/// A peer that has not been heard from for this long leaves the list. Keep this
/// longer than an old peer's own nine-second expiry plus our two-second unicast
/// refresh, or a scan-only peer will flash and disappear between replies.
const PEER_TIMEOUT: Duration = Duration::from_secs(15);
const BEACON_INTERVAL: Duration = Duration::from_secs(2);
/// A unicast sweep is a fallback for networks that drop both multicast and
/// broadcast. Keep it infrequent; once one peer answers, unicast keeps it fresh.
const SWEEP_INTERVAL: Duration = Duration::from_secs(30);
/// How long the receiving side may take to accept an incoming file. The sender
/// blocks on its copy of this wait, so keep it short enough to notice by hand.
pub const DECISION_TIMEOUT: Duration = Duration::from_secs(45);
/// How long a batch stays pre-approved on the receiving side, so a multi-file send
/// asks once but a later, unrelated batch still prompts again.
const BATCH_WINDOW: Duration = Duration::from_secs(120);
/// Re-create the discovery socket this often so the multicast group is re-joined
/// on the current interface after a Wi-Fi change or a wake from sleep.
const SOCKET_LIFETIME: Duration = Duration::from_secs(30);
/// Starts every transfer so a stray connection cannot corrupt a file.
const MAGIC: &[u8; 4] = b"SHSF";
/// A local-network attacker can mint unlimited beacon ids. Keep the visible peer
/// table bounded so discovery cannot grow without limit.
const MAX_PEERS: usize = 256;
/// How long the discovery thread may sleep inside `recv_from` before checking the
/// clock again. This is the loop's heartbeat, so it is the whole cost of a Shift that
/// is running and has nothing to do.
const LISTEN_TIMEOUT: Duration = Duration::from_millis(400);
/// The same wait while this copy stays silent on the discovery port. It still has to
/// hear other copies — being switched off means "do not announce", not "be deaf", and
/// a deaf copy cannot be sent to — but nothing is timed except a peer's expiry, so it
/// can afford to wake a quarter as often.
const LISTEN_TIMEOUT_SILENT: Duration = Duration::from_millis(1_600);
/// How often stale peers are dropped and the list is compared against what the UI was
/// last told. With peers live this stays sharp enough that a machine which stops
/// answering leaves the list inside its own timeout; with none, the tick has nothing to
/// expire and nothing to report, so it is worth a quarter as much.
const PEER_CHECK: Duration = Duration::from_millis(600);
const PEER_CHECK_IDLE: Duration = Duration::from_millis(2_400);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Peer {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub address: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Beacon {
    id: String,
    name: String,
    platform: String,
    /// TCP port of the file receiver.
    port: u16,
    /// True on a unicast answer. It prevents two new copies from answering each
    /// other forever while still refreshing peers on every regular beacon.
    #[serde(default)]
    reply: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct ReceivedBeacon {
    name: String,
    fresh: bool,
    reply: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TransferEvent {
    transfer_id: String,
    file_name: String,
    peer_name: String,
    sent: u64,
    total: u64,
    /// "sending" | "received" | "error"
    state: String,
    path: Option<String>,
}

/// An incoming file that the user has not answered yet.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingShare {
    pub transfer_id: String,
    pub file_name: String,
    pub size: u64,
    pub peer_name: String,
    /// Seconds the receiver waits before it refuses on the user's behalf, so the
    /// notice can show a countdown that cannot disagree with the real deadline.
    pub timeout_seconds: u64,
}

enum Decision {
    Accepted,
    Rejected,
    Expired,
}

/// One waiting offer: how to wake the receiving thread, plus the UI notice id the
/// answer came from so a click is never applied twice.
struct PendingOffer {
    respond: mpsc::Sender<Decision>,
    notice_id: String,
}

struct KnownPeer {
    name: String,
    platform: String,
    /// Where to connect to send a file.
    address: SocketAddr,
    seen: Instant,
}

pub struct ShareState {
    local: Mutex<Beacon>,
    peers: Mutex<HashMap<String, KnownPeer>>,
    pending: Mutex<HashMap<String, PendingOffer>>,
    /// Batch ids the user already accepted, so a multi-file send is one prompt.
    accepted_batches: Mutex<HashMap<String, Instant>>,
    /// False means this copy stays silent on the discovery port.
    enabled: AtomicBool,
    /// Set by the UI so the long-lived discovery thread can scan immediately.
    refresh_requested: AtomicBool,
}

impl Default for ShareState {
    fn default() -> Self {
        Self {
            local: Mutex::new(Beacon::default()),
            peers: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            accepted_batches: Mutex::new(HashMap::new()),
            enabled: AtomicBool::new(true),
            refresh_requested: AtomicBool::new(false),
        }
    }
}

impl ShareState {
    fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    fn request_refresh(&self) {
        self.refresh_requested.store(true, Ordering::Relaxed);
    }

    fn take_refresh_request(&self) -> bool {
        self.refresh_requested.swap(false, Ordering::Relaxed)
    }

    fn peers_visible(&self) -> Vec<Peer> {
        let local_id = self
            .local
            .lock()
            .map(|local| local.id.clone())
            .unwrap_or_default();
        let mut peers: Vec<Peer> = self
            .peers
            .lock()
            .map(|peers| {
                peers
                    .iter()
                    .filter(|(id, _)| **id != local_id)
                    .map(|(id, peer)| Peer {
                        id: id.clone(),
                        name: peer.name.clone(),
                        platform: peer.platform.clone(),
                        address: peer.address.to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        peers.sort_by_key(|peer| peer.name.to_lowercase());
        peers
    }

    fn local_beacon(&self) -> Beacon {
        self.local
            .lock()
            .map(|local| local.clone())
            .unwrap_or_default()
    }

    fn note(&self, beacon: &Beacon, from: SocketAddr) -> bool {
        let Ok(mut peers) = self.peers.lock() else {
            return false;
        };
        let known = peers.contains_key(&beacon.id);
        if !known && peers.len() >= MAX_PEERS {
            peers.retain(|_, peer| peer.seen.elapsed() < PEER_TIMEOUT);
            if peers.len() >= MAX_PEERS {
                return false;
            }
        }
        peers.insert(
            beacon.id.clone(),
            KnownPeer {
                name: beacon.name.clone(),
                platform: beacon.platform.clone(),
                address: SocketAddr::new(from.ip(), beacon.port),
                seen: Instant::now(),
            },
        );
        !known
    }

    fn peer(&self, id: &str) -> Option<(String, SocketAddr)> {
        self.peers
            .lock()
            .ok()
            .and_then(|peers| peers.get(id).map(|peer| (peer.name.clone(), peer.address)))
    }

    fn peer_destinations(&self) -> Vec<SocketAddr> {
        self.peers
            .lock()
            .map(|peers| {
                peers
                    .values()
                    .map(|peer| SocketAddr::new(peer.address.ip(), BEACON_PORT))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Drop stale peers; returns true when the visible list changed.
    fn expire(&self) -> bool {
        let Ok(mut peers) = self.peers.lock() else {
            return false;
        };
        let before = peers.len();
        peers.retain(|_, peer| peer.seen.elapsed() < PEER_TIMEOUT);
        peers.len() != before
    }

    /// Whether this copy currently knows of another copy, ignoring its own id.
    /// Used only to decide how often the discovery loop may check the clock, never
    /// to decide what the peer list contains.
    fn has_peers(&self) -> bool {
        let local_id = self
            .local
            .lock()
            .map(|local| local.id.clone())
            .unwrap_or_default();
        self.peers
            .lock()
            .map(|peers| peers.keys().any(|id| *id != local_id))
            .unwrap_or(false)
    }

    fn add_pending(&self, transfer_id: &str, respond: mpsc::Sender<Decision>, notice_id: &str) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.insert(
                transfer_id.to_string(),
                PendingOffer {
                    respond,
                    notice_id: notice_id.to_string(),
                },
            );
        }
    }

    /// Remove a waiting offer and hand it the user's answer. Returns the notice id
    /// so the UI can drop the matching card, or None when nobody was waiting.
    fn resolve_pending(&self, transfer_id: &str, accepted: bool) -> Option<String> {
        let offer = self.pending.lock().ok()?.remove(transfer_id)?;
        let _ = offer.respond.send(if accepted {
            Decision::Accepted
        } else {
            Decision::Rejected
        });
        Some(offer.notice_id)
    }

    /// Decline the offer behind a notice window so the sender stops waiting the
    /// moment its card disappears rather than after the full timeout.
    pub fn reject_notice(&self, notice_id: &str) -> bool {
        let offer = {
            let Ok(mut pending) = self.pending.lock() else {
                return false;
            };
            let id = pending
                .iter()
                .find(|(_, offer)| offer.notice_id == notice_id)
                .map(|(id, _)| id.clone());
            id.and_then(|id| pending.remove(&id))
        };
        match offer {
            Some(offer) => offer.respond.send(Decision::Rejected).is_ok(),
            None => false,
        }
    }

    fn forget_pending(&self, transfer_id: &str) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(transfer_id);
        }
    }

    /// True when this header belongs to a multi-file batch the user has accepted.
    fn batch_accepted(&self, header: &TransferHeader) -> bool {
        if header.batch_total < 2 || header.batch_id.is_empty() {
            return false;
        }
        let fresh = self
            .accepted_batches
            .lock()
            .ok()
            .and_then(|batches| {
                batches
                    .get(&header.batch_id)
                    .map(|seen| seen.elapsed() < BATCH_WINDOW)
            })
            .unwrap_or(false);
        fresh
    }

    fn accept_batch(&self, header: &TransferHeader) {
        if header.batch_total < 2 || header.batch_id.is_empty() {
            return;
        }
        if let Ok(mut batches) = self.accepted_batches.lock() {
            // A long-running app should not remember batches forever.
            batches.retain(|_, seen| seen.elapsed() < BATCH_WINDOW);
            batches.insert(header.batch_id.clone(), Instant::now());
        }
    }
}

pub fn set_enabled(app: &AppHandle, enabled: bool) {
    if let Some(state) = app.try_state::<ShareState>() {
        state.enabled.store(enabled, Ordering::Relaxed);
    }
}

/// Starts the beacon loop and the TCP file receiver. Called once from setup.
pub fn spawn(app: &AppHandle) {
    let discovery = match discovery_socket() {
        Ok(sock) => sock,
        Err(error) => {
            eprintln!("Shift share is unavailable: {error}");
            return;
        }
    };
    let receiver = match TcpListener::bind(("0.0.0.0", 0)) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("Shift share is unavailable: {error}");
            return;
        }
    };
    let port = receiver
        .local_addr()
        .map(|address| address.port())
        .unwrap_or(0);
    let local = Beacon {
        id: uuid::Uuid::new_v4().to_string(),
        name: machine_name(),
        platform: platform_label(),
        port,
        reply: false,
    };
    if let Ok(mut guard) = app.state::<ShareState>().local.lock() {
        *guard = local.clone();
    }

    let beacon_app = app.clone();
    thread::spawn(move || beacon_loop(&beacon_app, discovery, &local));

    let file_app = app.clone();
    thread::spawn(move || {
        for stream in receiver.incoming() {
            let Ok(stream) = stream else { continue };
            let handle = file_app.clone();
            thread::spawn(move || {
                if let Err(error) = receive_file(&handle, stream) {
                    eprintln!("Shift share transfer failed: {error}");
                }
            });
        }
    });
}

fn machine_name() -> String {
    std::env::var("COMPUTERNAME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(hostname_native)
        .map(|value| value.trim().trim_end_matches(".local").to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Shift".to_string())
}

#[cfg(target_os = "windows")]
fn hostname_native() -> Option<String> {
    use windows_sys::Win32::System::WindowsProgramming::GetComputerNameW;

    let mut buffer = [0_u16; 256];
    let mut length = buffer.len() as u32;
    let success = unsafe { GetComputerNameW(buffer.as_mut_ptr(), &mut length) };
    (success != 0)
        .then(|| String::from_utf16_lossy(&buffer[..length as usize]))
        .filter(|value| !value.is_empty())
}

#[cfg(not(target_os = "windows"))]
fn hostname_native() -> Option<String> {
    crate::process::output_with_timeout("hostname", &[], std::time::Duration::from_secs(3))
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
}

fn platform_label() -> String {
    if cfg!(target_os = "macos") {
        "macos".into()
    } else if cfg!(target_os = "windows") {
        "windows".into()
    } else {
        "linux".into()
    }
}

fn prepare(sock: &UdpSocket) {
    let _ = sock.set_broadcast(true);
    let _ = sock.join_multicast_v4(&GROUP, &Ipv4Addr::UNSPECIFIED);
    // Keep the multicast loop on so several Shift copies on one machine also see
    // each other; a copy that hears its own beacon drops it by id.
    let _ = sock.set_multicast_loop_v4(true);
    let _ = sock.set_read_timeout(Some(Duration::from_millis(400)));
}

/// Turn an interface address and mask into the broadcast address for that subnet.
/// Loopback, link-local and unusable masks are excluded because sending there
/// cannot reach another Shift instance.
fn directed_broadcast(address: Ipv4Addr, netmask: Ipv4Addr) -> Option<Ipv4Addr> {
    let mask = u32::from(netmask);
    if mask == 0
        || mask == u32::MAX
        || address.is_unspecified()
        || address.is_loopback()
        || address.is_link_local()
        || address.is_multicast()
    {
        return None;
    }
    Some(Ipv4Addr::from(u32::from(address) | !mask))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct LocalNetwork {
    address: Ipv4Addr,
    netmask: Ipv4Addr,
}

#[cfg(unix)]
fn interface_networks() -> Vec<LocalNetwork> {
    let mut head = std::ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut head) } != 0 {
        return Vec::new();
    }

    let mut networks = Vec::new();
    let mut current = head;
    while !current.is_null() {
        let interface = unsafe { &*current };
        current = interface.ifa_next;

        let up = interface.ifa_flags & libc::IFF_UP as libc::c_uint != 0;
        let supports_broadcast = interface.ifa_flags & libc::IFF_BROADCAST as libc::c_uint != 0;
        if !up || !supports_broadcast {
            continue;
        }

        let Some(address) = ipv4_from_sockaddr(interface.ifa_addr) else {
            continue;
        };
        let Some(netmask) = ipv4_from_sockaddr(interface.ifa_netmask) else {
            continue;
        };
        if directed_broadcast(address, netmask).is_some() {
            networks.push(LocalNetwork { address, netmask });
        }
    }
    unsafe { libc::freeifaddrs(head) };

    networks.sort_unstable();
    networks.dedup();
    networks
}

#[cfg(unix)]
fn ipv4_from_sockaddr(address: *const libc::sockaddr) -> Option<Ipv4Addr> {
    if address.is_null() || unsafe { (*address).sa_family } as i32 != libc::AF_INET {
        return None;
    }
    let address = unsafe { &*(address as *const libc::sockaddr_in) };
    Some(Ipv4Addr::from(address.sin_addr.s_addr.to_ne_bytes()))
}

#[cfg(target_os = "windows")]
fn interface_networks() -> Vec<LocalNetwork> {
    use windows_sys::Win32::{
        Foundation::{ERROR_BUFFER_OVERFLOW, NO_ERROR},
        NetworkManagement::{
            IpHelper::{
                GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
                GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH,
            },
            Ndis::IfOperStatusUp,
        },
        Networking::WinSock::{AF_INET, SOCKADDR_IN},
    };

    const FLAGS: u32 = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    let family = AF_INET as u32;
    let mut size = 0;
    let first = unsafe {
        GetAdaptersAddresses(
            family,
            FLAGS,
            std::ptr::null(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if first != ERROR_BUFFER_OVERFLOW {
        return Vec::new();
    }

    let mut networks = Vec::new();
    loop {
        // IP_ADAPTER_ADDRESSES_LH is 8-byte aligned, so backing the API's buffer
        // with u64 avoids the unaligned pointer a Vec<u8> could produce.
        let mut storage = vec![0_u64; size.div_ceil(8) as usize];
        let status = unsafe {
            GetAdaptersAddresses(
                family,
                FLAGS,
                std::ptr::null(),
                storage.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH,
                &mut size,
            )
        };
        if status == ERROR_BUFFER_OVERFLOW {
            continue;
        }
        if status != NO_ERROR {
            return Vec::new();
        }

        let mut adapter = storage.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
        while !adapter.is_null() {
            let current = unsafe { &*adapter };
            adapter = current.Next;
            if current.OperStatus != IfOperStatusUp {
                continue;
            }

            let mut unicast = current.FirstUnicastAddress;
            while !unicast.is_null() {
                let address = unsafe { &*unicast };
                unicast = address.Next;
                if address.Address.lpSockaddr.is_null() {
                    continue;
                }

                let socket = unsafe { &*(address.Address.lpSockaddr as *const SOCKADDR_IN) };
                if socket.sin_family != AF_INET as u16 {
                    continue;
                }
                let ip = Ipv4Addr::from(unsafe { socket.sin_addr.S_un.S_addr }.to_ne_bytes());
                let netmask = prefix_netmask(address.OnLinkPrefixLength.min(32));
                if directed_broadcast(ip, netmask).is_some() {
                    networks.push(LocalNetwork {
                        address: ip,
                        netmask,
                    });
                }
            }
        }
        break;
    }

    networks.sort_unstable();
    networks.dedup();
    networks
}

#[cfg(target_os = "windows")]
fn prefix_netmask(prefix: u8) -> Ipv4Addr {
    let mask = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    Ipv4Addr::from(mask)
}

#[cfg(not(any(unix, target_os = "windows")))]
fn interface_networks() -> Vec<LocalNetwork> {
    Vec::new()
}

fn interface_broadcasts() -> Vec<Ipv4Addr> {
    let mut broadcasts: Vec<Ipv4Addr> = interface_networks()
        .into_iter()
        .filter_map(|network| directed_broadcast(network.address, network.netmask))
        .collect();
    broadcasts.sort_unstable();
    broadcasts.dedup();
    broadcasts
}

/// Enumerate a small subnet by unicast. Broadcast and multicast are the efficient
/// paths; this one still works when an access point drops both, which is common on
/// guest Wi-Fi and some enterprise networks. Subnets larger than /24 are sampled
/// from the /24 that contains this host so discovery cannot turn into a flood.
fn subnet_scan_addresses(address: Ipv4Addr, netmask: Ipv4Addr) -> Vec<Ipv4Addr> {
    if directed_broadcast(address, netmask).is_none() {
        return Vec::new();
    }
    let mask = u32::from(netmask);
    let host_bits = (!mask).count_ones();
    let (first, count) = if host_bits <= 8 {
        let network = u32::from(address) & mask;
        (network + 1, (1_u32 << host_bits).saturating_sub(2))
    } else {
        (u32::from(address) & 0xffff_ff00 | 1, 254)
    };
    (0..count)
        .map(|offset| Ipv4Addr::from(first + offset))
        .filter(|candidate| *candidate != address)
        .collect()
}

fn scan_destinations() -> Vec<SocketAddr> {
    let mut addresses: Vec<Ipv4Addr> = interface_networks()
        .into_iter()
        .flat_map(|network| subnet_scan_addresses(network.address, network.netmask))
        .collect();
    addresses.sort_unstable();
    addresses.dedup();
    addresses
        .into_iter()
        .map(|address| SocketAddr::from((address, BEACON_PORT)))
        .collect()
}

/// A discovery socket on the shared beacon port. SO_REUSEPORT must be set before
/// bind, so unix builds create the raw socket with libc; Windows instances fall
/// back to a plain bind (one listener per machine, which is also what the
/// default firewall allows).
fn discovery_socket() -> std::io::Result<UdpSocket> {
    #[cfg(unix)]
    {
        use std::os::unix::io::FromRawFd;
        let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let on: libc::c_int = 1;
        unsafe {
            for option in [libc::SO_REUSEADDR, libc::SO_REUSEPORT] {
                libc::setsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    option,
                    &on as *const _ as *const libc::c_void,
                    std::mem::size_of::<libc::c_int>() as libc::socklen_t,
                );
            }
            let mut address: libc::sockaddr_in = std::mem::zeroed();
            address.sin_family = libc::AF_INET as libc::sa_family_t;
            address.sin_port = BEACON_PORT.to_be();
            address.sin_addr = libc::in_addr {
                s_addr: u32::from_ne_bytes([0, 0, 0, 0]),
            };
            let bound = libc::bind(
                fd,
                &address as *const libc::sockaddr_in as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
            );
            if bound < 0 {
                let error = std::io::Error::last_os_error();
                libc::close(fd);
                return Err(error);
            }
        }
        let sock = unsafe { UdpSocket::from_raw_fd(fd) };
        prepare(&sock);
        Ok(sock)
    }
    #[cfg(not(unix))]
    {
        let sock = UdpSocket::bind(("0.0.0.0", BEACON_PORT))?;
        prepare(&sock);
        Ok(sock)
    }
}

/// Reach every attached subnet. Multicast covers modern networks; directed
/// broadcast covers Wi-Fi and guest networks that filter multicast but still
/// forward subnet broadcasts. The limited broadcast remains a last resort.
fn announce(sock: &UdpSocket, payload: &[u8]) {
    let _ = sock.send_to(payload, SocketAddr::from((IpAddr::V4(GROUP), BEACON_PORT)));

    let mut broadcasts = interface_broadcasts();
    broadcasts.push(Ipv4Addr::BROADCAST);
    broadcasts.sort_unstable();
    broadcasts.dedup();
    for address in broadcasts {
        let _ = sock.send_to(
            payload,
            SocketAddr::from((IpAddr::V4(address), BEACON_PORT)),
        );
    }
}

/// Last-resort discovery for networks that drop both multicast and broadcast.
fn scan_subnet(sock: &UdpSocket, payload: &[u8]) {
    for destination in scan_destinations() {
        let _ = sock.send_to(payload, destination);
    }
}

/// Register one received datagram. `fresh` only controls logging; every regular
/// beacon gets an answer so a peer stays visible even when its own sends are
/// filtered by the access point.
fn register_beacon(
    state: &ShareState,
    bytes: &[u8],
    from: SocketAddr,
    local: &Beacon,
) -> Option<ReceivedBeacon> {
    let Ok(beacon) = serde_json::from_slice::<Beacon>(bytes) else {
        return None;
    };
    if beacon.id.is_empty() || beacon.port == 0 || beacon.id == local.id {
        return None;
    }
    Some(ReceivedBeacon {
        name: beacon.name.clone(),
        fresh: state.note(&beacon, from),
        reply: beacon.reply,
    })
}

fn beacon_loop(app: &AppHandle, mut sock: UdpSocket, local: &Beacon) {
    let Ok(payload) = serde_json::to_vec(local) else {
        return;
    };
    let mut reply_beacon = local.clone();
    reply_beacon.reply = true;
    let Ok(reply_payload) = serde_json::to_vec(&reply_beacon) else {
        return;
    };
    let mut buffer = [0u8; 2048];
    let mut signature: Option<String> = None;
    // A menu bar app outlives Wi-Fi switches and sleep, so a socket that stops
    // working is replaced rather than abandoned, and the loop never exits.
    'discovery: loop {
        prepare(&sock);
        // `prepare` gives a fresh socket the default heartbeat, so the cadence the
        // loop wants is re-applied on the first pass rather than assumed.
        let mut listen_timeout = Duration::ZERO;
        let opened = Instant::now();
        let mut last_announce = opened - BEACON_INTERVAL;
        let mut last_scan = opened - SWEEP_INTERVAL;
        let mut last_peer_check = opened;

        while opened.elapsed() < SOCKET_LIFETIME {
            let (discoverable, refresh) = {
                let state = app.state::<ShareState>();
                (state.is_enabled(), state.take_refresh_request())
            };
            // A silent copy has no beacon to time, so it can wait longer for the next
            // packet and wake a quarter as often. An announced peer is still heard just
            // as fast, because the announce clock below is untouched.
            let wanted = if discoverable {
                LISTEN_TIMEOUT
            } else {
                LISTEN_TIMEOUT_SILENT
            };
            if wanted != listen_timeout {
                listen_timeout = wanted;
                let _ = sock.set_read_timeout(Some(wanted));
            }
            if discoverable && (refresh || last_announce.elapsed() >= BEACON_INTERVAL) {
                announce(&sock, &payload);
                let destinations = app.state::<ShareState>().peer_destinations();
                for destination in destinations {
                    let _ = sock.send_to(&payload, destination);
                }
                last_announce = Instant::now();
            }
            if discoverable && (refresh || last_scan.elapsed() >= SWEEP_INTERVAL) {
                scan_subnet(&sock, &payload);
                last_scan = Instant::now();
            }

            match sock.recv_from(&mut buffer) {
                Ok((length, from)) => {
                    let received = {
                        let state = app.state::<ShareState>();
                        register_beacon(&state, &buffer[..length], from, local)
                    };
                    if let Some(received) = received {
                        if received.fresh {
                            eprintln!("Shift share: found {} on {}", received.name, from.ip());
                        }
                        if discoverable && !received.reply {
                            let _ = sock.send_to(
                                &reply_payload,
                                SocketAddr::from((from.ip(), BEACON_PORT)),
                            );
                        }
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock
                            | std::io::ErrorKind::TimedOut
                            // Scanning closed ports makes Windows surface queued
                            // ICMP failures as WSAECONNRESET. They are not fatal.
                            | std::io::ErrorKind::ConnectionReset
                            | std::io::ErrorKind::ConnectionAborted
                    ) => {}
                Err(error) => {
                    eprintln!("Shift share beacon read failed: {error}");
                    match discovery_socket() {
                        Ok(replacement) => sock = replacement,
                        Err(retry) => {
                            eprintln!("Shift share retry failed: {retry}");
                            // Back off so a network the OS will not accept for us
                            // does not turn this thread into a busy loop.
                            thread::sleep(Duration::from_secs(2));
                        }
                    }
                    continue 'discovery;
                }
            }

            let watched = discoverable || app.state::<ShareState>().has_peers();
            if last_peer_check.elapsed() >= if watched { PEER_CHECK } else { PEER_CHECK_IDLE } {
                last_peer_check = Instant::now();
                let state = app.state::<ShareState>();
                state.expire();
                // Push list changes so the panel needs no polling.
                let current = peer_signature(&state);
                if signature.as_deref() != Some(current.as_str()) {
                    signature = Some(current);
                    emit_peers(app);
                }
            }
        }

        // Time to re-join multicast on whatever interface routes traffic now.
        if let Ok(replacement) = discovery_socket() {
            sock = replacement;
        }
    }
}

/// Identity of the visible peer set, so any change emits and repeats do not.
fn peer_signature(state: &ShareState) -> String {
    state
        .peers_visible()
        .iter()
        .map(|peer| peer.id.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

fn emit_peers(app: &AppHandle) {
    let peers = app.state::<ShareState>().peers_visible();
    let _ = app.emit("share://peers", peers);
}

/// Send the beacon once and report what this host can actually hear. Used by the
/// ignored discovery test when LAN behaviour needs debugging.
#[cfg(test)]
fn probe_network(payload: &[u8]) -> (bool, bool, String) {
    let listener = match discovery_socket() {
        Ok(sock) => sock,
        Err(error) => return (false, false, format!("bind failed: {error}")),
    };
    let talker = match UdpSocket::bind(("0.0.0.0", 0)) {
        Ok(sock) => sock,
        Err(error) => return (false, false, format!("bind failed: {error}")),
    };
    let _ = talker.set_broadcast(true);
    let mut buffer = [0u8; 2048];
    let mut hears = |destination: SocketAddr| {
        for _ in 0..3 {
            let _ = talker.send_to(payload, destination);
            if let Ok((length, _)) = listener.recv_from(&mut buffer) {
                if serde_json::from_slice::<Beacon>(&buffer[..length]).is_ok() {
                    return true;
                }
            }
        }
        false
    };
    let multicast = hears(SocketAddr::from((IpAddr::V4(GROUP), BEACON_PORT)));
    let directed = interface_broadcasts()
        .into_iter()
        .any(|address| hears(SocketAddr::from((IpAddr::V4(address), BEACON_PORT))));
    let limited = hears(SocketAddr::from((
        IpAddr::V4(Ipv4Addr::BROADCAST),
        BEACON_PORT,
    )));
    (
        multicast,
        directed || limited,
        format!(
            "multicast={multicast} directed={directed} limited={limited} interface={}",
            local_address()
        ),
    )
}

/// Best-effort primary IPv4 of this machine, for diagnostics.
#[cfg(test)]
fn local_address() -> String {
    let probe = match UdpSocket::bind(("0.0.0.0", 0)) {
        Ok(sock) => sock,
        Err(_) => return "unknown".into(),
    };
    // UDP connect does not send anything; it only selects the outbound route.
    if probe
        .connect(("239.255.255.255", BEACON_PORT))
        .and_then(|()| probe.local_addr())
        .is_ok()
    {
        return probe
            .local_addr()
            .map(|address| address.ip().to_string())
            .unwrap_or_else(|_| "unknown".into());
    }
    "unknown".into()
}

fn safe_name(raw: &str) -> Option<String> {
    let name = raw.replace('\\', "/");
    let name = name.rsplit('/').next().unwrap_or("").trim();
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    Some(name.chars().take(200).collect())
}

fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("file");
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{value}"))
        .unwrap_or_default();
    let mut index = 2;
    loop {
        let candidate = dir.join(format!("{stem} {index}{extension}"));
        if !candidate.exists() {
            return candidate;
        }
        index += 1;
    }
}

/// One file crossing the wire, in both directions.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct TransferHeader {
    file_name: String,
    size: u64,
    peer_name: String,
    transfer_id: String,
    /// A multi-file send reuses one batch id so the receiver asks once and accepts
    /// the rest of the batch on its own. Defaults keep a single-file send working
    /// against an older peer that never sends these fields.
    batch_id: String,
    batch_index: u32,
    batch_total: u32,
}

fn read_header<R: Read>(stream: &mut R) -> Result<TransferHeader, String> {
    let mut magic = [0u8; 4];
    stream
        .read_exact(&mut magic)
        .map_err(|error| error.to_string())?;
    if &magic != MAGIC {
        return Err("unexpected handshake".into());
    }
    let mut length = [0u8; 4];
    stream
        .read_exact(&mut length)
        .map_err(|error| error.to_string())?;
    let header_len = u32::from_be_bytes(length) as usize;
    if header_len == 0 || header_len > 64 * 1024 {
        return Err("bad header size".into());
    }
    let mut header = vec![0u8; header_len];
    stream
        .read_exact(&mut header)
        .map_err(|error| error.to_string())?;
    serde_json::from_slice(&header).map_err(|error| error.to_string())
}

fn write_header<W: Write>(stream: &mut W, header: &TransferHeader) -> Result<(), String> {
    let payload = serde_json::to_vec(header).map_err(|error| error.to_string())?;
    stream.write_all(MAGIC).map_err(|error| error.to_string())?;
    stream
        .write_all(&(payload.len() as u32).to_be_bytes())
        .map_err(|error| error.to_string())?;
    stream
        .write_all(&payload)
        .map_err(|error| error.to_string())
}

/// Receiver half: read the header, wait for the user's decision, then read the body
/// into `directory` and ack. `on_header` asks the user and must block until the answer
/// (or the timeout) is known; returning Err refuses the file before any bytes move.
fn accept_transfer<S, A, P>(
    stream: &mut S,
    directory: &Path,
    mut on_header: A,
    mut on_progress: P,
) -> Result<(TransferHeader, PathBuf), String>
where
    S: Read + Write,
    A: FnMut(&TransferHeader) -> Result<(), String>,
    P: FnMut(&TransferHeader, u64),
{
    let header = read_header(stream)?;
    let name = safe_name(&header.file_name).ok_or_else(|| "bad file name".to_string())?;
    // The answer travels back as one byte before any file data, so a refused offer
    // costs a packet instead of the whole payload.
    if let Err(error) = on_header(&header) {
        let _ = stream.write_all(&[0u8]);
        return Err(error);
    }
    stream
        .write_all(&[1u8])
        .map_err(|error| error.to_string())?;
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let target = unique_path(directory, &name);
    let temp = target.with_extension(format!("{}.shift-part", uuid::Uuid::new_v4()));

    let writing = || -> Result<(), String> {
        let mut file = fs::File::create(&temp).map_err(|error| error.to_string())?;
        let mut buffer = vec![0u8; 64 * 1024];
        let mut received = 0u64;
        while received < header.size {
            let want = std::cmp::min(buffer.len() as u64, header.size - received) as usize;
            let read = stream
                .read(&mut buffer[..want])
                .map_err(|error| error.to_string())?;
            if read == 0 {
                return Err("connection closed early".into());
            }
            file.write_all(&buffer[..read])
                .map_err(|error| error.to_string())?;
            received += read as u64;
            on_progress(&header, received);
        }
        file.flush().map_err(|error| error.to_string())
    }();

    if let Err(error) = writing {
        let _ = fs::remove_file(&temp);
        let _ = stream.write_all(&[0u8]);
        return Err(error);
    }
    fs::rename(&temp, &target).map_err(|error| error.to_string())?;
    let _ = stream.write_all(&[1u8]);
    Ok((header, target))
}

/// Sender half: introduce the file, wait for the peer's accept, then stream it and
/// wait for the write acknowledgement. Takes a concrete TcpStream because the two
/// waits need different socket read timeouts.
fn push_file<P>(
    stream: &mut TcpStream,
    source: &Path,
    header: &TransferHeader,
    mut on_progress: P,
) -> Result<(), String>
where
    P: FnMut(u64),
{
    write_header(stream, header)?;
    let mut decision = [0u8; 1];
    stream
        .set_read_timeout(Some(DECISION_TIMEOUT))
        .map_err(|error| error.to_string())?;
    // A timeout means the other side never answered; any other failure means its
    // card went away, which is a decline. The UI words these differently.
    match stream.read_exact(&mut decision) {
        Ok(()) => {}
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) =>
        {
            return Err("share-timeout".to_string());
        }
        Err(_) => return Err("share-declined".to_string()),
    }
    if decision[0] != 1 {
        return Err("share-declined".to_string());
    }
    // Body transfer may legitimately take a while; relax the read timeout again.
    stream
        .set_read_timeout(Some(Duration::from_secs(120)))
        .map_err(|error| error.to_string())?;
    let mut file = fs::File::open(source).map_err(|error| error.to_string())?;
    copy_file_body(&mut file, stream, header.size, &mut on_progress)?;
    stream.flush().map_err(|error| error.to_string())?;

    let mut ack = [0u8; 1];
    stream
        .read_exact(&mut ack)
        .map_err(|_| "share-no-ack".to_string())?;
    if ack[0] != 1 {
        return Err("share-rejected".to_string());
    }
    Ok(())
}

/// Copy exactly the number of bytes the header promised. Reading a fixed-size
/// chunk would let a source file that grew after `metadata` push extra bytes into
/// the stream, where the sender would mistake them for the receiver's ack.
fn copy_file_body<R: Read, W: Write, P: FnMut(u64)>(
    reader: &mut R,
    writer: &mut W,
    size: u64,
    mut on_progress: P,
) -> Result<(), String> {
    let mut buffer = vec![0u8; 64 * 1024];
    let mut sent = 0u64;
    while sent < size {
        let want = std::cmp::min(buffer.len() as u64, size - sent) as usize;
        let read = reader
            .read(&mut buffer[..want])
            .map_err(|error| error.to_string())?;
        if read == 0 {
            return Err("The source file changed while it was being sent.".into());
        }
        writer
            .write_all(&buffer[..read])
            .map_err(|error| error.to_string())?;
        sent += read as u64;
        on_progress(sent);
    }
    Ok(())
}

/// Ask the user about an incoming file and block until they answer. The notice
/// window owns the buttons; this thread only waits, so a decision still arrives
/// correctly when Shift has been idle in the menu bar.
fn await_decision(app: &AppHandle, header: &TransferHeader) -> Result<(), String> {
    let state = app.state::<ShareState>();
    // A later file of a batch the user already approved arrives without a prompt.
    if state.batch_accepted(header) {
        return Ok(());
    }
    let offer = PendingShare {
        transfer_id: header.transfer_id.clone(),
        file_name: safe_name(&header.file_name).unwrap_or_else(|| "file".into()),
        size: header.size,
        peer_name: header.peer_name.clone(),
        timeout_seconds: DECISION_TIMEOUT.as_secs(),
    };
    let (respond, answer) = mpsc::channel();
    let notice_id = format!("share-{}", uuid::Uuid::new_v4());
    state.add_pending(&offer.transfer_id, respond, &notice_id);
    if let Err(error) = show_notice(app, &notice_id, &offer) {
        state.forget_pending(&offer.transfer_id);
        return Err(format!("share-ui-unavailable: {error}"));
    }

    match answer
        .recv_timeout(DECISION_TIMEOUT)
        .unwrap_or(Decision::Expired)
    {
        Decision::Accepted => {
            state.forget_pending(&offer.transfer_id);
            state.accept_batch(header);
            close_notice(app, &notice_id);
            Ok(())
        }
        Decision::Rejected => {
            close_notice(app, &notice_id);
            Err("share-refused".to_string())
        }
        Decision::Expired => {
            state.forget_pending(&offer.transfer_id);
            close_notice(app, &notice_id);
            Err("share-expired".to_string())
        }
    }
}

/// Small always-on-top card asking whether to accept an incoming file. One window
/// per offer, like the pinned images, so concurrent offers simply stack.
fn show_notice(app: &AppHandle, notice_id: &str, offer: &PendingShare) -> Result<(), String> {
    let query = serde_json::to_vec(offer)
        .map(|bytes| hex_encode(&String::from_utf8_lossy(&bytes)))
        .map_err(|error| error.to_string())?;
    let (width, height) = (400.0_f64, 190.0_f64);
    let mut builder = tauri::WebviewWindowBuilder::new(
        app,
        notice_id,
        tauri::WebviewUrl::App(format!("index.html?view=share-notice&offer={query}").into()),
    )
    .title("Shift")
    .inner_size(width, height)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .shadow(true)
    .focused(true);
    if let Ok(Some(monitor)) = app.primary_monitor() {
        let size = monitor.size().to_logical::<f64>(monitor.scale_factor());
        builder = builder.position(
            (size.width - width - 28.0).max(16.0),
            (size.height - height - 28.0).max(16.0),
        );
    }
    builder
        .build()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn close_notice(app: &AppHandle, notice_id: &str) {
    if let Some(window) = app.get_webview_window(notice_id) {
        let _ = window.close();
    }
}

fn hex_encode(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn receive_file(app: &AppHandle, mut stream: TcpStream) -> Result<(), String> {
    // The receiver holds the connection open while the user decides.
    stream
        .set_read_timeout(Some(DECISION_TIMEOUT + Duration::from_secs(15)))
        .map_err(|error| error.to_string())?;
    let directory = crate::settings::received_dir(app)?;
    let mut last_emit = Instant::now();
    let approved = std::cell::Cell::new(false);
    let current = std::cell::RefCell::new(None);
    let result = accept_transfer(
        &mut stream,
        &directory,
        |header| {
            *current.borrow_mut() = Some(header.clone());
            let answer = await_decision(app, header);
            if answer.is_ok() {
                approved.set(true);
            }
            answer
        },
        |header, received| {
            // Throttle progress so a fast LAN link does not flood the UI.
            if last_emit.elapsed() < Duration::from_millis(120) {
                return;
            }
            last_emit = Instant::now();
            emit_transfer(
                app,
                TransferEvent {
                    transfer_id: header.transfer_id.clone(),
                    file_name: safe_name(&header.file_name).unwrap_or_else(|| "file".into()),
                    peer_name: header.peer_name.clone(),
                    sent: received,
                    total: header.size,
                    state: "sending".into(),
                    path: None,
                },
            );
        },
    );
    let (header, target) = match result {
        Ok(value) => value,
        Err(error) => {
            if approved.get() {
                if let Some(header) = current.borrow().clone() {
                    emit_transfer(
                        app,
                        TransferEvent {
                            transfer_id: header.transfer_id,
                            file_name: safe_name(&header.file_name)
                                .unwrap_or_else(|| "file".into()),
                            peer_name: header.peer_name,
                            sent: 0,
                            total: header.size,
                            state: "error".into(),
                            path: None,
                        },
                    );
                }
            }
            return Err(error);
        }
    };
    let name = safe_name(&header.file_name).unwrap_or_else(|| "file".into());
    let path = target.to_string_lossy().to_string();
    // Read the batch fields before the event below moves out of `header`.
    let reveal = reveals_on_landing(&header);
    emit_transfer(
        app,
        TransferEvent {
            transfer_id: header.transfer_id,
            file_name: name.clone(),
            peer_name: header.peer_name,
            sent: header.size,
            total: header.size,
            state: "received".into(),
            path: Some(path.clone()),
        },
    );
    // Reuse the result pipeline so received files land in the history library.
    crate::history::record(app, std::slice::from_ref(&path));
    let _ = app.emit(
        "app://file-result",
        serde_json::json!({ "outputs": [path], "message": name }),
    );
    // A landed file follows the same rule as a finished conversion: the
    // "show results in Finder" switch decides whether Finder comes forward.
    if reveal && crate::settings::auto_reveal(app) {
        let _ = crate::files::reveal_path(Path::new(&target));
    }
    Ok(())
}

/// Whether a file that just landed should bring the file manager forward. A batch
/// answers once, when its last file arrives, the way a batch conversion reveals one
/// output instead of one per file. The `saturating_add` and the `>=` keep a legacy
/// peer's header - which sends no batch fields, so both read 0 - still revealing.
fn reveals_on_landing(header: &TransferHeader) -> bool {
    header.batch_index.saturating_add(1) >= header.batch_total
}

fn emit_transfer(app: &AppHandle, event: TransferEvent) {
    let _ = app.emit("share://transfer", event);
}

/// Answer an incoming offer. Returns false when nothing was waiting for this id,
/// which happens when the offer expired between render and click.
#[tauri::command]
pub fn share_answer(
    state: tauri::State<'_, ShareState>,
    transfer_id: String,
    accepted: bool,
) -> bool {
    state.resolve_pending(&transfer_id, accepted).is_some()
}

/// Ask the long-lived discovery socket for peers right away. A second socket on
/// Windows cannot bind the shared port, and on unix it can steal the reply, so the
/// panel wakes the existing thread instead.
#[tauri::command]
pub async fn share_refresh(app: AppHandle) -> Vec<Peer> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<ShareState>();
        if state.is_enabled() {
            state.request_refresh();
            // The discovery loop wakes from its 400 ms read timeout, answers, and
            // emits peers while this command returns the refreshed snapshot.
            thread::sleep(Duration::from_millis(700));
        }
        state.peers_visible()
    })
    .await
    .unwrap_or_default()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendResult {
    pub name: String,
    pub size: u64,
    pub peer: String,
}

/// Send one or more files to a peer. A multi-file send shares one batch id, so the
/// receiver prompts once and takes the rest of the batch on trust.
#[tauri::command]
pub async fn share_send(
    app: AppHandle,
    peer_id: String,
    paths: Vec<String>,
) -> Result<Vec<SendResult>, String> {
    if paths.is_empty() {
        return Err("share-no-files".to_string());
    }
    let (peer_name, address) = app
        .state::<ShareState>()
        .peer(&peer_id)
        .ok_or_else(|| "share-peer-gone".to_string())?;
    let local_name = app.state::<ShareState>().local_beacon().name;
    let batch_id = uuid::Uuid::new_v4().to_string();
    let total = paths.len() as u32;
    let reporter = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut sent: Vec<SendResult> = Vec::with_capacity(paths.len());
        for (index, path) in paths.iter().enumerate() {
            let outcome = send_file(
                &reporter,
                &uuid::Uuid::new_v4().to_string(),
                &batch_id,
                index as u32,
                total,
                path,
                address,
                &peer_name,
                &local_name,
            );
            // Stop at the first refusal: continuing would prompt the peer again
            // for every remaining file in the batch.
            sent.push(outcome?);
        }
        Ok(sent)
    })
    .await
    .map_err(|error| format!("share send failed: {error}"))?
}

#[allow(clippy::too_many_arguments)]
fn send_file(
    app: &AppHandle,
    transfer_id: &str,
    batch_id: &str,
    batch_index: u32,
    batch_total: u32,
    path: &str,
    address: SocketAddr,
    peer_name: &str,
    local_name: &str,
) -> Result<SendResult, String> {
    let name = Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("file");
    let size = fs::metadata(path).map_err(|error| error.to_string())?.len();
    let mut stream = TcpStream::connect(address).map_err(|_| "share-unreachable".to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(60)))
        .map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(120)))
        .map_err(|error| error.to_string())?;

    let header = TransferHeader {
        file_name: name.into(),
        size,
        peer_name: local_name.into(),
        transfer_id: transfer_id.into(),
        batch_id: batch_id.into(),
        batch_index,
        batch_total,
    };
    let mut last_emit = Instant::now();
    push_file(&mut stream, Path::new(path), &header, |sent| {
        if last_emit.elapsed() >= Duration::from_millis(120) {
            last_emit = Instant::now();
            emit_transfer(
                app,
                TransferEvent {
                    transfer_id: transfer_id.into(),
                    file_name: name.into(),
                    peer_name: peer_name.into(),
                    sent,
                    total: size,
                    state: "sending".into(),
                    path: None,
                },
            );
        }
    })?;

    emit_transfer(
        app,
        TransferEvent {
            transfer_id: transfer_id.into(),
            file_name: name.into(),
            peer_name: peer_name.into(),
            sent: size,
            total: size,
            state: "received".into(),
            path: None,
        },
    );
    Ok(SendResult {
        name: name.into(),
        size,
        peer: peer_name.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beacon() -> Beacon {
        Beacon {
            id: "abc".into(),
            name: "Mini".into(),
            platform: "macos".into(),
            port: 51_000,
            reply: false,
        }
    }

    #[test]
    fn beacon_round_trips() {
        let decoded: Beacon =
            serde_json::from_slice(&serde_json::to_vec(&beacon()).unwrap()).unwrap();
        assert_eq!(decoded.port, 51_000);
        assert_eq!(decoded.name, "Mini");
    }

    #[test]
    fn incomplete_beacons_are_ignored() {
        assert!(
            serde_json::from_str::<Beacon>(r#"{"name":"a"}"#)
                .unwrap()
                .port
                == 0
        );
        assert!(serde_json::from_str::<Beacon>("not json").is_err());
    }

    #[test]
    fn names_cannot_escape_the_output_folder() {
        assert_eq!(safe_name("../../etc/passwd").unwrap(), "passwd");
        assert_eq!(safe_name(r"C:\temp\note.txt").unwrap(), "note.txt");
        assert_eq!(safe_name("ok.pdf").unwrap(), "ok.pdf");
        assert!(safe_name("../").is_none());
        assert!(safe_name("  ").is_none());
    }

    #[test]
    fn duplicate_names_get_a_suffix() {
        let dir = std::env::temp_dir().join(format!("shift-share-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("report.pdf"), b"x").unwrap();
        assert_eq!(
            unique_path(&dir, "report.pdf").file_name().unwrap(),
            "report 2.pdf"
        );
        assert_eq!(
            unique_path(&dir, "fresh.txt").file_name().unwrap(),
            "fresh.txt"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn expired_peers_leave_the_list() {
        let state = ShareState::default();
        let address = SocketAddr::from(([192, 168, 1, 20], 50_000));
        assert!(state.note(&beacon(), address));
        assert!(!state.note(&beacon(), address));
        assert_eq!(state.peers_visible().len(), 1);
        // The UDP source port only tells us the host; the TCP port comes from the beacon.
        assert_eq!(
            state.peer("abc").unwrap().1,
            SocketAddr::from(([192, 168, 1, 20], 51_000))
        );
        assert!(!state.expire(), "a fresh peer must survive");
        state.peers.lock().unwrap().get_mut("abc").unwrap().seen =
            Instant::now() - PEER_TIMEOUT - Duration::from_secs(1);
        assert!(state.expire());
        assert!(state.peers_visible().is_empty());
    }

    #[test]
    fn the_peer_table_stops_accepting_new_ids_at_its_limit() {
        let state = ShareState::default();
        let address = SocketAddr::from(([192, 168, 1, 20], 50_000));
        for index in 0..MAX_PEERS {
            let mut peer = beacon();
            peer.id = format!("peer-{index}");
            assert!(state.note(&peer, address));
        }

        let mut extra = beacon();
        extra.id = "one-too-many".into();
        assert!(!state.note(&extra, address));
        assert_eq!(state.peers_visible().len(), MAX_PEERS);

        let mut known = beacon();
        known.id = "peer-0".into();
        assert!(!state.note(&known, address), "refreshing is not a new peer");
        assert_eq!(state.peer("peer-0").unwrap().1.port(), 51_000);
    }

    #[test]
    fn peer_timeout_covers_a_legacy_peers_reply_cycle() {
        // Versions before the reply field answer once, forget this host after nine
        // seconds, then answer the next unicast refresh two seconds later.
        let legacy_expiry = Duration::from_secs(9);
        assert!(PEER_TIMEOUT > legacy_expiry + BEACON_INTERVAL);
    }

    #[test]
    fn discovery_is_enabled_by_default_and_can_be_silenced() {
        let state = ShareState::default();
        assert!(state.is_enabled());
        assert!(!state.take_refresh_request());
        state.request_refresh();
        assert!(state.take_refresh_request());
        assert!(!state.take_refresh_request(), "a refresh is consumed once");
        state.enabled.store(false, Ordering::Relaxed);
        assert!(!state.is_enabled());
    }

    /// The distinction the discovery thread's idle cadence is built on.
    ///
    /// Switching ShiftDrop off means "stop announcing", never "stop listening": a
    /// silent copy still hears beacons, so another machine can still find and send to
    /// it, and `expire()` still runs so a peer that went away really leaves the list.
    /// A change that made a disabled copy deaf would look like a CPU win and quietly
    /// break sending, so it is pinned here rather than only in a comment.
    #[test]
    fn a_silent_copy_still_knows_and_still_loses_peers() {
        let state = ShareState::default();
        let address: SocketAddr = "192.168.1.40:51234".parse().unwrap();
        assert!(!state.has_peers(), "a fresh copy knows nobody");
        state.enabled.store(false, Ordering::Relaxed);
        assert!(
            state.note(&beacon(), address),
            "a silent copy must still record a peer it hears"
        );
        assert!(state.has_peers());
        // The slow bookkeeping tick still has something to expire; if `expire` only
        // ran while enabled, this peer would sit in the list forever pretending to be live.
        state
            .peers
            .lock()
            .unwrap()
            .values_mut()
            .for_each(|peer| peer.seen = Instant::now() - PEER_TIMEOUT);
        assert!(state.expire(), "a silent copy must still expire its peers");
        assert!(!state.has_peers());
    }
    #[test]
    fn directed_broadcasts_follow_the_interface_netmask() {
        assert_eq!(
            directed_broadcast(
                Ipv4Addr::new(192, 168, 2, 34),
                Ipv4Addr::new(255, 255, 255, 0)
            ),
            Some(Ipv4Addr::new(192, 168, 2, 255))
        );
        assert_eq!(
            directed_broadcast(
                Ipv4Addr::new(10, 20, 30, 40),
                Ipv4Addr::new(255, 255, 240, 0)
            ),
            Some(Ipv4Addr::new(10, 20, 31, 255))
        );
        assert_eq!(
            directed_broadcast(Ipv4Addr::new(127, 0, 0, 1), Ipv4Addr::new(255, 0, 0, 0)),
            None
        );
        assert_eq!(
            directed_broadcast(Ipv4Addr::new(169, 254, 1, 2), Ipv4Addr::new(255, 255, 0, 0)),
            None
        );
        assert_eq!(
            directed_broadcast(
                Ipv4Addr::new(192, 168, 2, 34),
                Ipv4Addr::new(255, 255, 255, 255)
            ),
            None
        );
    }

    #[test]
    fn interface_broadcasts_are_unique_and_usable() {
        let broadcasts = interface_broadcasts();
        let mut unique = broadcasts.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(broadcasts, unique);
        assert!(broadcasts
            .iter()
            .all(|address| !address.is_unspecified() && !address.is_loopback()));
    }

    #[test]
    fn subnet_scan_skips_this_host_and_broadcast() {
        let addresses = subnet_scan_addresses(
            Ipv4Addr::new(192, 168, 2, 34),
            Ipv4Addr::new(255, 255, 255, 0),
        );
        assert_eq!(addresses.len(), 253);
        assert!(addresses.contains(&Ipv4Addr::new(192, 168, 2, 1)));
        assert!(addresses.contains(&Ipv4Addr::new(192, 168, 2, 254)));
        assert!(!addresses.contains(&Ipv4Addr::new(192, 168, 2, 34)));
        assert!(!addresses.contains(&Ipv4Addr::BROADCAST));
    }

    #[test]
    fn subnet_scan_bounds_a_large_network_to_this_twenty_four() {
        let addresses =
            subnet_scan_addresses(Ipv4Addr::new(10, 20, 30, 40), Ipv4Addr::new(255, 255, 0, 0));
        assert_eq!(addresses.len(), 253);
        assert!(addresses.contains(&Ipv4Addr::new(10, 20, 30, 1)));
        assert!(addresses.contains(&Ipv4Addr::new(10, 20, 30, 254)));
        assert!(!addresses.contains(&Ipv4Addr::new(10, 20, 31, 1)));
    }

    #[test]
    fn reply_beacons_are_distinguished_from_regular_ones() {
        let state = ShareState::default();
        let local = Beacon {
            id: "me".into(),
            name: "Me".into(),
            platform: "macos".into(),
            port: 40_000,
            reply: false,
        };
        let mut peer = beacon();
        peer.reply = true;
        let received = register_beacon(
            &state,
            &serde_json::to_vec(&peer).unwrap(),
            SocketAddr::from(([192, 168, 1, 9], 55_999)),
            &local,
        )
        .unwrap();
        assert!(received.reply);
        assert!(received.fresh);
    }

    #[test]
    fn both_readmes_document_the_real_discovery_address() {
        let expected = format!("{GROUP}:{BEACON_PORT}");
        for readme in ["../README.md", "../README.zh-CN.md"] {
            let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(readme);
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            assert!(
                text.contains(&expected),
                "{} documents the wrong discovery address",
                path.display()
            );
        }
    }

    #[test]
    fn magic_is_four_bytes() {
        assert_eq!(MAGIC.len(), 4);
    }

    /// A landed file reveals once per batch, never once per file, and a peer that
    /// sends no batch fields still reveals.
    #[test]
    fn only_the_last_file_of_a_batch_reveals() {
        let single = TransferHeader {
            batch_index: 0,
            batch_total: 1,
            ..Default::default()
        };
        assert!(reveals_on_landing(&single));
        let first = TransferHeader {
            batch_index: 0,
            batch_total: 3,
            ..Default::default()
        };
        let middle = TransferHeader {
            batch_index: 1,
            batch_total: 3,
            ..Default::default()
        };
        let last = TransferHeader {
            batch_index: 2,
            batch_total: 3,
            ..Default::default()
        };
        assert!(!reveals_on_landing(&first));
        assert!(!reveals_on_landing(&middle));
        assert!(reveals_on_landing(&last));
        assert!(reveals_on_landing(&TransferHeader::default()));
    }

    #[test]
    fn peer_signature_changes_only_with_the_peer_set() {
        let state = ShareState::default();
        let local = Beacon {
            id: "me".into(),
            name: "Me".into(),
            platform: "macos".into(),
            port: 40_000,
            reply: false,
        };
        let empty = peer_signature(&state);
        assert_eq!(
            register_beacon(
                &state,
                &serde_json::to_vec(&beacon()).unwrap(),
                SocketAddr::from(([192, 168, 1, 9], 55_999)),
                &local
            )
            .map(|received| received.name)
            .as_deref(),
            Some("Mini")
        );
        let after = peer_signature(&state);
        assert_ne!(empty, after, "a new peer must change the signature");
        assert_eq!(
            after,
            peer_signature(&state),
            "a repeat beacon must not change it"
        );
    }

    #[test]
    fn beacons_turn_into_peers_once_each() {
        let state = ShareState::default();
        let local = Beacon {
            id: "me".into(),
            name: "Me".into(),
            platform: "macos".into(),
            port: 40_000,
            reply: false,
        };
        let from = SocketAddr::from(([192, 168, 1, 9], 55_999));
        let bytes = serde_json::to_vec(&beacon()).unwrap();

        let first = register_beacon(&state, &bytes, from, &local).unwrap();
        assert_eq!(first.name, "Mini");
        assert!(first.fresh);
        assert!(!first.reply);
        assert_eq!(state.peers_visible().len(), 1);
        let repeated = register_beacon(&state, &bytes, from, &local).unwrap();
        assert!(!repeated.fresh, "a known peer must not log again");
        assert_eq!(state.peers_visible().len(), 1);
    }

    #[test]
    fn own_and_malformed_beacons_never_become_peers() {
        let state = ShareState::default();
        let local = Beacon {
            id: "abc".into(),
            name: "Mini".into(),
            platform: "macos".into(),
            port: 40_000,
            reply: false,
        };
        let from = SocketAddr::from(([192, 168, 1, 9], 55_999));
        let own = serde_json::to_vec(&beacon()).unwrap();
        assert_eq!(register_beacon(&state, &own, from, &local), None);
        assert_eq!(register_beacon(&state, b"nonsense", from, &local), None);
        assert_eq!(
            register_beacon(&state, br#"{"id":"x","name":"x","port":0}"#, from, &local),
            None
        );
        assert!(state.peers_visible().is_empty());
    }

    #[test]
    fn a_file_crosses_the_wire_and_acks() {
        let dir = std::env::temp_dir().join(format!("shift-wire-{}", uuid::Uuid::new_v4()));
        let source = dir.join("payload.bin");
        fs::create_dir_all(&dir).unwrap();
        let body: Vec<u8> = (0..300_000u32).map(|index| (index % 251) as u8).collect();
        fs::write(&source, &body).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let inbound = dir.join("in");
        let received_dir = inbound.clone();
        let receiver = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            accept_transfer(&mut stream, &received_dir, |_| Ok(()), |_, _| {})
        });

        let mut stream = TcpStream::connect(address).unwrap();
        let header = TransferHeader {
            file_name: "payload.bin".into(),
            size: body.len() as u64,
            peer_name: "Mini".into(),
            transfer_id: "t-1".into(),
            ..Default::default()
        };
        push_file(&mut stream, &source, &header, |_| {}).unwrap();

        let (got, target) = receiver.join().unwrap().unwrap();
        assert_eq!(got.transfer_id, "t-1");
        assert_eq!(fs::read(&target).unwrap(), body);
        // No stray .shift-part files are left behind.
        assert_eq!(fs::read_dir(&inbound).unwrap().count(), 1);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn bytes_after_the_declared_size_never_cross_the_wire() {
        let dir = std::env::temp_dir().join(format!("shift-long-{}", uuid::Uuid::new_v4()));
        let source = dir.join("grew.bin");
        fs::create_dir_all(&dir).unwrap();
        let body: Vec<u8> = (0..130_000u32).map(|index| (index % 251) as u8).collect();
        fs::write(&source, &body).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let inbound = dir.join("in");
        let received_dir = inbound.clone();
        let receiver = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            accept_transfer(&mut stream, &received_dir, |_| Ok(()), |_, _| {})
        });

        let mut stream = TcpStream::connect(address).unwrap();
        let header = TransferHeader {
            file_name: "grew.bin".into(),
            size: 1_000,
            peer_name: "Mini".into(),
            transfer_id: "t-long".into(),
            ..Default::default()
        };
        push_file(&mut stream, &source, &header, |_| {}).unwrap();
        drop(stream);

        let (_, target) = receiver.join().unwrap().unwrap();
        assert_eq!(fs::read(target).unwrap(), body[..1_000]);
        assert_eq!(fs::read_dir(&inbound).unwrap().count(), 1);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_source_that_shrinks_is_refused_without_a_partial_file() {
        let dir = std::env::temp_dir().join(format!("shift-short-{}", uuid::Uuid::new_v4()));
        let source = dir.join("shrank.bin");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&source, b"only some of the promised bytes").unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let inbound = dir.join("in");
        let received_dir = inbound.clone();
        let receiver = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            accept_transfer(&mut stream, &received_dir, |_| Ok(()), |_, _| {})
        });

        let mut stream = TcpStream::connect(address).unwrap();
        let header = TransferHeader {
            file_name: "shrank.bin".into(),
            size: 1_000,
            peer_name: "Mini".into(),
            transfer_id: "t-short".into(),
            ..Default::default()
        };
        let error = push_file(&mut stream, &source, &header, |_| {}).unwrap_err();
        assert!(error.contains("source file changed"), "{error}");
        drop(stream);

        assert!(receiver.join().unwrap().is_err());
        assert_eq!(fs::read_dir(&inbound).unwrap().count(), 0);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_truncated_stream_is_rejected_without_a_partial_file() {
        let dir = std::env::temp_dir().join(format!("shift-cut-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let inbound = dir.join("in");
        let received_dir = inbound.clone();
        let receiver = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            accept_transfer(&mut stream, &received_dir, |_| Ok(()), |_, _| {}).map(|(_, path)| path)
        });

        let mut stream = TcpStream::connect(address).unwrap();
        let header = TransferHeader {
            file_name: "big.bin".into(),
            size: 4096,
            peer_name: "Mini".into(),
            transfer_id: "t-2".into(),
            ..Default::default()
        };
        write_header(&mut stream, &header).unwrap();
        stream.write_all(b"only a little").unwrap();
        drop(stream);

        assert!(receiver.join().unwrap().is_err());
        assert_eq!(fs::read_dir(&inbound).unwrap().count(), 0);
        fs::remove_dir_all(&dir).ok();
    }

    /// Live discovery checks, in one test because both bind the single shared
    /// discovery port and would otherwise steal each other's beacons: this host
    /// must deliver a beacon to the shared port, and two copies on one machine
    /// must see each other. Run when debugging LAN behaviour with
    /// `cargo test share:: -- --ignored --nocapture`.
    #[test]
    #[ignore = "binds the shared discovery port"]
    fn two_copies_on_one_machine_discover_each_other() {
        let payload = serde_json::to_vec(&beacon()).unwrap();
        let (multicast, broadcast, detail) = probe_network(&payload);
        println!("share probe: {detail}");
        assert!(multicast || broadcast, "no discovery path works: {detail}");

        let left_state = ShareState::default();
        let right_state = ShareState::default();
        let left = Beacon {
            id: "left".into(),
            name: "Left".into(),
            platform: "macos".into(),
            port: 40_001,
            reply: false,
        };
        let right = Beacon {
            id: "right".into(),
            name: "Right".into(),
            platform: "windows".into(),
            port: 40_002,
            reply: false,
        };
        let left_sock = discovery_socket().expect("left socket");
        let right_sock = discovery_socket().expect("right socket");
        left_sock
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        right_sock
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();

        // Each copy keeps announcing while the other listens, like the beacon loop.
        fn announcer(sock: UdpSocket, beacon: Beacon) -> thread::JoinHandle<()> {
            thread::spawn(move || {
                let payload = serde_json::to_vec(&beacon).unwrap();
                for _ in 0..40 {
                    announce(&sock, &payload);
                    thread::sleep(Duration::from_millis(100));
                }
            })
        }
        let left_talker = announcer(left_sock.try_clone().unwrap(), left.clone());
        let right_talker = announcer(right_sock.try_clone().unwrap(), right.clone());

        let mut buffer = [0u8; 2048];
        let mut hear = |state: &ShareState, local: &Beacon, want: &str| -> bool {
            let sock = if local.id == "left" {
                &left_sock
            } else {
                &right_sock
            };
            for _ in 0..40 {
                if let Ok((length, from)) = sock.recv_from(&mut buffer) {
                    if register_beacon(state, &buffer[..length], from, local)
                        .is_some_and(|received| received.name == want)
                    {
                        return true;
                    }
                }
            }
            false
        };

        let left_saw = hear(&left_state, &left, "Right");
        let right_saw = hear(&right_state, &right, "Left");
        left_talker.join().unwrap();
        right_talker.join().unwrap();

        assert!(left_saw, "left copy never heard the right copy");
        assert_eq!(left_state.peers_visible()[0].name, "Right");
        let address = left_state.peer("right").unwrap().1;
        assert_eq!(
            address.port(),
            40_002,
            "the TCP port must come from the beacon, not the UDP source port"
        );
        assert!(!address.ip().is_unspecified());
        assert!(right_saw, "right copy never heard the left copy");
        assert_eq!(right_state.peers_visible()[0].name, "Left");
    }

    #[test]
    fn a_bad_handshake_is_refused() {
        let dir = std::env::temp_dir().join(format!("shift-bad-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let mut cursor = std::io::Cursor::new(b"JUNK".to_vec());
        assert!(read_header(&mut cursor).is_err());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_declined_offer_moves_no_bytes_and_leaves_no_file() {
        let dir = std::env::temp_dir().join(format!("shift-decline-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let inbound = dir.join("in");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let received_dir = inbound.clone();
        let receiver = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            // The user pressed decline, so refuse before touching the body.
            accept_transfer(
                &mut stream,
                &received_dir,
                |_| Err("share-refused".into()),
                |_, _| {},
            )
            .map(|_| ())
        });

        let mut stream = TcpStream::connect(address).unwrap();
        let header = TransferHeader {
            file_name: "nope.bin".into(),
            size: 4_000,
            peer_name: "Mini".into(),
            transfer_id: "t-3".into(),
            ..Default::default()
        };
        let error = push_file(&mut stream, &dir.join("missing.bin"), &header, |_| {})
            .err()
            .unwrap_or_default();
        assert_eq!(
            error, "share-declined",
            "the sender must learn it was refused"
        );
        assert!(receiver.join().unwrap().is_err());
        // A refused offer is refused before the output folder is even created, so
        // either it does not exist or it is still empty.
        assert!(
            !inbound.exists() || fs::read_dir(&inbound).unwrap().next().is_none(),
            "a refused file must not reach the disk"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_accepted_offer_asks_once_and_saves_the_body() {
        let dir = std::env::temp_dir().join(format!("shift-accept-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("note.txt");
        fs::write(&source, b"accepted body").unwrap();
        let inbound = dir.join("in");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let received_dir = inbound.clone();
        let receiver = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut asked = 0usize;
            let mut last = 0u64;
            accept_transfer(
                &mut stream,
                &received_dir,
                |_| {
                    asked += 1;
                    Ok(())
                },
                |_, received| last = received,
            )
            .map(|(_, path)| (asked, last, path))
        });

        let mut stream = TcpStream::connect(address).unwrap();
        let header = TransferHeader {
            file_name: "note.txt".into(),
            size: 13,
            peer_name: "Mini".into(),
            transfer_id: "t-4".into(),
            ..Default::default()
        };
        push_file(&mut stream, &source, &header, |_| {}).unwrap();

        let (asked, last, target) = receiver.join().unwrap().unwrap();
        assert_eq!(asked, 1, "the user is asked exactly once per offer");
        assert_eq!(last, 13, "progress must reach the full size");
        assert_eq!(fs::read(&target).unwrap(), b"accepted body");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn pending_offers_answer_once_and_remember_their_notice() {
        let state = ShareState::default();
        let (respond, answer) = mpsc::channel();
        state.add_pending("t-1", respond, "notice-1");

        assert_eq!(
            state.resolve_pending("t-1", true),
            Some("notice-1".into()),
            "the answer must name the notice to dismiss"
        );
        assert!(answer.recv().is_ok(), "the waiting thread must wake up");
        assert_eq!(
            state.resolve_pending("t-1", true),
            None,
            "a second click on the same offer must be inert"
        );

        let (second, second_answer) = mpsc::channel();
        state.add_pending("t-2", second, "notice-2");
        state.forget_pending("t-2");
        assert_eq!(
            state.resolve_pending("t-2", true),
            None,
            "a forgotten offer cannot be answered"
        );
        assert!(
            matches!(
                second_answer.recv_timeout(Duration::from_millis(50)),
                Err(mpsc::RecvTimeoutError::Disconnected)
            ),
            "forgetting an offer must release the waiting thread"
        );
    }

    fn batch_header(index: u32, total: u32, id: &str) -> TransferHeader {
        TransferHeader {
            file_name: format!("part-{index}.bin"),
            size: 4,
            peer_name: "Mini".into(),
            transfer_id: format!("t-{index}"),
            batch_id: id.into(),
            batch_index: index,
            batch_total: total,
        }
    }

    #[test]
    fn an_accepted_batch_prompts_once_and_a_later_batch_asks_again() {
        let state = ShareState::default();
        let first = batch_header(0, 2, "batch-a");
        let second = batch_header(1, 2, "batch-a");

        assert!(!state.batch_accepted(&first), "a new batch must prompt");
        state.accept_batch(&first);
        assert!(
            state.batch_accepted(&second),
            "the rest of the batch is trusted"
        );
        assert!(
            !state.batch_accepted(&batch_header(0, 2, "batch-b")),
            "a later, unrelated batch prompts again"
        );
        // A single-file send has no batch, so it must never be silently accepted.
        let single = batch_header(0, 1, "batch-a");
        assert!(!state.batch_accepted(&single));
        assert!(!state.batch_accepted(&batch_header(0, 3, "")));
    }

    #[test]
    fn a_whole_batch_crosses_the_wire_on_one_answer() {
        let dir = std::env::temp_dir().join(format!("shift-batch-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let inbound = dir.join("in");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let state = std::sync::Arc::new(ShareState::default());
        let received_dir = inbound.clone();
        let receiver_state = state.clone();
        let receiver = thread::spawn(move || {
            let mut asked = 0usize;
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let stored = accept_transfer(
                    &mut stream,
                    &received_dir,
                    |header| {
                        if receiver_state.batch_accepted(header) {
                            return Ok(());
                        }
                        asked += 1;
                        receiver_state.accept_batch(header);
                        Ok(())
                    },
                    |_, _| {},
                );
                stored?;
            }
            Ok::<usize, String>(asked)
        });

        for index in 0..2u32 {
            let source = dir.join(format!("part-{index}.bin"));
            fs::write(&source, b"data").unwrap();
            let mut stream = TcpStream::connect(address).unwrap();
            push_file(
                &mut stream,
                &source,
                &batch_header(index, 2, "batch-a"),
                |_| {},
            )
            .unwrap();
        }

        assert_eq!(
            receiver.join().unwrap().unwrap(),
            1,
            "the user is asked once"
        );
        assert_eq!(fs::read_dir(&inbound).unwrap().count(), 2);
        fs::remove_dir_all(&dir).ok();
    }
}
