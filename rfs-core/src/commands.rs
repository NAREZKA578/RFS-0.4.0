use std::collections::HashSet;
use std::net::IpAddr;
use std::path::Path;

/// Storage for banned IP addresses and client UUIDs.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct BanList {
    banned_ips: HashSet<IpAddr>,
    banned_uuids: HashSet<String>,
}

impl BanList {
    /// Returns `true` if the given IP is banned.
    pub fn is_banned(&self, ip: &IpAddr) -> bool {
        self.banned_ips.contains(ip)
    }

    /// Returns `true` if the given UUID is banned.
    pub fn is_banned_uuid(&self, uuid: &str) -> bool {
        self.banned_uuids.contains(uuid)
    }

    /// Adds an IP to the ban list.
    pub fn ban(&mut self, ip: IpAddr) {
        self.banned_ips.insert(ip);
    }

    /// Adds a UUID to the ban list.
    pub fn ban_uuid(&mut self, uuid: &str) {
        self.banned_uuids.insert(uuid.to_string());
    }

    /// Removes an IP from the ban list. Returns `true` if it was present.
    pub fn unban(&mut self, ip: &IpAddr) -> bool {
        self.banned_ips.remove(ip)
    }

    /// Removes a UUID from the ban list. Returns `true` if it was present.
    pub fn unban_uuid(&mut self, uuid: &str) -> bool {
        self.banned_uuids.remove(uuid)
    }

    /// Returns a list of all banned IPs.
    pub fn list(&self) -> Vec<IpAddr> {
        self.banned_ips.iter().copied().collect()
    }

    /// Returns a list of all banned UUIDs.
    pub fn list_uuids(&self) -> Vec<String> {
        self.banned_uuids.iter().cloned().collect()
    }

    /// Saves the ban list to a JSON file.
    pub fn save(&self, path: &Path) -> Result<(), std::io::Error> {
        let data = serde_json::json!({
            "ips": self.banned_ips.iter().map(|ip| ip.to_string()).collect::<Vec<_>>(),
            "uuids": self.banned_uuids.iter().cloned().collect::<Vec<_>>(),
        });
        let json = serde_json::to_string_pretty(&data).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, &json)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Loads the ban list from a JSON file. Creates an empty list if file doesn't exist.
    /// Supports both legacy (IP-only) and new (IP+UUID) formats.
    pub fn load(path: &Path) -> Self {
        if !path.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(path) {
            Ok(content) => {
                // Try new format first
                if let Ok(data) = serde_json::from_str::<serde_json::Value>(&content) {
                    let mut ban_list = Self::default();
                    if let Some(ips) = data.get("ips").and_then(|v| v.as_array()) {
                        for ip_str in ips {
                            if let Some(s) = ip_str.as_str() {
                                if let Ok(ip) = s.parse::<IpAddr>() {
                                    ban_list.ban(ip);
                                }
                            }
                        }
                    }
                    if let Some(uuids) = data.get("uuids").and_then(|v| v.as_array()) {
                        for uuid in uuids {
                            if let Some(s) = uuid.as_str() {
                                ban_list.ban_uuid(s);
                            }
                        }
                    }
                    if !ban_list.banned_ips.is_empty() || !ban_list.banned_uuids.is_empty() {
                        return ban_list;
                    }
                }
                // Fallback to legacy format (just IP strings)
                if let Ok(ips) = serde_json::from_str::<Vec<String>>(&content) {
                    let mut ban_list = Self::default();
                    for ip_str in ips {
                        if let Ok(ip) = ip_str.parse::<IpAddr>() {
                            ban_list.ban(ip);
                        }
                    }
                    ban_list
                } else {
                    Self::default()
                }
            }
            Err(_) => Self::default(),
        }
    }
}

/// All supported console commands.
#[derive(Debug, Clone, PartialEq)]
pub enum ConsoleCommand {
    /// Display server status.
    Status,
    /// List connected players.
    Players,
    /// Show help.
    Help,
    /// Kick a player by ID or name.
    Kick { target: String, reason: String },
    /// Ban a player by ID or name (IP ban).
    Ban { target: String, reason: String },
    /// Unban an IP address.
    Unban { ip: String },
    /// Broadcast a server-wide message.
    Say { message: String },
    /// Restart the current scenario.
    Restart,
    /// Trigger a new dispatch call.
    Dispatch { call_type: String, severity: u8 },
    /// Rescue a player instantly (admin).
    Rescue { target: String },
    /// Heal a player by ID or name.
    Heal { target: String, amount: f32 },
    /// Teleport a player to coordinates.
    Teleport {
        target: String,
        x: f32,
        y: f32,
        z: f32,
    },
    /// Change max players (affects new connections).
    SetMaxPlayers { count: usize },
    /// Set or clear the auth token.
    Password { token: Option<String> },
    /// Spawn a fire at coordinates.
    SpawnFire {
        x: f32,
        y: f32,
        z: f32,
        intensity: f32,
    },
    /// Extinguish all fires.
    ExtinguishAll,
    /// Spawn a civilian at coordinates.
    SpawnCivilian { x: f32, y: f32, z: f32 },
    /// Clear all civilians.
    ClearCivilians,
    /// Shutdown the server.
    Shutdown,
    /// Unknown command.
    Unknown(String),
}

