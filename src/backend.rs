//! NetworkManager backend built on `nmcli`.
//!
//! The backend is deliberately synchronous and lives on a worker thread. The UI
//! communicates with it through [`Request`] and [`Response`] messages so a slow
//! scan or connection attempt never blocks rendering or input handling.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::process::Command;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

const CONNECT_WAIT: &str = "30";
const ACTION_WAIT: &str = "10";
const SCAN_SETTLE: Duration = Duration::from_millis(900);

/// A user-facing backend failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendError(String);

impl BackendError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BackendError {}

pub type Result<T> = std::result::Result<T, BackendError>;

/// The security class that matters for connecting and QR sharing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    Open,
    Wep,
    Wpa,
    Enterprise,
    Unknown,
}

impl Security {
    pub fn from_scan_label(label: &str) -> Self {
        let upper = label.trim().to_ascii_uppercase();
        if upper.is_empty() || upper == "--" {
            Self::Open
        } else if upper.contains("802.1X") || upper.contains("EAP") {
            Self::Enterprise
        } else if upper.contains("WEP") {
            Self::Wep
        } else if upper.contains("WPA") || upper.contains("SAE") {
            Self::Wpa
        } else {
            Self::Unknown
        }
    }

    pub fn from_key_mgmt(key_mgmt: &str) -> Self {
        match key_mgmt.trim().to_ascii_lowercase().as_str() {
            "" | "none" => Self::Open,
            "wep" => Self::Wep,
            "wpa-psk" | "sae" => Self::Wpa,
            "802.1x" | "wpa-eap" | "wpa-eap-suite-b-192" => Self::Enterprise,
            _ => Self::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Wep => "WEP",
            Self::Wpa => "WPA",
            Self::Enterprise => "Enterprise",
            Self::Unknown => "Unknown",
        }
    }

    /// The `T:` value used by the Wi-Fi QR format.
    pub fn qr_type(self) -> Option<&'static str> {
        match self {
            Self::Open => Some("nopass"),
            Self::Wep => Some("WEP"),
            Self::Wpa => Some("WPA"),
            Self::Enterprise | Self::Unknown => None,
        }
    }

    pub fn needs_password(self) -> bool {
        matches!(self, Self::Wep | Self::Wpa)
    }
}

fn key_mgmt_label(key_mgmt: &str) -> &'static str {
    match key_mgmt.trim().to_ascii_lowercase().as_str() {
        "" | "none" => "Open",
        "wpa-psk" => "WPA/WPA2 personal",
        "sae" => "WPA3 personal",
        "wep" => "WEP",
        "802.1x" => "Enterprise (802.1X)",
        "wpa-eap" => "WPA Enterprise",
        "wpa-eap-suite-b-192" => "WPA3 Enterprise 192-bit",
        _ => "Unknown",
    }
}

/// The NetworkManager connection type, reduced to what the UI cares about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionKind {
    Wifi,
    Ethernet,
    Other(String),
}

impl ConnectionKind {
    pub fn from_nm_type(value: &str) -> Self {
        match value {
            "802-11-wireless" | "wifi" => Self::Wifi,
            "802-3-ethernet" | "ethernet" => Self::Ethernet,
            "loopback" => Self::Other("loopback".to_string()),
            other => Self::Other(other.to_string()),
        }
    }

    pub fn is_wifi(&self) -> bool {
        matches!(self, Self::Wifi)
    }

