//! Application state, key handling, and backend coordination.

use std::net::Ipv4Addr;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::TableState;

use crate::backend::{
    DetailsData, DetailsPurpose, Request, Response, SavedConnection, Security, ShareInfo, Snapshot,
    SnapshotKind, WifiNetwork,
};
use crate::input::TextInput;
use crate::qr::{self, QrView};

const AUTO_REFRESH: Duration = Duration::from_secs(5);
const STATUS_TTL: Duration = Duration::from_secs(8);
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Wifi,
    Connections,
}

impl Tab {
    pub fn next(self) -> Self {
        match self {
            Self::Wifi => Self::Connections,
            Self::Connections => Self::Wifi,
        }
    }

    pub fn previous(self) -> Self {
        self.next()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct Status {
    pub kind: StatusKind,
    pub text: String,
    pub at: Instant,
}

#[derive(Debug, Clone)]
pub struct Busy {
    pub label: String,
}

#[derive(Debug, Clone)]
pub enum ConfirmAction {
    Disconnect { device: String, ssid: String },
    Deactivate { uuid: String, name: String },
    Delete { uuid: String, name: String },
    DisableWifi,
}

#[derive(Debug, Clone)]
pub struct Confirm {
    pub title: String,
    pub body: String,
    pub action: ConfirmAction,
}

#[derive(Debug, Clone)]
pub enum PromptPurpose {
    ConnectPassword {
        ssid: String,
        bssid: Option<String>,
        hidden: bool,
    },
    SharePassword {
        ssid: String,
        security: Security,
        hidden: bool,
    },
    HiddenSsid,
    HiddenPassword {
        ssid: String,
    },
}

#[derive(Clone)]
pub struct Prompt {
    pub title: String,
    pub label: String,
    pub help: Option<String>,
    pub input: TextInput,
    pub purpose: PromptPurpose,
    pub error: Option<String>,
}

impl Prompt {
    fn password(title: String, purpose: PromptPurpose, help: Option<String>) -> Self {
        Self {
            title,
            label: "Wi-Fi password".to_string(),
            help,
            input: TextInput::secret(),
            purpose,
            error: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ipv4Method {
    Auto,
    Manual,
    Disabled,
    LinkLocal,
    Shared,
}

impl Ipv4Method {
    pub const ALL: [Self; 5] = [
        Self::Auto,
        Self::Manual,
        Self::Disabled,
        Self::LinkLocal,
        Self::Shared,
    ];

    pub fn from_str(value: &str) -> Self {
        match value {
            "manual" => Self::Manual,
            "disabled" => Self::Disabled,
            "link-local" => Self::LinkLocal,
            "shared" => Self::Shared,
            _ => Self::Auto,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Manual => "manual",
            Self::Disabled => "disabled",
            Self::LinkLocal => "link-local",
            Self::Shared => "shared",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Automatic (DHCP)",
            Self::Manual => "Manual",
            Self::Disabled => "Disabled",
            Self::LinkLocal => "Link-local only",
            Self::Shared => "Shared to other computers",
        }
    }

    fn next(self) -> Self {
        let index = Self::ALL
            .iter()
            .position(|method| *method == self)
            .unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    fn previous(self) -> Self {
        let index = Self::ALL
            .iter()
            .position(|method| *method == self)
            .unwrap_or(0);
        Self::ALL[(index + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

#[derive(Debug, Clone)]
pub struct EditForm {
    pub uuid: String,
    pub name: TextInput,
    pub autoconnect: bool,
    pub method: Ipv4Method,
    pub addresses: TextInput,
    pub gateway: TextInput,
    pub dns: TextInput,
    pub focus: usize,
    pub error: Option<String>,
}

impl EditForm {
    pub const NAME: usize = 0;
    pub const AUTOCONNECT: usize = 1;
    pub const METHOD: usize = 2;
    pub const ADDRESSES: usize = 3;
    pub const GATEWAY: usize = 4;
    pub const DNS: usize = 5;
    pub const FIELD_COUNT: usize = 6;

    fn from_details(details: DetailsData) -> Self {
        Self {
            uuid: details.uuid,
            name: TextInput::new(&details.name),
            autoconnect: details.autoconnect,
            method: Ipv4Method::from_str(&details.ipv4_method),
            addresses: TextInput::new(&details.addresses.join(", ")),
            gateway: TextInput::new(details.gateway.as_deref().unwrap_or_default()),
            dns: TextInput::new(&details.dns.join(", ")),
            focus: Self::NAME,
            error: None,
        }
    }

    pub fn text_field_mut(&mut self) -> Option<&mut TextInput> {
        match self.focus {
            Self::NAME => Some(&mut self.name),
            Self::ADDRESSES => Some(&mut self.addresses),
            Self::GATEWAY => Some(&mut self.gateway),
            Self::DNS => Some(&mut self.dns),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DetailsView {
    pub title: String,
    pub lines: Vec<(String, String)>,
    pub scroll: u16,
}

pub enum Overlay {
    Help { scroll: u16 },
    Confirm(Confirm),
    Prompt(Prompt),
    Edit(EditForm),
    Qr(QrView),
    Details(DetailsView),
}

#[derive(Debug, Clone)]
struct PendingConnect {
    ssid: String,
    bssid: Option<String>,
    hidden: bool,
}

pub struct App {
    pub tab: Tab,
    pub snapshot: Snapshot,
    pub loaded: bool,
    pub load_error: Option<String>,
    pub wifi_state: TableState,
    pub conn_state: TableState,
    pub filter: TextInput,
    pub filtering: bool,
    pub overlay: Option<Overlay>,
    pub status: Option<Status>,
    pub busy: Option<Busy>,
    pub refreshing: bool,
    pub spinner: usize,
    pub should_quit: bool,
    pub last_refresh: Instant,
    pub last_snapshot_at: Option<Instant>,
    pub last_warning: Option<String>,
    pending_connect: Option<PendingConnect>,
    tx: Sender<Request>,
    rx: Receiver<Response>,
}

impl App {
    pub fn new(tx: Sender<Request>, rx: Receiver<Response>) -> Self {
        Self {
            tab: Tab::Wifi,
            snapshot: Snapshot::default(),
            loaded: false,
            load_error: None,
            wifi_state: TableState::default(),
            conn_state: TableState::default(),
            filter: TextInput::default(),
            filtering: false,
            overlay: None,
            status: None,
            busy: None,
            refreshing: false,
            spinner: 0,
            should_quit: false,
            last_refresh: Instant::now(),
            last_snapshot_at: None,
            last_warning: None,
            pending_connect: None,
            tx,
            rx,
        }
    }

    pub fn start(&mut self) {
        self.request_refresh();
    }

    pub fn tick(&mut self) {
        self.drain_responses();
        self.spinner = (self.spinner + 1) % SPINNER.len();

        if self
            .status
            .as_ref()
            .is_some_and(|status| status.at.elapsed() >= STATUS_TTL)
        {
            self.status = None;
        }

        if self.loaded
            && !self.refreshing
            && self.busy.is_none()
            && self.overlay.is_none()
            && self.last_refresh.elapsed() >= AUTO_REFRESH
        {
            self.request_refresh();
        }
    }

    pub fn spinner_text(&self) -> &'static str {
        SPINNER[self.spinner % SPINNER.len()]
    }

    pub fn wifi_indices(&self) -> Vec<usize> {
        let needle = self.filter.value().to_lowercase();
        self.snapshot
            .networks
            .iter()
            .enumerate()
            .filter(|(_, network)| {
                needle.is_empty()
                    || network.ssid.to_lowercase().contains(&needle)
                    || network.security_short().to_lowercase().contains(&needle)
                    || network.bssid.to_lowercase().contains(&needle)
            })
            .map(|(index, _)| index)
            .collect()
    }

    pub fn conn_indices(&self) -> Vec<usize> {
        let needle = self.filter.value().to_lowercase();
        self.snapshot
            .connections
            .iter()
            .enumerate()
            .filter(|(_, connection)| {
                needle.is_empty()
                    || connection.name.to_lowercase().contains(&needle)
                    || connection.kind.display().to_lowercase().contains(&needle)
                    || connection
                        .ssid
                        .as_deref()
                        .is_some_and(|ssid| ssid.to_lowercase().contains(&needle))
                    || connection
                        .device
                        .as_deref()
                        .is_some_and(|device| device.to_lowercase().contains(&needle))
            })
            .map(|(index, _)| index)
            .collect()
    }

    pub fn selected_wifi(&self) -> Option<&WifiNetwork> {
        let indices = self.wifi_indices();
        let index = self.wifi_state.selected()?;
        indices
            .get(index)
            .and_then(|network_index| self.snapshot.networks.get(*network_index))
    }

    pub fn selected_conn(&self) -> Option<&SavedConnection> {
        let indices = self.conn_indices();
        let index = self.conn_state.selected()?;
        indices
            .get(index)
            .and_then(|connection_index| self.snapshot.connections.get(*connection_index))
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
            self.should_quit = true;
            return;
        }

        if let Some(overlay) = self.overlay.take() {
            self.overlay = self.handle_overlay(overlay, key);
            return;
        }

        if self.filtering {
            self.handle_filter_key(key);
            return;
        }

        if !is_unmodified(key) {
            return;
        }

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('?') => self.overlay = Some(Overlay::Help { scroll: 0 }),
            KeyCode::Tab | KeyCode::Right => self.switch_tab(self.tab.next()),
            KeyCode::BackTab | KeyCode::Left => self.switch_tab(self.tab.previous()),
            KeyCode::Char('1') => self.switch_tab(Tab::Wifi),
            KeyCode::Char('2') => self.switch_tab(Tab::Connections),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Home | KeyCode::Char('g') => self.select_first(),
            KeyCode::End | KeyCode::Char('G') => self.select_last(),
            KeyCode::Enter | KeyCode::Char('c') => self.primary_action(),
            KeyCode::Char('d') => self.disconnect_or_deactivate(),
            KeyCode::Char('p') => self.share_selected(),
            KeyCode::Char('s') if self.tab == Tab::Wifi => self.scan(),
            KeyCode::Char('w') => self.toggle_wifi(),
            KeyCode::Char('n') if self.tab == Tab::Wifi => self.hidden_network_prompt(),
            KeyCode::Char('e') if self.tab == Tab::Connections => self.edit_selected(),
            KeyCode::Char('a') if self.tab == Tab::Connections => self.toggle_autoconnect(),
            KeyCode::Char('x') | KeyCode::Delete if self.tab == Tab::Connections => {
                self.delete_selected();
            }
            KeyCode::Char('i') => self.details_selected(),
            KeyCode::Char('r') => self.manual_refresh(),
            KeyCode::Char('/') => self.open_filter(),
            KeyCode::Esc if !self.filter.is_empty() => {
                self.filter.clear();
                self.clamp_selections();
            }
            _ => {}
        }
    }

    pub(crate) fn apply_snapshot(&mut self, snapshot: Snapshot) {
        let selected_ssid = self.selected_wifi().map(|network| network.ssid.clone());
        let selected_uuid = self
            .selected_conn()
            .map(|connection| connection.uuid.clone());
        let warning = snapshot.warnings.first().cloned();

        self.snapshot = snapshot;
        self.load_error = None;
        self.restore_wifi_selection(selected_ssid);
        self.restore_conn_selection(selected_uuid);
        self.clamp_selections();
        self.last_snapshot_at = Some(Instant::now());

        if let Some(warning) = &warning
            && self.last_warning.as_ref() != Some(warning)
        {
            self.set_status(StatusKind::Warning, warning.clone());
        }
        self.last_warning = warning;
    }

    fn request_refresh(&mut self) {
        if self.refreshing || self.busy.is_some() {
            return;
        }
        if self.tx.send(Request::Refresh).is_ok() {
            self.refreshing = true;
            self.last_refresh = Instant::now();
        } else {
            self.last_refresh = Instant::now();
            if self.last_snapshot_at.is_none() {
                self.load_error = Some("The backend worker stopped".to_string());
            }
            self.set_status(StatusKind::Error, "The backend worker stopped");
        }
    }

    fn manual_refresh(&mut self) {
        if self.busy.is_some() {
            self.set_status(StatusKind::Info, "Still working; please wait");
        } else if self.refreshing {
            self.set_status(StatusKind::Info, "Refresh already in progress");
        } else {
            self.request_refresh();
        }
    }

    fn drain_responses(&mut self) {
        loop {
            let response = match self.rx.try_recv() {
                Ok(response) => response,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.refreshing = false;
                    self.busy = None;
                    self.loaded = true;
                    if self.last_snapshot_at.is_none() {
                        self.load_error = Some("The backend worker stopped".to_string());
                    }
                    self.set_status(StatusKind::Error, "The backend worker stopped");
                    break;
                }
            };
            match response {
                Response::Snapshot { kind, result } => {
                    match kind {
                        SnapshotKind::Refresh => self.refreshing = false,
                        SnapshotKind::Scan => self.busy = None,
                    }
                    match result {
                        Ok(snapshot) => {
                            self.apply_snapshot(snapshot);
                            self.loaded = true;
                            self.last_refresh = Instant::now();
                            if kind == SnapshotKind::Scan && self.last_warning.is_none() {
                                self.set_status(StatusKind::Success, "Scan complete");
                            }
                        }
                        Err(error) => {
                            self.loaded = true;
                            self.last_refresh = Instant::now();
                            if self.last_snapshot_at.is_none() {
                                self.load_error = Some(error.message().to_string());
                            }
                            self.set_status(
                                StatusKind::Error,
                                format!("Could not refresh NetworkManager: {error}"),
                            );
                        }
                    }
                }
                Response::Action { result, snapshot } => {
                    self.busy = None;
                    let pending = self.pending_connect.take();
                    if let Some(snapshot) = snapshot {
                        self.apply_snapshot(snapshot);
                        self.last_refresh = Instant::now();
                    }
                    match result {
                        Ok(message) => self.set_status(StatusKind::Success, message),
                        Err(error) => {
                            if let Some(pending) =
                                pending.filter(|_| needs_password(error.message()))
                            {
                                self.overlay = Some(Overlay::Prompt(Prompt::password(
                                    format!("Connect to \"{}\"", pending.ssid),
                                    PromptPurpose::ConnectPassword {
                                        ssid: pending.ssid.clone(),
                                        bssid: pending.bssid.clone(),
                                        hidden: pending.hidden,
                                    },
                                    Some(
                                        "The saved profile did not provide a password. This one is not saved."
                                            .to_string(),
                                    ),
                                )));
                                self.set_status(
                                    StatusKind::Info,
                                    "Enter the Wi-Fi password to connect",
                                );
                            } else {
                                self.set_status(StatusKind::Error, error.message().to_string());
                            }
                        }
                    }
                }
                Response::Share { result } => {
                    self.busy = None;
                    match result {
                        Ok(info) => {
                            if let Err(error) = self.open_qr(info) {
                                self.set_status(StatusKind::Error, error);
                            }
                        }
                        Err(error) => {
                            self.set_status(StatusKind::Error, error.message().to_string())
                        }
                    }
                }
                Response::Details { purpose, result } => {
                    self.busy = None;
                    match result {
                        Ok(details) => match purpose {
                            DetailsPurpose::View => {
                                self.overlay = Some(Overlay::Details(self.profile_details(details)))
                            }
                            DetailsPurpose::Edit => {
                                self.overlay = Some(Overlay::Edit(EditForm::from_details(details)))
                            }
                        },
                        Err(error) => {
                            self.set_status(StatusKind::Error, error.message().to_string())
                        }
                    }
                }
            }
        }
    }

    fn handle_filter_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.filter.clear();
                self.filtering = false;
                self.clamp_selections();
            }
            KeyCode::Enter => {
                self.filtering = false;
                self.clamp_selections();
            }
            KeyCode::Tab => {
                self.filtering = false;
                self.switch_tab(self.tab.next());
            }
            KeyCode::BackTab => {
                self.filtering = false;
                self.switch_tab(self.tab.previous());
            }
            _ => {
                if self.filter.handle_key(key) {
                    self.wifi_state.select(Some(0));
                    self.conn_state.select(Some(0));
                    self.clamp_selections();
                }
            }
        }
    }

    fn handle_overlay(&mut self, overlay: Overlay, key: KeyEvent) -> Option<Overlay> {
        match overlay {
            Overlay::Help { scroll } => {
                if !is_unmodified(key) {
                    return Some(Overlay::Help { scroll });
                }
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => None,
                    KeyCode::Down | KeyCode::Char('j') => Some(Overlay::Help {
                        scroll: scroll.saturating_add(1),
                    }),
                    KeyCode::Up | KeyCode::Char('k') => Some(Overlay::Help {
                        scroll: scroll.saturating_sub(1),
                    }),
                    KeyCode::PageDown => Some(Overlay::Help {
                        scroll: scroll.saturating_add(10),
                    }),
                    KeyCode::PageUp => Some(Overlay::Help {
                        scroll: scroll.saturating_sub(10),
                    }),
                    _ => Some(Overlay::Help { scroll }),
                }
            }
            Overlay::Confirm(confirm) => {
                if !is_unmodified(key) {
                    return Some(Overlay::Confirm(confirm));
                }
                match key.code {
                    KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                        self.execute_confirm(confirm.action);
                        None
                    }
                    KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Char('q') => {
                        None
                    }
                    _ => Some(Overlay::Confirm(confirm)),
                }
            }
            Overlay::Prompt(mut prompt) => {
                if is_unmodified(key) {
                    if matches!(key.code, KeyCode::Esc) {
                        return None;
                    }
                    if matches!(key.code, KeyCode::Enter) {
                        return self.submit_prompt(prompt);
                    }
                }
                prompt.input.handle_key(key);
                Some(Overlay::Prompt(prompt))
            }
            Overlay::Edit(mut form) => {
                if is_unmodified(key) {
                    if matches!(key.code, KeyCode::Esc) {
                        return None;
                    }
                    if matches!(key.code, KeyCode::Enter) {
                        return self.submit_edit(form);
                    }
                    match key.code {
                        KeyCode::Tab | KeyCode::Down => {
                            form.focus = (form.focus + 1) % EditForm::FIELD_COUNT;
                            form.error = None;
                        }
                        KeyCode::BackTab | KeyCode::Up => {
                            form.focus =
                                (form.focus + EditForm::FIELD_COUNT - 1) % EditForm::FIELD_COUNT;
                            form.error = None;
                        }
                        KeyCode::Char(' ') if form.focus == EditForm::AUTOCONNECT => {
                            form.autoconnect = !form.autoconnect;
                        }
                        KeyCode::Char(' ') if form.focus == EditForm::METHOD => {
                            form.method = form.method.next();
                        }
                        KeyCode::Left if form.focus == EditForm::METHOD => {
                            form.method = form.method.previous();
                        }
                        KeyCode::Right if form.focus == EditForm::METHOD => {
                            form.method = form.method.next();
                        }
                        _ => {}
                    }
                }
                if let Some(input) = form.text_field_mut() {
                    input.handle_key(key);
                }
                Some(Overlay::Edit(form))
            }
            Overlay::Qr(view) => {
                if !is_unmodified(key) {
                    return Some(Overlay::Qr(view));
                }
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => None,
                    _ => Some(Overlay::Qr(view)),
                }
            }
            Overlay::Details(mut view) => {
                if !is_unmodified(key) {
                    return Some(Overlay::Details(view));
                }
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => None,
                    KeyCode::Down | KeyCode::Char('j') => {
                        view.scroll = view.scroll.saturating_add(1);
                        Some(Overlay::Details(view))
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        view.scroll = view.scroll.saturating_sub(1);
                        Some(Overlay::Details(view))
                    }
                    KeyCode::PageDown => {
                        view.scroll = view.scroll.saturating_add(10);
                        Some(Overlay::Details(view))
                    }
                    KeyCode::PageUp => {
                        view.scroll = view.scroll.saturating_sub(10);
                        Some(Overlay::Details(view))
                    }
                    _ => Some(Overlay::Details(view)),
                }
            }
        }
    }

    fn submit_prompt(&mut self, mut prompt: Prompt) -> Option<Overlay> {
        let value = prompt.input.value();
        match prompt.purpose.clone() {
            PromptPurpose::ConnectPassword {
                ssid,
                bssid,
                hidden,
            } => {
                if value.is_empty() {
                    prompt.error = Some("Password cannot be empty".to_string());
                    return Some(Overlay::Prompt(prompt));
                }
                let target = bssid.unwrap_or_else(|| ssid.clone());
                self.send_action(
                    format!("Connecting to \"{ssid}\"…"),
                    Request::Connect {
                        ssid,
                        target,
                        password: Some(value),
                        hidden,
                    },
                    None,
                );
                None
            }
            PromptPurpose::SharePassword {
                ssid,
                security,
                hidden,
            } => {
                if security.needs_password() && value.is_empty() {
                    prompt.error = Some("Password cannot be empty".to_string());
                    return Some(Overlay::Prompt(prompt));
                }
                let info = ShareInfo {
                    ssid,
                    security,
                    security_label: security.label().to_string(),
                    password: (!value.is_empty()).then_some(value),
                    hidden,
                };
                match self.open_qr(info) {
                    Ok(()) => None,
                    Err(error) => {
                        prompt.error = Some(error);
                        Some(Overlay::Prompt(prompt))
                    }
                }
            }
            PromptPurpose::HiddenSsid => {
                let ssid = value.trim().to_string();
                if ssid.is_empty() {
                    prompt.error = Some("Network name cannot be empty".to_string());
                    return Some(Overlay::Prompt(prompt));
                }
                Some(Overlay::Prompt(Prompt::password(
                    format!("Hidden network \"{ssid}\""),
                    PromptPurpose::HiddenPassword { ssid },
                    Some("Leave blank for an open network".to_string()),
                )))
            }
            PromptPurpose::HiddenPassword { ssid } => {
                let password = (!value.is_empty()).then_some(value);
                self.send_action(
                    format!("Connecting to \"{ssid}\"…"),
                    Request::Connect {
                        ssid: ssid.clone(),
                        target: ssid,
                        password,
                        hidden: true,
                    },
                    None,
                );
                None
            }
        }
    }

    fn submit_edit(&mut self, mut form: EditForm) -> Option<Overlay> {
        let name = form.name.trimmed();
        if name.is_empty() {
            form.error = Some("Connection name cannot be empty".to_string());
            return Some(Overlay::Edit(form));
        }

        let addresses = normalize_list(&form.addresses.value());
        let gateway = form.gateway.trimmed();
        let dns = normalize_list(&form.dns.value());

        if form.method == Ipv4Method::Manual {
            if addresses.is_empty() {
                form.error = Some("Manual IPv4 needs at least one address".to_string());
                return Some(Overlay::Edit(form));
            }
            if let Err(error) = validate_addresses(&addresses) {
                form.error = Some(error);
                return Some(Overlay::Edit(form));
            }
            if !gateway.is_empty() && gateway.parse::<Ipv4Addr>().is_err() {
                form.error = Some("Gateway is not a valid IPv4 address".to_string());
                return Some(Overlay::Edit(form));
            }
            if let Err(error) = validate_dns(&dns) {
                form.error = Some(error);
                return Some(Overlay::Edit(form));
            }
        }

        self.send_action(
            format!("Saving settings for \"{name}\"…"),
            Request::Edit {
                uuid: form.uuid.clone(),
                name,
                autoconnect: form.autoconnect,
                ipv4_method: form.method.as_str().to_string(),
                addresses,
                gateway,
                dns,
            },
            None,
        );
        None
    }

    fn open_qr(&mut self, info: ShareInfo) -> Result<(), String> {
        let payload = qr::wifi_payload(
            &info.ssid,
            info.security,
            info.password.as_deref(),
            info.hidden,
        )?;
        let view = QrView::new(payload, info.ssid, info.security_label)?;
        self.overlay = Some(Overlay::Qr(view));
        Ok(())
    }

    fn send_action(
        &mut self,
        label: impl Into<String>,
        request: Request,
        pending: Option<PendingConnect>,
    ) -> bool {
        if self.busy.is_some() {
            self.set_status(StatusKind::Info, "Still working; please wait");
            return false;
        }
        if self.refreshing {
            self.set_status(StatusKind::Info, "Refresh in progress; try again");
            return false;
        }
        if self.tx.send(request).is_ok() {
            self.busy = Some(Busy {
                label: label.into(),
            });
            self.pending_connect = pending;
            self.status = None;
            true
        } else {
            self.pending_connect = None;
            self.set_status(StatusKind::Error, "The backend worker stopped");
            false
        }
    }

    fn primary_action(&mut self) {
        match self.tab {
            Tab::Wifi => {
                let Some(network) = self.selected_wifi().cloned() else {
                    return;
                };
                if network.active {
                    self.set_status(StatusKind::Info, "Already connected");
                    return;
                }

                if let Some(uuid) = network.saved_uuid.clone() {
                    let hidden = self
                        .snapshot
                        .connections
                        .iter()
                        .find(|connection| connection.uuid == uuid)
                        .is_some_and(|connection| connection.hidden);
                    let pending =
                        matches!(network.security, Security::Wpa | Security::Wep).then(|| {
                            PendingConnect {
                                ssid: network.ssid.clone(),
                                bssid: (!network.bssid.is_empty()).then(|| network.bssid.clone()),
                                hidden,
                            }
                        });
                    self.send_action(
                        format!("Activating \"{}\"…", network.ssid),
                        Request::Activate {
                            uuid,
                            name: network.ssid,
                        },
                        pending,
                    );
                    return;
                }

                match network.security {
                    Security::Enterprise => self.set_status(
                        StatusKind::Warning,
                        "Enterprise (802.1X) networks need nmtui or nmcli",
                    ),
                    Security::Unknown => self.set_status(
                        StatusKind::Warning,
                        format!("Unsupported security: {}", network.security_label),
                    ),
                    Security::Open => {
                        self.send_action(
                            format!("Connecting to \"{}\"…", network.ssid),
                            Request::Connect {
                                ssid: network.ssid.clone(),
                                target: target_for(&network),
                                password: None,
                                hidden: false,
                            },
                            None,
                        );
                    }
                    Security::Wpa | Security::Wep => {
                        self.overlay = Some(Overlay::Prompt(Prompt::password(
                            format!("Connect to \"{}\"", network.ssid),
                            PromptPurpose::ConnectPassword {
                                ssid: network.ssid.clone(),
                                bssid: (!network.bssid.is_empty()).then(|| network.bssid.clone()),
                                hidden: false,
                            },
                            Some(
                                "The password is stored by NetworkManager after connecting."
                                    .to_string(),
                            ),
                        )));
                    }
                }
            }
            Tab::Connections => {
                let Some(connection) = self.selected_conn().cloned() else {
                    return;
                };
                if connection.active {
                    self.set_status(StatusKind::Info, "Already active");
                    return;
                }
                let pending = connection.kind.is_wifi().then(|| PendingConnect {
                    ssid: connection
                        .ssid
                        .clone()
                        .unwrap_or_else(|| connection.name.clone()),
                    bssid: None,
                    hidden: connection.hidden,
                });
                self.send_action(
                    format!("Activating \"{}\"…", connection.name),
                    Request::Activate {
                        uuid: connection.uuid,
                        name: connection.name,
                    },
                    pending,
                );
            }
        }
    }

    fn disconnect_or_deactivate(&mut self) {
        match self.tab {
            Tab::Wifi => {
                let Some(network) = self.selected_wifi().cloned() else {
                    return;
                };
                if !network.active {
                    self.set_status(StatusKind::Info, "Selected network is not connected");
                    return;
                }
                let Some(active) = self.snapshot.active.clone() else {
                    self.set_status(StatusKind::Error, "No active connection was found");
                    return;
                };
                self.overlay = Some(Overlay::Confirm(Confirm {
                    title: "Disconnect Wi-Fi".to_string(),
                    body: format!("Disconnect from \"{}\"?", network.ssid),
                    action: ConfirmAction::Disconnect {
                        device: active.device,
                        ssid: network.ssid,
                    },
                }));
            }
            Tab::Connections => {
                let Some(connection) = self.selected_conn().cloned() else {
                    return;
                };
                if !connection.active {
                    self.set_status(StatusKind::Info, "Selected connection is not active");
                    return;
                }
                self.overlay = Some(Overlay::Confirm(Confirm {
                    title: "Deactivate connection".to_string(),
                    body: format!("Deactivate \"{}\"?", connection.name),
                    action: ConfirmAction::Deactivate {
                        uuid: connection.uuid,
                        name: connection.name,
                    },
                }));
            }
        }
    }

    fn share_selected(&mut self) {
        match self.tab {
            Tab::Wifi => {
                let Some(network) = self.selected_wifi().cloned() else {
                    return;
                };
                if matches!(network.security, Security::Enterprise | Security::Unknown) {
                    self.set_status(
                        StatusKind::Warning,
                        "QR sharing supports open, WEP, and WPA personal networks",
                    );
                    return;
                }
                if let Some(uuid) = network.saved_uuid.clone() {
                    self.send_action(
                        format!("Reading saved password for \"{}\"…", network.ssid),
                        Request::Share {
                            uuid,
                            name: network.ssid,
                        },
                        None,
                    );
                    return;
                }
                if network.security == Security::Open {
                    let info = ShareInfo {
                        ssid: network.ssid,
                        security: network.security,
                        security_label: network.security.label().to_string(),
                        password: None,
                        hidden: false,
                    };
                    if let Err(error) = self.open_qr(info) {
                        self.set_status(StatusKind::Error, error);
                    }
                } else {
                    self.overlay = Some(Overlay::Prompt(Prompt::password(
                        format!("Share \"{}\"", network.ssid),
                        PromptPurpose::SharePassword {
                            ssid: network.ssid,
                            security: network.security,
                            hidden: false,
                        },
                        Some("Used only to render the QR code; it is not saved.".to_string()),
                    )));
                }
            }
            Tab::Connections => {
                let Some(connection) = self.selected_conn().cloned() else {
                    return;
                };
                if !connection.kind.is_wifi() {
                    self.set_status(StatusKind::Info, "QR sharing is only for Wi-Fi profiles");
                    return;
                }
                self.send_action(
                    format!("Reading saved password for \"{}\"…", connection.name),
                    Request::Share {
                        uuid: connection.uuid,
                        name: connection.name,
                    },
                    None,
                );
            }
        }
    }

    fn toggle_wifi(&mut self) {
        if self.busy.is_some() {
            self.set_status(StatusKind::Info, "Still working; please wait");
            return;
        }
        if self.snapshot.wifi_enabled {
            if let Some(active) = self
                .snapshot
                .active
                .clone()
                .filter(|link| link.kind.is_wifi())
            {
                self.overlay = Some(Overlay::Confirm(Confirm {
                    title: "Turn off Wi-Fi".to_string(),
                    body: format!(
                        "Turning off Wi-Fi disconnects \"{}\" on {}.",
                        active.connection, active.device
                    ),
                    action: ConfirmAction::DisableWifi,
                }));
                return;
            }
            self.send_action(
                "Turning Wi-Fi off…",
                Request::SetWifi { enabled: false },
                None,
            );
        } else {
            self.send_action(
                "Turning Wi-Fi on…",
                Request::SetWifi { enabled: true },
                None,
            );
        }
    }

    fn scan(&mut self) {
        self.send_action("Scanning for Wi-Fi networks…", Request::Scan, None);
    }

    fn hidden_network_prompt(&mut self) {
        if !self.snapshot.wifi_enabled {
            self.set_status(StatusKind::Warning, "Turn Wi-Fi on before scanning");
            return;
        }
        self.overlay = Some(Overlay::Prompt(Prompt {
            title: "Connect to hidden network".to_string(),
            label: "Network name (SSID)".to_string(),
            help: Some("The network will be joined with hidden=yes".to_string()),
            input: TextInput::default(),
            purpose: PromptPurpose::HiddenSsid,
            error: None,
        }));
    }

    fn edit_selected(&mut self) {
        let Some(connection) = self.selected_conn().cloned() else {
            return;
        };
        self.send_action(
            format!("Loading settings for \"{}\"…", connection.name),
            Request::Details {
                uuid: connection.uuid,
                kind: connection.kind,
                purpose: DetailsPurpose::Edit,
            },
            None,
        );
    }

    fn details_selected(&mut self) {
        match self.tab {
            Tab::Wifi => {
                if let Some(network) = self.selected_wifi().cloned() {
                    self.overlay = Some(Overlay::Details(self.network_details(&network)));
                }
            }
            Tab::Connections => {
                let Some(connection) = self.selected_conn().cloned() else {
                    return;
                };
                self.send_action(
                    format!("Loading details for \"{}\"…", connection.name),
                    Request::Details {
                        uuid: connection.uuid,
                        kind: connection.kind,
                        purpose: DetailsPurpose::View,
                    },
                    None,
                );
            }
        }
    }

    fn toggle_autoconnect(&mut self) {
        let Some(connection) = self.selected_conn().cloned() else {
            return;
        };
        let enabled = !connection.autoconnect;
        self.send_action(
            format!(
                "Turning autoconnect {} for \"{}\"…",
                if enabled { "on" } else { "off" },
                connection.name
            ),
            Request::SetAutoconnect {
                uuid: connection.uuid,
                name: connection.name,
                enabled,
            },
            None,
        );
    }

    fn delete_selected(&mut self) {
        let Some(connection) = self.selected_conn().cloned() else {
            return;
        };
        let body = if connection.active {
            format!(
                "Delete \"{}\"? It is active, so it will be disconnected first.",
                connection.name
            )
        } else {
            format!("Delete \"{}\"? This cannot be undone.", connection.name)
        };
        self.overlay = Some(Overlay::Confirm(Confirm {
            title: "Delete connection".to_string(),
            body,
            action: ConfirmAction::Delete {
                uuid: connection.uuid,
                name: connection.name,
            },
        }));
    }

    fn execute_confirm(&mut self, action: ConfirmAction) {
        match action {
            ConfirmAction::Disconnect { device, ssid } => {
                self.send_action(
                    format!("Disconnecting from \"{ssid}\"…"),
                    Request::Disconnect { device, ssid },
                    None,
                );
            }
            ConfirmAction::Deactivate { uuid, name } => {
                self.send_action(
                    format!("Deactivating \"{name}\"…"),
                    Request::Deactivate { uuid, name },
                    None,
                );
            }
            ConfirmAction::Delete { uuid, name } => {
                self.send_action(
                    format!("Deleting \"{name}\"…"),
                    Request::Delete { uuid, name },
                    None,
                );
            }
            ConfirmAction::DisableWifi => {
                self.send_action(
                    "Turning Wi-Fi off…",
                    Request::SetWifi { enabled: false },
                    None,
                );
            }
        }
    }

    fn profile_details(&self, details: DetailsData) -> DetailsView {
        let connection = self
            .snapshot
            .connections
            .iter()
            .find(|connection| connection.uuid == details.uuid);
        let mut lines = vec![
            ("Name".to_string(), details.name.clone()),
            ("UUID".to_string(), details.uuid.clone()),
            ("Type".to_string(), details.kind.display().to_string()),
        ];
        if let Some(connection) = connection {
            lines.push((
                "Device".to_string(),
                connection.device.clone().unwrap_or_else(|| "—".to_string()),
            ));
            lines.push((
                "State".to_string(),
                if connection.active {
                    "Active"
                } else {
                    "Inactive"
                }
                .to_string(),
            ));
        }
        lines.push((
            "Autoconnect".to_string(),
            if details.autoconnect { "yes" } else { "no" }.to_string(),
        ));
        lines.push(("IPv4 method".to_string(), details.ipv4_method));
        lines.push(("Addresses".to_string(), join_or_dash(&details.addresses)));
        lines.push((
            "Gateway".to_string(),
            details.gateway.unwrap_or_else(|| "—".to_string()),
        ));
        lines.push(("DNS".to_string(), join_or_dash(&details.dns)));
        if let Some(ssid) = details.ssid {
            lines.push(("SSID".to_string(), ssid));
        }
        if let Some(security) = details.security_label {
            lines.push(("Security".to_string(), security));
        }
        if let Some(active) = &self.snapshot.active
            && connection.is_some_and(|connection| connection.active)
            && active.connection == details.name
        {
            lines.push(("Current IP".to_string(), join_or_dash(&active.ip4)));
            lines.push((
                "Current gateway".to_string(),
                active.gateway.clone().unwrap_or_else(|| "—".to_string()),
            ));
            lines.push(("Current DNS".to_string(), join_or_dash(&active.dns)));
        }

        DetailsView {
            title: format!("Connection · {}", details.name),
            lines,
            scroll: 0,
        }
    }

    fn network_details(&self, network: &WifiNetwork) -> DetailsView {
        let mut lines = vec![
            ("SSID".to_string(), network.ssid.clone()),
            (
                "Status".to_string(),
                if network.active {
                    "Connected"
                } else if network.saved() {
                    "Saved"
                } else {
                    "Available"
                }
                .to_string(),
            ),
            ("Security".to_string(), network.security_short().to_string()),
            ("Signal".to_string(), format!("{}%", network.signal)),
            ("Band".to_string(), network.band().to_string()),
            (
                "Frequency".to_string(),
                if network.freq_mhz > 0 {
                    format!("{} MHz", network.freq_mhz)
                } else {
                    "—".to_string()
                },
            ),
            (
                "Rate".to_string(),
                if network.rate.is_empty() {
                    "—".to_string()
                } else {
                    network.rate.clone()
                },
            ),
            (
                "BSSID".to_string(),
                if network.bssid.is_empty() {
                    "—".to_string()
                } else {
                    network.bssid.clone()
                },
            ),
            (
                "Saved profile".to_string(),
                network
                    .saved_uuid
                    .clone()
                    .unwrap_or_else(|| "no".to_string()),
            ),
        ];
        if network.security == Security::Enterprise {
            lines.push((
                "Note".to_string(),
                "Enterprise (802.1X) needs nmtui or nmcli".to_string(),
            ));
        }
        DetailsView {
            title: format!("Wi-Fi · {}", network.ssid),
            lines,
            scroll: 0,
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let length = match self.tab {
            Tab::Wifi => self.wifi_indices().len(),
            Tab::Connections => self.conn_indices().len(),
        };
        if length == 0 {
            self.set_selection(None);
            return;
        }
        let current = self.current_selection().unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, length as isize - 1);
        self.set_selection(Some(next as usize));
    }

    fn select_first(&mut self) {
        if self.list_length() > 0 {
            self.set_selection(Some(0));
        }
    }

    fn select_last(&mut self) {
        let length = self.list_length();
        if length > 0 {
            self.set_selection(Some(length - 1));
        }
    }

    fn list_length(&self) -> usize {
        match self.tab {
            Tab::Wifi => self.wifi_indices().len(),
            Tab::Connections => self.conn_indices().len(),
        }
    }

    fn current_selection(&self) -> Option<usize> {
        match self.tab {
            Tab::Wifi => self.wifi_state.selected(),
            Tab::Connections => self.conn_state.selected(),
        }
    }

    fn set_selection(&mut self, selection: Option<usize>) {
        match self.tab {
            Tab::Wifi => self.wifi_state.select(selection),
            Tab::Connections => self.conn_state.select(selection),
        }
    }

    fn switch_tab(&mut self, tab: Tab) {
        if self.tab == tab {
            return;
        }
        self.tab = tab;
        self.filter.clear();
        self.filtering = false;
        self.clamp_selections();
    }

    fn open_filter(&mut self) {
        self.filtering = true;
        self.filter.end();
    }

    fn clamp_selections(&mut self) {
        let wifi_len = self.wifi_indices().len();
        let conn_len = self.conn_indices().len();
        clamp_state(&mut self.wifi_state, wifi_len);
        clamp_state(&mut self.conn_state, conn_len);
    }

    fn restore_wifi_selection(&mut self, ssid: Option<String>) {
        let indices = self.wifi_indices();
        if indices.is_empty() {
            self.wifi_state.select(None);
            return;
        }
        let index = ssid
            .and_then(|ssid| {
                indices
                    .iter()
                    .position(|index| self.snapshot.networks[*index].ssid == ssid)
            })
            .unwrap_or(0);
        self.wifi_state.select(Some(index));
    }

    fn restore_conn_selection(&mut self, uuid: Option<String>) {
        let indices = self.conn_indices();
        if indices.is_empty() {
            self.conn_state.select(None);
            return;
        }
        let index = uuid
            .and_then(|uuid| {
                indices
                    .iter()
                    .position(|index| self.snapshot.connections[*index].uuid == uuid)
            })
            .unwrap_or(0);
        self.conn_state.select(Some(index));
    }

    fn set_status(&mut self, kind: StatusKind, text: impl Into<String>) {
        self.status = Some(Status {
            kind,
            text: text.into(),
            at: Instant::now(),
        });
    }
}

fn clamp_state(state: &mut TableState, length: usize) {
    if length == 0 {
        state.select(None);
        return;
    }
    match state.selected() {
        Some(index) if index < length => {}
        Some(_) => state.select(Some(length - 1)),
        None => state.select(Some(0)),
    }
}

fn target_for(network: &WifiNetwork) -> String {
    if network.bssid.is_empty() {
        network.ssid.clone()
    } else {
        network.bssid.clone()
    }
}

fn is_unmodified(key: KeyEvent) -> bool {
    !key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT)
}

fn needs_password(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("password is required")
        || lower.contains("secrets were required")
        || lower.contains("no secrets")
}

fn normalize_list(value: &str) -> String {
    value
        .split(|character: char| character == ',' || character.is_whitespace())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(",")
}

fn validate_addresses(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Ok(());
    }
    for address in value.split(',') {
        let (ip, prefix) = address
            .split_once('/')
            .ok_or_else(|| format!("Address \"{address}\" needs a prefix, e.g. 192.168.1.10/24"))?;
        ip.parse::<Ipv4Addr>()
            .map_err(|_| format!("Address \"{address}\" is not a valid IPv4 address"))?;
        let prefix: u8 = prefix
            .parse()
            .map_err(|_| format!("Prefix in \"{address}\" is not a number"))?;
        if prefix > 32 {
            return Err(format!("Prefix in \"{address}\" is larger than 32"));
        }
    }
    Ok(())
}

