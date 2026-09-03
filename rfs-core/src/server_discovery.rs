use bincode::Options;
use serde::{Deserialize, Serialize};
use std::net::UdpSocket;
use std::time::Instant;

/// UDP port for server broadcast beacons.
pub const DISCOVERY_PORT: u16 = 27016;

/// Magic bytes to identify RFS discovery packets.
pub const DISCOVERY_MAGIC: [u8; 4] = [0x52, 0x46, 0x53, 0x01]; // "RFS" + version

/// Maximum size of a discovery beacon packet.
const MAX_BEACON_SIZE: usize = 512;

/// Information about a discovered server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerBeacon {
    /// Server name.
    pub name: String,
    /// Server IP address.
    pub address: String,
    /// Server port.
    pub port: u16,
    /// Current map name.
    pub map: String,
    /// Current game mode.
    pub game_mode: String,
    /// Current player count.
    pub players: u32,
    /// Maximum player count.
    pub max_players: u32,
    /// Server version.
    pub version: String,
    /// Whether the server requires a password.
    pub password_protected: bool,
    /// Server region.
    pub region: String,
    /// Tick rate.
    pub tick_rate: u32,
    /// Timestamp when this beacon was last received.
    #[serde(skip)]
    pub last_seen: Option<Instant>,
}

/// Serialized beacon payload sent over UDP.
#[derive(Debug, Serialize, Deserialize)]
struct BeaconPayload {
    magic: [u8; 4],
    beacon: ServerBeacon,
}

impl ServerBeacon {
    /// Serializes the beacon into a UDP packet.
    pub fn to_bytes(&self) -> Vec<u8> {
        let payload = BeaconPayload {
            magic: DISCOVERY_MAGIC,
            beacon: self.clone(),
        };
        bincode::options()
            .with_limit(MAX_BEACON_SIZE)
            .serialize(&payload)
            .unwrap_or_default()
    }

    /// Deserializes a beacon from a UDP packet.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        // Reject packets larger than the beacon buffer — they cannot be valid
        // discovery beacons and would indicate a malformed or malicious packet.
        if data.len() > MAX_BEACON_SIZE {
            return None;
        }
        let payload: BeaconPayload = bincode::options()
            .with_limit(MAX_BEACON_SIZE)
            .deserialize(data)
            .ok()?;
        if payload.magic != DISCOVERY_MAGIC {
            return None;
        }
        let mut beacon = payload.beacon;
        beacon.last_seen = Some(Instant::now());
        Some(beacon)
    }
}

/// Broadcasts a server beacon on the LAN.
///
/// Sends a UDP broadcast packet to `DISCOVERY_PORT` containing server info.
/// Call this periodically (e.g. every 2 seconds) to announce the server.
pub fn broadcast_beacon(socket: &UdpSocket, beacon: &ServerBeacon) -> std::io::Result<usize> {
    let data = beacon.to_bytes();
    socket.set_broadcast(true)?;
    socket.send_to(&data, format!("255.255.255.255:{}", DISCOVERY_PORT))
}

/// Listens for incoming server beacons.
///
/// Non-blocking: returns immediately with `None` if no beacon is available.
/// Returns `(beacon, sender_addr)` — caller should validate beacon.address matches sender_addr.
pub fn listen_for_beacon(socket: &UdpSocket) -> Option<(ServerBeacon, std::net::SocketAddr)> {
    let mut buf = [0u8; MAX_BEACON_SIZE];
    // Non-blocking socket returns WouldBlock immediately — no need for read_timeout
    match socket.recv_from(&mut buf) {
        Ok((len, addr)) => {
            let mut beacon = ServerBeacon::from_bytes(&buf[..len])?;
            // Validate beacon address matches the actual sender (SD5: prevent address spoofing)
            let claimed_addr: std::net::SocketAddr = beacon.address.parse().ok()?;
            if claimed_addr.ip() != addr.ip() {
                return None;
            }
            beacon.last_seen = Some(Instant::now());
            Some((beacon, addr))
        }
        Err(_) => None,
    }
}

/// Creates a UDP socket bound to the discovery port for listening.
pub fn create_listener_socket() -> std::io::Result<UdpSocket> {
    let socket = UdpSocket::bind(format!("0.0.0.0:{}", DISCOVERY_PORT))?;
    socket.set_nonblocking(true)?;
    Ok(socket)
}

/// Creates a UDP socket for sending broadcasts (not bound to a specific port).
pub fn create_beacon_socket() -> std::io::Result<UdpSocket> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_broadcast(true)?;
    Ok(socket)
}

/// Loads additional servers from a `servers.json` file.
///
/// The file format is a JSON array of `ServerBeacon` objects.
pub fn load_server_list(path: &str) -> Vec<ServerBeacon> {
    match std::fs::read_to_string(path) {
        Ok(content) => serde_json::from_str::<Vec<ServerBeacon>>(&content).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// Saves the server list to a `servers.json` file.
pub fn save_server_list(path: &str, servers: &[ServerBeacon]) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(servers).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("JSON serialize error: {}", e),
        )
    })?;
    std::fs::write(path, json)
}