/// Parses a raw console input line into a `ConsoleCommand`.
pub fn parse_command(input: &str) -> ConsoleCommand {
    let parts: Vec<&str> = input.trim().splitn(2, ' ').collect();
    let cmd = parts[0].to_lowercase();
    let args = parts.get(1).unwrap_or(&"").trim();

    match cmd.as_str() {
        "status" | "info" => ConsoleCommand::Status,
        "players" | "who" | "list" => ConsoleCommand::Players,
        "help" | "cmd" | "commands" => ConsoleCommand::Help,

        "kick" => {
            let (target, reason) = split_args(args);
            ConsoleCommand::Kick {
                target,
                reason: if reason.is_empty() {
                    "Kicked by admin".into()
                } else {
                    reason
                },
            }
        }
        "ban" => {
            let (target, reason) = split_args(args);
            ConsoleCommand::Ban {
                target,
                reason: if reason.is_empty() {
                    "Banned by admin".into()
                } else {
                    reason
                },
            }
        }
        "unban" => ConsoleCommand::Unban {
            ip: args.to_string(),
        },

        "say" => ConsoleCommand::Say {
            message: args.to_string(),
        },

        "restart" | "reset" | "restart_round" => ConsoleCommand::Restart,

        "dispatch" | "call" => {
            let parts: Vec<&str> = args.split_whitespace().collect();
            let call_type = parts.first().unwrap_or(&"residential").to_string();
            let severity = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
            ConsoleCommand::Dispatch {
                call_type,
                severity,
            }
        }
        "rescue" => ConsoleCommand::Rescue {
            target: args.to_string(),
        },
        "heal" => {
            let parts: Vec<&str> = args.split_whitespace().collect();
            let target = parts.first().unwrap_or(&"").to_string();
            let amount = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(100.0);
            ConsoleCommand::Heal { target, amount }
        }
        "teleport" | "tp" => {
            let nums: Vec<&str> = args.split_whitespace().collect();
            let target = nums.first().unwrap_or(&"").to_string();
            let x = nums.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let y = nums.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let z = nums.get(3).and_then(|s| s.parse().ok()).unwrap_or(0.0);
            ConsoleCommand::Teleport { target, x, y, z }
        }

        "set_maxplayers" | "maxplayers" | "max_players" => {
            let count = args.parse().unwrap_or(32);
            ConsoleCommand::SetMaxPlayers { count }
        }

        "password" | "pass" | "auth" => {
            let token = if args.is_empty() {
                None
            } else {
                Some(args.to_string())
            };
            ConsoleCommand::Password { token }
        }

        "fire" => {
            let nums: Vec<&str> = args.split_whitespace().collect();
            let x = nums.first().and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let y = nums.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let z = nums.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let intensity = nums.get(3).and_then(|s| s.parse().ok()).unwrap_or(50.0);
            ConsoleCommand::SpawnFire { x, y, z, intensity }
        }
        "extinguish" | "extinguish_all" => ConsoleCommand::ExtinguishAll,

        "spawn_civilian" | "civ" => {
            let nums: Vec<&str> = args.split_whitespace().collect();
            let x = nums.first().and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let y = nums.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let z = nums.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0);
            ConsoleCommand::SpawnCivilian { x, y, z }
        }
        "clear_civilians" => ConsoleCommand::ClearCivilians,

        "quit" | "shutdown" | "exit" => ConsoleCommand::Shutdown,

        other => ConsoleCommand::Unknown(other.to_string()),
    }
}

fn split_args(args: &str) -> (String, String) {
    let parts: Vec<&str> = args.splitn(2, ' ').collect();
    let target = parts.first().unwrap_or(&"").to_string();
    let rest = parts.get(1).unwrap_or(&"").trim().to_string();
    (target, rest)
}