fn validate_dns(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Ok(());
    }
    for server in value.split(',') {
        server
            .parse::<Ipv4Addr>()
            .map_err(|_| format!("DNS server \"{server}\" is not a valid IPv4 address"))?;
    }
    Ok(())
}

fn join_or_dash(values: &[String]) -> String {
    if values.is_empty() {
        "—".to_string()
    } else {
        values.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{ActiveLink, ConnectionKind, SavedConnection, Snapshot, WifiNetwork};
    use std::sync::mpsc;

    fn network(ssid: &str, security: Security, saved_uuid: Option<&str>) -> WifiNetwork {
        WifiNetwork {
            ssid: ssid.to_string(),
            signal: 80,
            security,
            security_label: security.label().to_string(),
            freq_mhz: 2437,
            rate: "1170 Mbit/s".to_string(),
            bssid: "AA:BB:CC:DD:EE:FF".to_string(),
            active: false,
            saved_uuid: saved_uuid.map(ToString::to_string),
        }
    }

    fn connection(name: &str, ssid: &str, active: bool) -> SavedConnection {
        SavedConnection {
            uuid: format!("uuid-{name}"),
            name: name.to_string(),
            kind: ConnectionKind::Wifi,
            device: active.then(|| "wlan0".to_string()),
            active,
            autoconnect: true,
            ssid: Some(ssid.to_string()),
            hidden: false,
        }
    }

    fn snapshot() -> Snapshot {
        Snapshot {
            wifi_enabled: true,
            connectivity: "full".to_string(),
            hostname: "testhost".to_string(),
            wifi_device: Some("wlan0".to_string()),
            networks: vec![
                network("Cafe", Security::Open, None),
                network("Home", Security::Wpa, Some("uuid-Home")),
            ],
            connections: vec![connection("Home", "Home", true)],
            active: Some(ActiveLink {
                connection: "Home".to_string(),
                device: "wlan0".to_string(),
                kind: ConnectionKind::Wifi,
                ip4: vec!["192.168.1.42/24".to_string()],
                gateway: Some("192.168.1.1".to_string()),
                dns: vec!["1.1.1.1".to_string()],
            }),
            warnings: Vec::new(),
        }
    }

    fn app() -> (App, mpsc::Receiver<Request>) {
        let (tx, rx) = mpsc::channel();
        let (_backend_tx, backend_rx) = mpsc::channel();
        let mut app = App::new(tx, backend_rx);
        app.apply_snapshot(snapshot());
        (app, rx)
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn key_with_modifiers(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn type_text(app: &mut App, text: &str) {
        for character in text.chars() {
            app.handle_key(key(KeyCode::Char(character)));
        }
    }

    #[test]
    fn open_network_connects_without_a_prompt() {
        let (mut app, rx) = app();
        app.handle_key(key(KeyCode::Enter));
        match rx.try_recv().unwrap() {
            Request::Connect {
                ssid,
                target,
                password,
                hidden,
            } => {
                assert_eq!(ssid, "Cafe");
                assert_eq!(target, "AA:BB:CC:DD:EE:FF");
                assert!(password.is_none());
                assert!(!hidden);
            }
            _ => panic!("expected a connect request"),
        }
        assert!(app.busy.is_some());
    }

    #[test]
    fn secured_unsaved_network_prompts_for_password() {
        let (mut app, rx) = app();
        app.snapshot.networks[1].saved_uuid = None;
        app.wifi_state.select(Some(1));
        app.handle_key(key(KeyCode::Enter));
        assert!(matches!(app.overlay, Some(Overlay::Prompt(_))));
        assert!(rx.try_recv().is_err());

        type_text(&mut app, "secret");
        app.handle_key(key(KeyCode::Enter));
        match rx.try_recv().unwrap() {
            Request::Connect { ssid, password, .. } => {
                assert_eq!(ssid, "Home");
                assert_eq!(password.as_deref(), Some("secret"));
            }
            _ => panic!("expected a connect request"),
        }
    }

    #[test]
    fn saved_network_activates_the_profile() {
        let (mut app, rx) = app();
        app.wifi_state.select(Some(1));
        app.handle_key(key(KeyCode::Enter));
        match rx.try_recv().unwrap() {
            Request::Activate { uuid, name } => {
                assert_eq!(uuid, "uuid-Home");
                assert_eq!(name, "Home");
            }
            _ => panic!("expected an activate request"),
        }
    }

    #[test]
    fn share_open_network_opens_qr_without_backend() {
        let (mut app, rx) = app();
        app.handle_key(key(KeyCode::Char('p')));
        assert!(matches!(app.overlay, Some(Overlay::Qr(_))));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn share_saved_network_reads_the_secret() {
        let (mut app, rx) = app();
        app.wifi_state.select(Some(1));
        app.handle_key(key(KeyCode::Char('p')));
        match rx.try_recv().unwrap() {
            Request::Share { uuid, name } => {
                assert_eq!(uuid, "uuid-Home");
                assert_eq!(name, "Home");
            }
            _ => panic!("expected a share request"),
        }
    }

    #[test]
    fn filter_matches_and_escape_clears() {
        let (mut app, _rx) = app();
        app.handle_key(key(KeyCode::Char('/')));
        type_text(&mut app, "home");
        assert_eq!(app.wifi_indices().len(), 1);
        app.handle_key(key(KeyCode::Esc));
        assert!(app.filter.is_empty());
        assert_eq!(app.wifi_indices().len(), 2);
    }

    #[test]
    fn switching_tabs_clears_the_filter() {
        let (mut app, _rx) = app();
        app.handle_key(key(KeyCode::Char('/')));
        type_text(&mut app, "home");
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.tab, Tab::Connections);
        assert!(app.filter.is_empty());
    }

    #[test]
    fn delete_requires_confirmation() {
        let (mut app, rx) = app();
        app.handle_key(key(KeyCode::Tab));
        app.handle_key(key(KeyCode::Char('x')));
        assert!(matches!(app.overlay, Some(Overlay::Confirm(_))));
        assert!(rx.try_recv().is_err());
        app.handle_key(key(KeyCode::Char('y')));
        match rx.try_recv().unwrap() {
            Request::Delete { uuid, .. } => assert_eq!(uuid, "uuid-Home"),
            _ => panic!("expected a delete request"),
        }
    }

    #[test]
    fn password_prompt_requires_a_value() {
        let (mut app, rx) = app();
        app.snapshot.networks[1].saved_uuid = None;
        app.wifi_state.select(Some(1));
        app.handle_key(key(KeyCode::Enter));
        app.handle_key(key(KeyCode::Enter));
        assert!(rx.try_recv().is_err());
        assert!(matches!(
            app.overlay,
            Some(Overlay::Prompt(Prompt { error: Some(_), .. }))
        ));
    }

    #[test]
    fn manual_ipv4_form_accepts_plain_dns_addresses() {
        let (mut app, rx) = app();
        app.overlay = Some(Overlay::Edit(EditForm {
            uuid: "uuid-Home".to_string(),
            name: TextInput::new("Home"),
            autoconnect: true,
            method: Ipv4Method::Manual,
            addresses: TextInput::new("10.0.0.2/24"),
            gateway: TextInput::new("10.0.0.1"),
            dns: TextInput::new("1.1.1.1"),
            focus: EditForm::NAME,
            error: None,
        }));
        app.handle_key(key(KeyCode::Enter));
        assert!(app.overlay.is_none());
        match rx.try_recv().unwrap() {
            Request::Edit { dns, .. } => assert_eq!(dns, "1.1.1.1"),
            _ => panic!("expected an edit request"),
        }
    }

    #[test]
    fn manual_ipv4_form_rejects_invalid_dns() {
        let (mut app, rx) = app();
        app.overlay = Some(Overlay::Edit(EditForm {
            uuid: "uuid-Home".to_string(),
            name: TextInput::new("Home"),
            autoconnect: true,
            method: Ipv4Method::Manual,
            addresses: TextInput::new("10.0.0.2/24"),
            gateway: TextInput::new(""),
            dns: TextInput::new("not-a-dns"),
            focus: EditForm::NAME,
            error: None,
        }));
        app.handle_key(key(KeyCode::Enter));
        assert!(matches!(
            app.overlay,
            Some(Overlay::Edit(EditForm { error: Some(_), .. }))
        ));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn manual_ipv4_form_rejects_non_cidr_addresses() {
        let (mut app, rx) = app();
        app.overlay = Some(Overlay::Edit(EditForm {
            uuid: "uuid-Home".to_string(),
            name: TextInput::new("Home"),
            autoconnect: true,
            method: Ipv4Method::Manual,
            addresses: TextInput::new("10.0.0.2"),
            gateway: TextInput::new(""),
            dns: TextInput::new("1.1.1.1"),
            focus: EditForm::NAME,
            error: None,
        }));
        app.handle_key(key(KeyCode::Enter));
        assert!(matches!(
            app.overlay,
            Some(Overlay::Edit(EditForm { error: Some(_), .. }))
        ));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn manual_ipv4_form_can_clear_gateway_and_dns() {
        let (mut app, rx) = app();
        app.overlay = Some(Overlay::Edit(EditForm {
            uuid: "uuid-Home".to_string(),
            name: TextInput::new("Home"),
            autoconnect: true,
            method: Ipv4Method::Manual,
            addresses: TextInput::new("10.0.0.2/24"),
            gateway: TextInput::new(""),
            dns: TextInput::new(""),
            focus: EditForm::NAME,
            error: None,
        }));
        app.handle_key(key(KeyCode::Enter));
        match rx.try_recv().unwrap() {
            Request::Edit { gateway, dns, .. } => {
                assert_eq!(gateway, "");
                assert_eq!(dns, "");
            }
            _ => panic!("expected an edit request"),
        }
    }

    #[test]
    fn switching_to_auto_passes_through_form_values() {
        let (mut app, rx) = app();
        app.overlay = Some(Overlay::Edit(EditForm {
            uuid: "uuid-Home".to_string(),
            name: TextInput::new("Home"),
            autoconnect: true,
            method: Ipv4Method::Auto,
            addresses: TextInput::new("10.0.0.2/24"),
            gateway: TextInput::new("10.0.0.1"),
            dns: TextInput::new("1.1.1.1"),
            focus: EditForm::NAME,
            error: None,
        }));
        app.handle_key(key(KeyCode::Enter));
        match rx.try_recv().unwrap() {
            Request::Edit { ipv4_method, .. } => assert_eq!(ipv4_method, "auto"),
            _ => panic!("expected an edit request"),
        }
    }

    #[test]
    fn control_keys_do_not_trigger_shortcuts() {
        let (mut app, rx) = app();
        app.tab = Tab::Connections;
        app.handle_key(key_with_modifiers(
            KeyCode::Char('a'),
            KeyModifiers::CONTROL,
        ));
        assert!(rx.try_recv().is_err());
        app.handle_key(key_with_modifiers(
            KeyCode::Char('y'),
            KeyModifiers::CONTROL,
        ));
        assert!(rx.try_recv().is_err());
        assert!(app.overlay.is_none());
    }

    #[test]
    fn control_keys_do_not_confirm_deletion() {
        let (mut app, rx) = app();
        app.handle_key(key(KeyCode::Tab));
        app.handle_key(key(KeyCode::Char('x')));
        assert!(matches!(app.overlay, Some(Overlay::Confirm(_))));
        app.handle_key(key_with_modifiers(
            KeyCode::Char('y'),
            KeyModifiers::CONTROL,
        ));
        assert!(rx.try_recv().is_err());
        assert!(matches!(app.overlay, Some(Overlay::Confirm(_))));
    }

    #[test]
    fn control_keys_do_not_open_qr() {
        let (mut app, rx) = app();
        app.handle_key(key_with_modifiers(
            KeyCode::Char('p'),
            KeyModifiers::CONTROL,
        ));
        assert!(rx.try_recv().is_err());
        assert!(app.overlay.is_none());
    }

    #[test]
    fn saved_network_activation_carries_hidden_flag() {
        let (mut app, _rx) = app();
        app.snapshot.connections[0].hidden = true;
        app.wifi_state.select(Some(1));
        app.handle_key(key(KeyCode::Enter));
        assert!(matches!(
            app.pending_connect,
            Some(PendingConnect { hidden: true, .. })
        ));
    }

    #[test]
    fn manual_ipv4_form_requires_cidr() {
        let (mut app, rx) = app();
        app.overlay = Some(Overlay::Edit(EditForm {
            uuid: "uuid-Home".to_string(),
            name: TextInput::new("Home"),
            autoconnect: true,
            method: Ipv4Method::Manual,
            addresses: TextInput::new("not-an-address"),
            gateway: TextInput::new(""),
            dns: TextInput::new(""),
            focus: EditForm::NAME,
            error: None,
        }));
        app.handle_key(key(KeyCode::Enter));
        assert!(matches!(
            app.overlay,
            Some(Overlay::Edit(EditForm { error: Some(_), .. }))
        ));
        assert!(rx.try_recv().is_err());
    }
}