    pub fn display(&self) -> &str {
        match self {
            Self::Wifi => "Wi-Fi",
            Self::Ethernet => "Ethernet",
            Self::Other(value) => match value.as_str() {
                "vpn" => "VPN",
                "wireguard" => "WireGuard",
                "bridge" => "Bridge",
                "bond" => "Bond",
                "vlan" => "VLAN",
                "wifi-p2p" => "Wi-Fi P2P",
                other => other,
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct WifiNetwork {
    pub ssid: String,
    pub signal: u8,
    pub security: Security,
    pub security_label: String,
    pub freq_mhz: u32,
    pub rate: String,
    pub bssid: String,
    pub active: bool,
    pub saved_uuid: Option<String>,
}

impl WifiNetwork {
    pub fn saved(&self) -> bool {
        self.saved_uuid.is_some()
    }

    pub fn band(&self) -> &'static str {
        if self.freq_mhz >= 5925 {
            "6 GHz"
        } else if self.freq_mhz >= 4900 {
            "5 GHz"
        } else if self.freq_mhz > 0 {
            "2.4 GHz"
        } else {
            "unknown"
        }
    }

    pub fn band_short(&self) -> &'static str {
        if self.freq_mhz >= 5925 {
            "6G"
        } else if self.freq_mhz >= 4900 {
            "5G"
        } else if self.freq_mhz > 0 {
            "2.4G"
        } else {
            "?"
        }
    }

    pub fn security_short(&self) -> &'static str {
        match self.security {
            Security::Open => "Open",
            Security::Wep => "WEP",
            Security::Enterprise => "Enterprise",
            Security::Unknown => "Unknown",
            Security::Wpa => {
                let upper = self.security_label.to_ascii_uppercase();
                if upper.contains("WPA3") && upper.contains("WPA2") {
                    "WPA2/3"
                } else if upper.contains("WPA3") {
                    "WPA3"
                } else if upper.contains("WPA2") {
                    "WPA2"
                } else {
                    "WPA"
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct SavedConnection {
    pub uuid: String,
    pub name: String,
    pub kind: ConnectionKind,
    pub device: Option<String>,
    pub active: bool,
    pub autoconnect: bool,
    pub ssid: Option<String>,
    pub hidden: bool,
}

#[derive(Debug, Clone)]
pub struct ActiveLink {
    pub connection: String,
    pub device: String,
    pub kind: ConnectionKind,
    pub ip4: Vec<String>,
    pub gateway: Option<String>,
    pub dns: Vec<String>,
}

impl ActiveLink {
    pub fn primary_ip(&self) -> Option<&str> {
        self.ip4.first().map(String::as_str)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub wifi_enabled: bool,
    pub connectivity: String,
    pub hostname: String,
    pub wifi_device: Option<String>,
    pub networks: Vec<WifiNetwork>,
    pub connections: Vec<SavedConnection>,
    pub active: Option<ActiveLink>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DetailsData {
    pub name: String,
    pub uuid: String,
    pub kind: ConnectionKind,
    pub autoconnect: bool,
    pub ipv4_method: String,
    pub addresses: Vec<String>,
    pub gateway: Option<String>,
    pub dns: Vec<String>,
    pub ssid: Option<String>,
    pub security_label: Option<String>,
}

/// Everything needed to build a Wi-Fi QR payload.
#[derive(Clone)]
pub struct ShareInfo {
    pub ssid: String,
    pub security: Security,
    pub security_label: String,
    pub password: Option<String>,
    pub hidden: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotKind {
    Refresh,
    Scan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailsPurpose {
    View,
    Edit,
}

/// Work for the backend worker. Password-bearing variants intentionally do not
/// implement `Debug` so credentials are not accidentally logged.
pub enum Request {
    Refresh,
    Scan,
    Connect {
        ssid: String,
        target: String,
        password: Option<String>,
        hidden: bool,
    },
    Activate {
        uuid: String,
        name: String,
    },
    Disconnect {
        device: String,
        ssid: String,
    },
    Deactivate {
        uuid: String,
        name: String,
    },
    Delete {
        uuid: String,
        name: String,
    },
    SetWifi {
        enabled: bool,
    },
    SetAutoconnect {
        uuid: String,
        name: String,
        enabled: bool,
    },
    Edit {
        uuid: String,
        name: String,
        autoconnect: bool,
        ipv4_method: String,
        addresses: String,
        gateway: String,
        dns: String,
    },
    Share {
        uuid: String,
        name: String,
    },
    Details {
        uuid: String,
        kind: ConnectionKind,
        purpose: DetailsPurpose,
    },
}

pub enum Response {
    Snapshot {
        kind: SnapshotKind,
        result: Result<Snapshot>,
    },
    Action {
        result: Result<String>,
        snapshot: Option<Snapshot>,
    },
    Share {
        result: Result<ShareInfo>,
    },
    Details {
        purpose: DetailsPurpose,
        result: Result<DetailsData>,
    },
}

/// Spawn the backend worker and return its request sender and response receiver.
pub fn spawn(mut backend: Nmcli) -> (Sender<Request>, Receiver<Response>) {
    let (request_tx, request_rx) = mpsc::channel();
    let (response_tx, response_rx) = mpsc::channel();

    thread::spawn(move || {
        while let Ok(request) = request_rx.recv() {
            let response = handle_request(&mut backend, request);
            if response_tx.send(response).is_err() {
                break;
            }
        }
    });

    (request_tx, response_rx)
}

fn handle_request(backend: &mut Nmcli, request: Request) -> Response {
    match request {
        Request::Refresh => Response::Snapshot {
            kind: SnapshotKind::Refresh,
            result: backend.snapshot(),
        },
        Request::Scan => Response::Snapshot {
            kind: SnapshotKind::Scan,
            result: backend.scan(),
        },
        Request::Connect {
            ssid,
            target,
            password,
            hidden,
        } => finish_action(
            backend,
            backend
                .connect(&target, password.as_deref(), hidden)
                .map(|()| format!("Connected to \"{ssid}\"")),
        ),
        Request::Activate { uuid, name } => finish_action(
            backend,
            backend
                .activate(&uuid)
                .map(|()| format!("Activated \"{name}\"")),
        ),
        Request::Disconnect { device, ssid } => finish_action(
            backend,
            backend
                .disconnect(&device)
                .map(|()| format!("Disconnected from \"{ssid}\"")),
        ),
        Request::Deactivate { uuid, name } => finish_action(
            backend,
            backend
                .deactivate(&uuid)
                .map(|()| format!("Deactivated \"{name}\"")),
        ),
        Request::Delete { uuid, name } => finish_action(
            backend,
            backend
                .delete(&uuid)
                .map(|()| format!("Deleted \"{name}\"")),
        ),
        Request::SetWifi { enabled } => finish_action(
            backend,
            backend.set_wifi(enabled).map(|()| {
                if enabled {
                    "Wi-Fi enabled".to_string()
                } else {
                    "Wi-Fi disabled".to_string()
                }
            }),
        ),
        Request::SetAutoconnect {
            uuid,
            name,
            enabled,
        } => finish_action(
            backend,
            backend.set_autoconnect(&uuid, enabled).map(|()| {
                format!(
                    "Autoconnect {} for \"{name}\"",
                    if enabled { "enabled" } else { "disabled" }
                )
            }),
        ),
        Request::Edit {
            uuid,
            name,
            autoconnect,
            ipv4_method,
            addresses,
            gateway,
            dns,
        } => finish_action(
            backend,
            backend
                .edit(
                    &uuid,
                    &name,
                    autoconnect,
                    &ipv4_method,
                    &addresses,
                    &gateway,
                    &dns,
                )
                .map(|()| format!("Saved settings for \"{name}\"")),
        ),
        Request::Share { uuid, name } => Response::Share {
            result: backend.share_profile(&uuid, &name),
        },
        Request::Details {
            uuid,
            kind,
            purpose,
        } => Response::Details {
            purpose,
            result: backend.details(&uuid, &kind),
        },
    }
}

fn finish_action(backend: &mut Nmcli, result: Result<String>) -> Response {
    let snapshot = backend.snapshot().ok();
    Response::Action { result, snapshot }
}

#[derive(Debug, Clone)]
struct DeviceStatus {
    name: String,
    kind: ConnectionKind,
    state: String,
    connection: String,
}

pub struct Nmcli {
    program: String,
    ssid_cache: HashMap<String, Option<String>>,
    hidden_cache: HashMap<String, bool>,
}

impl Nmcli {
    pub fn new() -> Self {
        let program = std::env::var("BNM_NMCLI").unwrap_or_else(|_| "nmcli".to_string());
        Self {
            program,
            ssid_cache: HashMap::new(),
            hidden_cache: HashMap::new(),
        }
    }

    #[cfg(test)]
    fn with_program(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            ssid_cache: HashMap::new(),
            hidden_cache: HashMap::new(),
        }
    }

    fn run(&self, args: &[&str]) -> Result<String> {
        let output = Command::new(&self.program)
            .args(args)
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .output()
            .map_err(|error| {
                BackendError::new(format!(
                    "could not run {}: {error}. Is NetworkManager installed?",
                    self.program
                ))
            })?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(BackendError::new(clean_error(stderr.trim())))
        }
    }

    pub fn snapshot(&mut self) -> Result<Snapshot> {
        let general = self.run(&["-t", "-f", "WIFI,CONNECTIVITY", "general"])?;
        let general_fields = split_terse(trim_line_end(&general));
        if general_fields.len() < 2 {
            return Err(BackendError::new(
                "NetworkManager returned an unexpected general status",
            ));
        }

        let wifi_enabled = general_fields[0] == "enabled";
        let connectivity = general_fields[1].clone();
        let hostname = trim_line_end(&self.run(&["general", "hostname"])?).to_string();

        let device_output = self.run(&[
            "-t",
            "-f",
            "DEVICE,TYPE,STATE,CONNECTION",
            "device",
            "status",
        ])?;
        let devices = parse_devices(&device_output);
        let wifi_device = devices
            .iter()
            .find(|device| device.kind.is_wifi() && device.state != "unmanaged")
            .map(|device| device.name.clone());

        let mut warnings = Vec::new();
        let mut connections = match self.connections() {
            Ok(connections) => connections,
            Err(error) => {
                warnings.push(format!("Could not list saved connections: {error}"));
                Vec::new()
            }
        };
        self.cache_ssids(&mut connections);

        let networks = if wifi_enabled {
            match self.wifi_networks(&connections) {
                Ok(networks) => networks,
                Err(error) => {
                    warnings.push(format!("Could not list Wi-Fi networks: {error}"));
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };

        let active = self.active_link(&devices, &mut warnings);

        Ok(Snapshot {
            wifi_enabled,
            connectivity,
            hostname,
            wifi_device,
            networks,
            connections,
            active,
            warnings,
        })
    }

    pub fn scan(&mut self) -> Result<Snapshot> {
        let rescan = self.run(&["device", "wifi", "rescan"]);
        let mut throttled = false;
        match rescan {
            Ok(_) => thread::sleep(SCAN_SETTLE),
            Err(error) if is_scan_throttle(error.message()) => throttled = true,
            Err(error) => {
                let mut snapshot = self.snapshot()?;
                snapshot
                    .warnings
                    .push(format!("Wi-Fi scan failed: {error}"));
                return Ok(snapshot);
            }
        }
        let mut snapshot = self.snapshot()?;
        if throttled {
            snapshot.warnings.push(
                "Wi-Fi scan was throttled by NetworkManager; showing cached results".to_string(),
            );
        }
        Ok(snapshot)
    }

    fn connections(&self) -> Result<Vec<SavedConnection>> {
        let output = self.run(&[
            "-t",
            "-f",
            "UUID,NAME,TYPE,DEVICE,ACTIVE,AUTOCONNECT",
            "connection",
            "show",
        ])?;

        let mut connections = Vec::new();
        for fields in parse_records(&output, is_uuid_prefix) {
            if fields.len() < 6 {
                continue;
            }
            let kind = ConnectionKind::from_nm_type(&fields[2]);
            if matches!(&kind, ConnectionKind::Other(value) if value == "loopback") {
                continue;
            }
            connections.push(SavedConnection {
                uuid: fields[0].clone(),
                name: fields[1].clone(),
                kind,
                device: non_empty(&fields[3]),
                active: fields[4] == "yes",
                autoconnect: fields[5] == "yes",
                ssid: None,
                hidden: false,
            });
        }

        connections.sort_by(|left, right| {
            right
                .active
                .cmp(&left.active)
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        });
        Ok(connections)
    }

    fn cache_ssids(&mut self, connections: &mut [SavedConnection]) {
        let current: HashSet<&str> = connections
            .iter()
            .filter(|connection| connection.kind.is_wifi())
            .map(|connection| connection.uuid.as_str())
            .collect();
        self.ssid_cache
            .retain(|uuid, _| current.contains(uuid.as_str()));
        self.hidden_cache
            .retain(|uuid, _| current.contains(uuid.as_str()));

        for connection in connections.iter_mut().filter(|c| c.kind.is_wifi()) {
            if !self.ssid_cache.contains_key(&connection.uuid) {
                let args = [
                    "-g",
                    "802-11-wireless.ssid",
                    "connection",
                    "show",
                    "uuid",
                    connection.uuid.as_str(),
                ];
                if let Ok(output) = self.run(&args) {
                    let ssid = trim_line_end(&output).to_string();
                    self.ssid_cache.insert(
                        connection.uuid.clone(),
                        if ssid.is_empty() { None } else { Some(ssid) },
                    );
                }
            }
            if !self.hidden_cache.contains_key(&connection.uuid) {
                let args = [
                    "-g",
                    "802-11-wireless.hidden",
                    "connection",
                    "show",
                    "uuid",
                    connection.uuid.as_str(),
                ];
                if let Ok(output) = self.run(&args) {
                    self.hidden_cache
                        .insert(connection.uuid.clone(), trim_line_end(&output) == "yes");
                }
            }
            connection.ssid = self.ssid_cache.get(&connection.uuid).cloned().flatten();
            connection.hidden = self
                .hidden_cache
                .get(&connection.uuid)
                .copied()
                .unwrap_or(false);
        }
    }

    fn wifi_networks(&self, connections: &[SavedConnection]) -> Result<Vec<WifiNetwork>> {
        let output = self.run(&[
            "-t",
            "-f",
            "IN-USE,SSID,SIGNAL,SECURITY,FREQ,RATE,BSSID",
            "device",
            "wifi",
            "list",
        ])?;

        let saved_by_ssid: HashMap<&str, &str> = connections
            .iter()
            .filter(|connection| connection.kind.is_wifi())
            .filter_map(|connection| {
                connection
                    .ssid
                    .as_deref()
                    .map(|ssid| (ssid, connection.uuid.as_str()))
            })
            .collect();

        let mut by_ssid: HashMap<String, WifiNetwork> = HashMap::new();
        for fields in parse_wifi_records(&output) {
            if fields.len() < 7 {
                continue;
            }
            let active = fields[0].trim() == "*";
            let ssid = fields[1].clone();
            if ssid.is_empty() {
                continue;
            }
            let signal = fields[2].parse::<u8>().unwrap_or(0).min(100);
            let security_label = fields[3].trim().to_string();
            let security = Security::from_scan_label(&security_label);
            let freq_mhz = parse_freq(&fields[4]);
            let rate = fields[5].trim().to_string();
            let bssid = fields[6].trim().to_string();

            match by_ssid.get_mut(&ssid) {
                Some(existing) => {
                    let prefer_new = if active && !existing.active {
                        true
                    } else if existing.active && !active {
                        false
                    } else {
                        signal > existing.signal
                    };
                    existing.active |= active;
                    if prefer_new {
                        existing.signal = signal;
                        existing.security = security;
                        existing.security_label = security_label;
                        existing.freq_mhz = freq_mhz;
                        existing.rate = rate;
                        existing.bssid = bssid;
                    }
                }
                None => {
                    by_ssid.insert(
                        ssid.clone(),
                        WifiNetwork {
                            ssid,
                            signal,
                            security,
                            security_label,
                            freq_mhz,
                            rate,
                            bssid,
                            active,
                            saved_uuid: None,
                        },
                    );
                }
            }
        }

        let mut networks: Vec<WifiNetwork> = by_ssid
            .into_values()
            .map(|mut network| {
                network.saved_uuid = saved_by_ssid
                    .get(network.ssid.as_str())
                    .map(|uuid| (*uuid).to_string());
                network
            })
            .collect();

        networks.sort_by(|left, right| {
            right
                .active
                .cmp(&left.active)
                .then_with(|| right.saved().cmp(&left.saved()))
                .then_with(|| right.signal.cmp(&left.signal))
                .then_with(|| left.ssid.to_lowercase().cmp(&right.ssid.to_lowercase()))
        });
        Ok(networks)
    }

    fn active_link(
        &self,
        devices: &[DeviceStatus],
        warnings: &mut Vec<String>,
    ) -> Option<ActiveLink> {
        let device = devices
            .iter()
            .find(|device| device.kind.is_wifi() && device.state.starts_with("connected"))
            .or_else(|| {
                devices.iter().find(|device| {
                    matches!(device.kind, ConnectionKind::Ethernet)
                        && device.state.starts_with("connected")
                })
            })?;

        let mut link = ActiveLink {
            connection: device.connection.clone(),
            device: device.name.clone(),
            kind: device.kind.clone(),
            ip4: Vec::new(),
            gateway: None,
            dns: Vec::new(),
        };

        let args = [
            "-t",
            "-f",
            "GENERAL.CONNECTION,IP4.ADDRESS,IP4.GATEWAY,IP4.DNS",
            "device",
            "show",
            device.name.as_str(),
        ];
        match self.run(&args) {
            Ok(output) => {
                for (key, value) in parse_key_values(&output) {
                    if value.is_empty() {
                        continue;
                    }
                    match base_key(&key) {
                        "GENERAL.CONNECTION" => link.connection = value,
                        "IP4.ADDRESS" => link.ip4.push(value),
                        "IP4.GATEWAY" => link.gateway = Some(value),
                        "IP4.DNS" => link.dns.push(value),
                        _ => {}
                    }
                }
            }
            Err(error) => {
                warnings.push(format!("Could not read active connection details: {error}"))
            }
        }

        if link.connection.is_empty() {
            link.connection = device.connection.clone();
        }
        Some(link)
    }

    fn connect(&self, target: &str, password: Option<&str>, hidden: bool) -> Result<()> {
        let args = connect_args(target, password, hidden);
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        self.run(&arg_refs)?;
        Ok(())
    }

    fn activate(&self, uuid: &str) -> Result<()> {
        self.run(&["--wait", CONNECT_WAIT, "connection", "up", "uuid", uuid])?;
        Ok(())
    }

    fn disconnect(&self, device: &str) -> Result<()> {
        self.run(&["--wait", ACTION_WAIT, "device", "disconnect", device])?;
        Ok(())
    }

    fn deactivate(&self, uuid: &str) -> Result<()> {
        self.run(&["--wait", ACTION_WAIT, "connection", "down", "uuid", uuid])?;
        Ok(())
    }

    fn delete(&self, uuid: &str) -> Result<()> {
        self.run(&["connection", "delete", "uuid", uuid])?;
        Ok(())
    }

    fn set_wifi(&self, enabled: bool) -> Result<()> {
        self.run(&["radio", "wifi", if enabled { "on" } else { "off" }])?;
        Ok(())
    }

    fn set_autoconnect(&self, uuid: &str, enabled: bool) -> Result<()> {
        self.run(&[
            "connection",
            "modify",
            "uuid",
            uuid,
            "connection.autoconnect",
            if enabled { "yes" } else { "no" },
        ])?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn edit(
        &self,
        uuid: &str,
        name: &str,
        autoconnect: bool,
        ipv4_method: &str,
        addresses: &str,
        gateway: &str,
        dns: &str,
    ) -> Result<()> {
        let args = edit_args(
            uuid,
            name,
            autoconnect,
            ipv4_method,
            addresses,
            gateway,
            dns,
        );
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        self.run(&refs)?;
        Ok(())
    }

    fn details(&self, uuid: &str, kind: &ConnectionKind) -> Result<DetailsData> {
        let mut fields = String::from(
            "connection.id,connection.uuid,connection.type,connection.autoconnect,\
             ipv4.method,ipv4.addresses,ipv4.gateway,ipv4.dns",
        );
        if kind.is_wifi() {
            fields.push_str(",802-11-wireless.ssid,802-11-wireless-security.key-mgmt");
        }
        let output = self.run(&[
            "-t",
            "-f",
            fields.as_str(),
            "connection",
            "show",
            "uuid",
            uuid,
        ])?;
        let values: HashMap<String, String> = parse_key_values(&output).into_iter().collect();

        let name = values
            .get("connection.id")
            .filter(|value| !value.is_empty())
            .cloned()
            .unwrap_or_else(|| uuid.to_string());
        let kind = values
            .get("connection.type")
            .map(|value| ConnectionKind::from_nm_type(value))
            .unwrap_or_else(|| kind.clone());
        let key_mgmt = values
            .get("802-11-wireless-security.key-mgmt")
            .cloned()
            .unwrap_or_default();
        let security_label = if kind.is_wifi() {
            Some(key_mgmt_label(&key_mgmt).to_string())
        } else {
            None
        };

        Ok(DetailsData {
            name,
            uuid: values
                .get("connection.uuid")
                .cloned()
                .unwrap_or_else(|| uuid.to_string()),
            kind,
            autoconnect: values
                .get("connection.autoconnect")
                .map(|value| value == "yes")
                .unwrap_or(false),
            ipv4_method: values
                .get("ipv4.method")
                .cloned()
                .unwrap_or_else(|| "auto".to_string()),
            addresses: split_list(values.get("ipv4.addresses").map(String::as_str)),
            gateway: values
                .get("ipv4.gateway")
                .filter(|value| !value.is_empty())
                .cloned(),
            dns: split_list(values.get("ipv4.dns").map(String::as_str)),
            ssid: values
                .get("802-11-wireless.ssid")
                .filter(|value| !value.is_empty())
                .cloned(),
            security_label,
        })
    }

    fn share_profile(&self, uuid: &str, name: &str) -> Result<ShareInfo> {
        let output = self.run(&[
            "-t",
            "-f",
            "802-11-wireless.ssid,802-11-wireless-security.key-mgmt,802-11-wireless.hidden",
            "connection",
            "show",
            "uuid",
            uuid,
        ])?;
        let values: HashMap<String, String> = parse_key_values(&output).into_iter().collect();
        let ssid = values
            .get("802-11-wireless.ssid")
            .filter(|value| !value.is_empty())
            .cloned()
            .unwrap_or_else(|| name.to_string());
        let key_mgmt = values
            .get("802-11-wireless-security.key-mgmt")
            .map(String::as_str)
            .unwrap_or_default();
        let security = Security::from_key_mgmt(key_mgmt);
        let hidden = values
            .get("802-11-wireless.hidden")
            .map(|value| value == "yes")
            .unwrap_or(false);
        let password = match security {
            Security::Wpa => self.secret(uuid, "802-11-wireless-security.psk")?,
            Security::Wep => self.secret(uuid, "802-11-wireless-security.wep-key0")?,
            Security::Open | Security::Enterprise | Security::Unknown => None,
        };

        Ok(ShareInfo {
            ssid,
            security,
            security_label: key_mgmt_label(key_mgmt).to_string(),
            password,
            hidden,
        })
    }

    fn secret(&self, uuid: &str, field: &str) -> Result<Option<String>> {
        let output = self
            .run(&["-s", "-g", field, "connection", "show", "uuid", uuid])
            .map_err(|error| {
                BackendError::new(format!(
                    "NetworkManager would not reveal the saved password: {error}"
                ))
            })?;
        let value = trim_line_end(&output).to_string();
        Ok(if value.is_empty() { None } else { Some(value) })
    }
}

fn connect_args(target: &str, password: Option<&str>, hidden: bool) -> Vec<String> {
    let mut args = vec![
        "--wait".to_string(),
        CONNECT_WAIT.to_string(),
        "device".to_string(),
        "wifi".to_string(),
        "connect".to_string(),
        target.to_string(),
    ];
    if let Some(password) = password {
        args.push("password".to_string());
        args.push(password.to_string());
    }
    if hidden {
        args.push("hidden".to_string());
        args.push("yes".to_string());
    }
    args
}

fn edit_args(
    uuid: &str,
    name: &str,
    autoconnect: bool,
    ipv4_method: &str,
    addresses: &str,
    gateway: &str,
    dns: &str,
) -> Vec<String> {
    let mut args = vec![
        "connection".to_string(),
        "modify".to_string(),
        "uuid".to_string(),
        uuid.to_string(),
        "connection.id".to_string(),
        name.to_string(),
        "connection.autoconnect".to_string(),
        if autoconnect { "yes" } else { "no" }.to_string(),
        "ipv4.method".to_string(),
        ipv4_method.to_string(),
    ];
    args.push("ipv4.addresses".to_string());
    args.push(if ipv4_method == "manual" {
        addresses.to_string()
    } else {
        String::new()
    });
    args.push("ipv4.gateway".to_string());
    args.push(if ipv4_method == "manual" {
        gateway.to_string()
    } else {
        String::new()
    });
    args.push("ipv4.dns".to_string());
    args.push(if ipv4_method == "manual" {
        dns.to_string()
    } else {
        String::new()
    });
    args
}

fn parse_devices(output: &str) -> Vec<DeviceStatus> {
    output
        .lines()
        .filter_map(|line| {
            let fields = split_terse(line);
            (fields.len() >= 4).then(|| DeviceStatus {
                name: fields[0].clone(),
                kind: ConnectionKind::from_nm_type(&fields[1]),
                state: fields[2].clone(),
                connection: fields[3].clone(),
            })
        })
        .collect()
}

fn parse_freq(value: &str) -> u32 {
    value
        .split_whitespace()
        .next()
        .and_then(|number| number.parse().ok())
        .unwrap_or(0)
}

fn parse_records(output: &str, is_start: fn(&str) -> bool) -> Vec<Vec<String>> {
    let mut records = Vec::new();
    let mut current = String::new();

    for line in output.lines() {
        if is_start(line) {
            if !current.is_empty() {
                records.push(split_terse(&current));
            }
            current.clear();
            current.push_str(line);
        } else if !current.is_empty() {
            current.push('\n');
            current.push_str(line);
        }
    }
    if !current.is_empty() {
        records.push(split_terse(&current));
    }
    records
}

fn parse_wifi_records(output: &str) -> Vec<Vec<String>> {
    parse_records(output, |line| {
        line.starts_with("*:") || line.starts_with(" :")
    })
}

fn is_uuid_prefix(line: &str) -> bool {
    let bytes = line.as_bytes();
    if bytes.len() < 37 || bytes[36] != b':' {
        return false;
    }
    bytes[..36].iter().enumerate().all(|(index, byte)| {
        if matches!(index, 8 | 13 | 18 | 23) {
            *byte == b'-'
        } else {
            byte.is_ascii_hexdigit()
        }
    })
}

fn parse_key_values(output: &str) -> Vec<(String, String)> {
    let mut values: Vec<(String, String)> = Vec::new();
    for line in output.lines() {
        if let Some((key, value)) = split_key_value(line) {
            if !key.is_empty() {
                values.push((key.to_string(), value.to_string()));
            }
        } else if let Some((_, value)) = values.last_mut() {
            // Terse output does not escape newlines, so a line without an
            // unescaped colon is a continuation of the previous value.
            value.push('\n');
            value.push_str(line);
        }
    }
    values
        .into_iter()
        .map(|(key, value)| (unescape_terse(&key), unescape_terse(&value)))
        .collect()
}

/// Split a terse `key:value` line at the first unescaped colon.
fn split_key_value(line: &str) -> Option<(&str, &str)> {
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match character {
            '\\' => escaped = true,
            ':' => return Some((&line[..index], &line[index + 1..])),
            _ => {}
        }
    }
    None
}

fn base_key(key: &str) -> &str {
    key.split('[').next().unwrap_or(key)
}

fn split_list(value: Option<&str>) -> Vec<String> {
    value
        .map(|value| {
            value
                .split(|character: char| character == ',' || character.is_whitespace())
                .filter(|part| !part.is_empty())
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn non_empty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_string())
}

fn split_terse(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut escaped = false;

    for character in line.chars() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        match character {
            '\\' => escaped = true,
            ':' => {
                fields.push(std::mem::take(&mut current));
            }
            _ => current.push(character),
        }
    }
    if escaped {
        current.push('\\');
    }
    fields.push(current);
    fields
}

fn unescape_terse(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut escaped = false;
    for character in value.chars() {
        if escaped {
            output.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else {
            output.push(character);
        }
    }
    if escaped {
        output.push('\\');
    }
    output
}

fn trim_line_end(value: &str) -> &str {
    value.trim_end_matches(['\r', '\n'])
}

fn is_scan_throttle(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("scanning not allowed") || lower.contains("too frequent")
}

fn clean_error(stderr: &str) -> String {
    let message = stderr
        .trim()
        .strip_prefix("Error: ")
        .unwrap_or(stderr.trim())
        .to_string();
    if message.is_empty() {
        return "NetworkManager command failed".to_string();
    }

    let lower = message.to_ascii_lowercase();
    let friendly = if lower.contains("not authorized") || lower.contains("permission denied") {
        "Permission denied by NetworkManager or PolicyKit".to_string()
    } else if lower.contains("secrets were required") || lower.contains("no secrets") {
        "A password is required to activate this connection".to_string()
    } else if lower.contains("no network with ssid") {
        "Network not found; try rescanning".to_string()
    } else if lower.contains("already active") {
        "The connection is already active".to_string()
    } else if lower.contains("not running") {
        "NetworkManager is not running".to_string()
    } else {
        message
    };

    truncate(&friendly, 300)
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        value.to_string()
    } else {
        let mut truncated: String = value.chars().take(max_chars).collect();
        truncated.push('…');
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn terse_parser_handles_escaped_colons_and_backslashes() {
        assert_eq!(split_terse(r"a:b\:c:d\\e:"), vec!["a", "b:c", "d\\e", ""]);
        assert_eq!(split_terse(r"ssid:with\:colon"), vec!["ssid", "with:colon"]);
        assert_eq!(split_terse(r"trailing\\"), vec!["trailing\\"]);
    }

    #[test]
    fn connection_records_survive_newlines_in_names() {
        let output = concat!(
            "11111111-1111-1111-1111-111111111111:first:802-11-wireless::no:yes\n",
            "22222222-2222-2222-2222-222222222222:multi\n",
            "line\n",
            "name:802-11-wireless::no:no\n"
        );
        let records = parse_records(output, is_uuid_prefix);
        assert_eq!(records.len(), 2);
        assert_eq!(records[1][1], "multi\nline\nname");
        assert_eq!(records[1][2], "802-11-wireless");
    }

    #[test]
    fn wifi_records_survive_newlines_in_ssids() {
        let output = concat!(
            " :Cafe:80:WPA2:2437 MHz:54 Mbit/s:AA\\:BB\\:CC\\:DD\\:EE\\:01\n",
            " :Multi\n",
            "line:20::2412 MHz:1 Mbit/s:AA\\:BB\\:CC\\:DD\\:EE\\:02\n"
        );
        let records = parse_wifi_records(output);
        assert_eq!(records.len(), 2);
        assert_eq!(records[1][1], "Multi\nline");
    }

    #[test]
    fn key_value_parser_unescapes_values() {
        let values = parse_key_values("IP4.DNS[1]:1.1.1.1\nGENERAL.CONNECTION:My\\:WiFi\n");
        assert_eq!(values[0], ("IP4.DNS[1]".to_string(), "1.1.1.1".to_string()));
        assert_eq!(
            values[1],
            ("GENERAL.CONNECTION".to_string(), "My:WiFi".to_string())
        );
        assert_eq!(base_key("IP4.DNS[2]"), "IP4.DNS");
    }

    #[test]
    fn key_value_parser_keeps_newline_values() {
        let values = parse_key_values(
            "connection.id:multi\nline\nconnection.uuid:abc\n802-11-wireless.ssid:Cafe\nGuest\n802-11-wireless-security.key-mgmt:wpa-psk\n",
        );
        assert_eq!(
            values[0],
            ("connection.id".to_string(), "multi\nline".to_string())
        );
        assert_eq!(
            values[1],
            ("connection.uuid".to_string(), "abc".to_string())
        );
        assert_eq!(
            values[2],
            (
                "802-11-wireless.ssid".to_string(),
                "Cafe\nGuest".to_string()
            )
        );
        assert_eq!(values[3].1, "wpa-psk");
    }

    #[test]
    fn key_value_parser_handles_escaped_colons_in_continuations() {
        let values = parse_key_values("connection.id:multi\nline\\:two\n");
        assert_eq!(
            values[0],
            ("connection.id".to_string(), "multi\nline:two".to_string())
        );
    }

    #[test]
    fn security_classification_is_conservative() {
        assert_eq!(Security::from_scan_label(""), Security::Open);
        assert_eq!(Security::from_scan_label("--"), Security::Open);
        assert_eq!(Security::from_scan_label("WPA1 WPA2"), Security::Wpa);
        assert_eq!(Security::from_scan_label("WPA2 WPA3"), Security::Wpa);
        assert_eq!(
            Security::from_scan_label("WPA1 WPA2 802.1X"),
            Security::Enterprise
        );
        assert_eq!(Security::from_scan_label("WEP"), Security::Wep);
        assert_eq!(Security::from_key_mgmt("wpa-psk"), Security::Wpa);
        assert_eq!(Security::from_key_mgmt("sae"), Security::Wpa);
        assert_eq!(Security::from_key_mgmt("none"), Security::Open);
        assert_eq!(Security::from_key_mgmt("802.1x"), Security::Enterprise);
        assert_eq!(key_mgmt_label("wpa-psk"), "WPA/WPA2 personal");
        assert_eq!(key_mgmt_label("sae"), "WPA3 personal");
        assert_eq!(key_mgmt_label("none"), "Open");
    }

    #[test]
    fn connect_args_include_password_and_hidden_flags() {
        let args = connect_args("AA:BB:CC:DD:EE:FF", Some("secret"), true);
        assert_eq!(
            args,
            vec![
                "--wait",
                "30",
                "device",
                "wifi",
                "connect",
                "AA:BB:CC:DD:EE:FF",
                "password",
                "secret",
                "hidden",
                "yes"
            ]
        );
        let open = connect_args("Cafe", None, false);
        assert!(!open.iter().any(|arg| arg == "password"));
        assert!(!open.iter().any(|arg| arg == "hidden"));
    }

    #[test]
    fn edit_args_clear_static_values_for_non_manual_methods() {
        let args = edit_args(
            "abc",
            "Home",
            true,
            "auto",
            "10.0.0.2/24",
            "10.0.0.1",
            "1.1.1.1",
        );
        let tail: Vec<&str> = args[args.len() - 6..].iter().map(String::as_str).collect();
        assert_eq!(
            tail,
            vec!["ipv4.addresses", "", "ipv4.gateway", "", "ipv4.dns", ""]
        );
    }

    #[test]
    fn edit_args_keep_manual_values_and_clear_empty_ones() {
        let args = edit_args("abc", "Home", false, "manual", "10.0.0.2/24", "", "1.1.1.1");
        let tail: Vec<&str> = args[args.len() - 6..].iter().map(String::as_str).collect();
        assert_eq!(
            tail,
            vec![
                "ipv4.addresses",
                "10.0.0.2/24",
                "ipv4.gateway",
                "",
                "ipv4.dns",
                "1.1.1.1"
            ]
        );
    }

    #[test]
    fn error_messages_are_actionable() {
        assert_eq!(
            clean_error("Error: not authorized to control networking."),
            "Permission denied by NetworkManager or PolicyKit"
        );
        assert_eq!(
            clean_error("Error: No network with SSID 'Cafe' found."),
            "Network not found; try rescanning"
        );
        assert_eq!(
            clean_error("Error: Scanning not allowed immediately following previous scan."),
            "Scanning not allowed immediately following previous scan."
        );
        assert!(is_scan_throttle(
            "Scanning not allowed immediately following previous scan."
        ));
        assert_eq!(
            clean_error("Error: device 'wlan9' not found"),
            "device 'wlan9' not found"
        );
    }

    fn fake_nmcli(script: &str) -> (PathBuf, Nmcli) {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("bnm-fake-nmcli-{}-{unique}.sh", std::process::id()));
        fs::write(&path, script).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
        let program = path.to_string_lossy().to_string();
        (path, Nmcli::with_program(program))
    }

    #[test]
    fn snapshot_reads_networkmanager_state() {
        let script = r#"#!/bin/sh
case "$*" in
  "-t -f WIFI,CONNECTIVITY general")
    printf 'enabled:full\n' ;;
  "general hostname")
    printf 'testhost\n' ;;
  "-t -f DEVICE,TYPE,STATE,CONNECTION device status")
    printf 'wlan0:wifi:connected:Home\neth0:802-3-ethernet:disconnected:\nlo:loopback:connected (externally):lo\n' ;;
  "-t -f UUID,NAME,TYPE,DEVICE,ACTIVE,AUTOCONNECT connection show")
    printf '11111111-1111-1111-1111-111111111111:Home:802-11-wireless:wlan0:yes:yes\n22222222-2222-2222-2222-222222222222:Cafe:802-11-wireless::no:yes\n' ;;
  "-g 802-11-wireless.ssid connection show uuid 11111111-1111-1111-1111-111111111111")
    printf 'Home\n' ;;
  "-g 802-11-wireless.hidden connection show uuid 11111111-1111-1111-1111-111111111111")
    printf 'no\n' ;;
  "-g 802-11-wireless.ssid connection show uuid 22222222-2222-2222-2222-222222222222")
    printf 'Cafe\n' ;;
  "-g 802-11-wireless.hidden connection show uuid 22222222-2222-2222-2222-222222222222")
    printf 'yes\n' ;;
  "-t -f IN-USE,SSID,SIGNAL,SECURITY,FREQ,RATE,BSSID device wifi list")
    printf '*:Home:91:WPA2 WPA3:2437 MHz:1170 Mbit/s:AA\\:BB\\:CC\\:DD\\:EE\\:01\n :Cafe:45:WPA2:2412 MHz:54 Mbit/s:AA\\:BB\\:CC\\:DD\\:EE\\:02\n :Home:60:WPA2 WPA3:5180 MHz:540 Mbit/s:AA\\:BB\\:CC\\:DD\\:EE\\:03\n' ;;
  "-t -f GENERAL.CONNECTION,IP4.ADDRESS,IP4.GATEWAY,IP4.DNS device show wlan0")
    printf 'GENERAL.CONNECTION:Home\nIP4.ADDRESS[1]:192.168.1.42/24\nIP4.GATEWAY:192.168.1.1\nIP4.DNS[1]:1.1.1.1\nIP4.DNS[2]:8.8.8.8\n' ;;
  *)
    echo "unexpected args: $*" >&2
    exit 1 ;;
esac
"#;
        let (path, mut backend) = fake_nmcli(script);
        let snapshot = backend.snapshot().unwrap();
        fs::remove_file(path).ok();

        assert!(snapshot.wifi_enabled);
        assert_eq!(snapshot.hostname, "testhost");
        assert_eq!(snapshot.connectivity, "full");
        assert_eq!(snapshot.wifi_device.as_deref(), Some("wlan0"));
        assert_eq!(snapshot.networks.len(), 2);
        assert_eq!(snapshot.networks[0].ssid, "Home");
        assert!(snapshot.networks[0].active);
        assert_eq!(
            snapshot.networks[0].saved_uuid.as_deref(),
            Some("11111111-1111-1111-1111-111111111111")
        );
        assert_eq!(snapshot.networks[1].ssid, "Cafe");
        assert_eq!(snapshot.connections.len(), 2);
        assert_eq!(snapshot.connections[0].ssid.as_deref(), Some("Home"));
        assert!(!snapshot.connections[0].hidden);
        assert!(snapshot.connections[1].hidden);
        let active = snapshot.active.unwrap();
        assert_eq!(active.connection, "Home");
        assert_eq!(active.primary_ip(), Some("192.168.1.42/24"));
        assert_eq!(active.gateway.as_deref(), Some("192.168.1.1"));
        assert_eq!(active.dns, vec!["1.1.1.1", "8.8.8.8"]);
    }

    #[test]
    fn share_reads_ssid_security_and_secret() {
        let script = r#"#!/bin/sh
case "$*" in
  "-t -f 802-11-wireless.ssid,802-11-wireless-security.key-mgmt,802-11-wireless.hidden connection show uuid abc")
    printf '802-11-wireless.ssid:Cafe\n802-11-wireless-security.key-mgmt:wpa-psk\n802-11-wireless.hidden:no\n' ;;
  "-s -g 802-11-wireless-security.psk connection show uuid abc")
    printf 'pa:ss\n' ;;
  *)
    echo "unexpected args: $*" >&2
    exit 1 ;;
esac
"#;
        let (path, backend) = fake_nmcli(script);
        let info = backend.share_profile("abc", "Cafe").unwrap();
        fs::remove_file(path).ok();

        assert_eq!(info.ssid, "Cafe");
        assert_eq!(info.security, Security::Wpa);
        assert_eq!(info.password.as_deref(), Some("pa:ss"));
        assert!(!info.hidden);
    }
}
