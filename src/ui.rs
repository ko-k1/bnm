//! Ratatui rendering for bnm.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Cell, Clear, Paragraph, Row, Table, Wrap};
use unicode_width::UnicodeWidthChar;

use crate::app::{App, DetailsView, EditForm, Ipv4Method, Overlay, Prompt, StatusKind, Tab};
use crate::backend::{SavedConnection, Security, WifiNetwork};
use crate::input::TextInput;

const ACCENT: Color = Color::Cyan;
const OK: Color = Color::Green;
const WARN: Color = Color::Yellow;
const ERR: Color = Color::Red;
const MUTED: Color = Color::Gray;
const FAINT: Color = Color::DarkGray;
const SELECTED_BG: Color = Color::Rgb(38, 52, 60);

pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    if area.width < 50 || area.height < 16 {
        if let Some(overlay) = &app.overlay {
            render_overlay(frame, overlay, area);
        } else {
            render_too_small(frame, area);
        }
        return;
    }

    let [header, tabs, body, footer] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Min(6),
        Constraint::Length(2),
    ])
    .areas(area);

    render_header(frame, app, header);
    render_tabs(frame, app, tabs);

    match app.tab {
        Tab::Wifi => render_wifi(frame, app, body),
        Tab::Connections => render_connections(frame, app, body),
    }

    render_footer(frame, app, footer);

    if let Some(overlay) = &app.overlay {
        render_overlay(frame, overlay, area);
    }
}

fn render_too_small(frame: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(Span::styled(
            "Terminal too small",
            Style::new().fg(WARN).bold(),
        )),
        Line::from(""),
        Line::from(format!(
            "bnm needs at least 50×16 (now {}×{})",
            area.width, area.height
        )),
        Line::from(""),
        Line::from(Span::styled("q / Ctrl+C quit", Style::new().fg(MUTED))),
    ];
    let paragraph = Paragraph::new(lines)
        .alignment(Alignment::Center)
        .block(panel_block(" bnm "));
    frame.render_widget(paragraph, area);
}

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let [first, second] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(area);
    let [first_left, first_right] =
        Layout::horizontal([Constraint::Percentage(56), Constraint::Percentage(44)]).areas(first);
    let [second_left, second_right] =
        Layout::horizontal([Constraint::Percentage(56), Constraint::Percentage(44)]).areas(second);

    let title = Line::from(vec![
        Span::styled(" bnm ", Style::new().fg(Color::Black).bg(ACCENT).bold()),
        Span::styled(" better network manager", Style::new().fg(MUTED)),
    ]);
    frame.render_widget(Paragraph::new(title), first_left);

    let mut status_spans = Vec::new();
    if !app.loaded {
        status_spans.push(Span::styled("starting…", Style::new().fg(MUTED)));
    } else if app.load_error.is_some() {
        status_spans.push(Span::styled("unavailable ", Style::new().fg(WARN).bold()));
        status_spans.push(Span::styled("✗", Style::new().fg(WARN)));
    } else if let Some(active) = &app.snapshot.active {
        status_spans.push(Span::styled("● ", Style::new().fg(OK)));
        status_spans.push(Span::styled(
            clip(&active.connection, 22),
            Style::new().fg(OK).bold(),
        ));
        if let Some(ip) = active.primary_ip() {
            status_spans.push(Span::styled(
                format!("  {}", clip(ip, 18)),
                Style::new().fg(MUTED),
            ));
        }
    } else if !app.snapshot.wifi_enabled {
        status_spans.push(Span::styled("wifi off ", Style::new().fg(WARN).bold()));
        status_spans.push(Span::styled("○", Style::new().fg(WARN)));
    } else {
        status_spans.push(Span::styled("○ offline", Style::new().fg(MUTED)));
    }
    frame.render_widget(
        Paragraph::new(Line::from(status_spans)).alignment(Alignment::Right),
        first_right,
    );

    let left_line = if app.loaded {
        if let Some(error) = &app.load_error {
            Line::from(vec![
                Span::styled(
                    " NetworkManager unavailable: ",
                    Style::new().fg(WARN).bold(),
                ),
                Span::styled(clip(error, 48), Style::new().fg(MUTED)),
            ])
        } else {
            let mut left_spans = vec![Span::styled(
                format!(" {} ", app.snapshot.hostname),
                Style::new().fg(MUTED),
            )];
            if let Some(device) = &app.snapshot.wifi_device {
                left_spans.push(Span::styled("· ", Style::new().fg(FAINT)));
                left_spans.push(Span::styled(format!("{device} "), Style::new().fg(MUTED)));
            }
            left_spans.push(Span::styled("· Wi-Fi ", Style::new().fg(FAINT)));
            left_spans.push(Span::styled(
                if app.snapshot.wifi_enabled {
                    "on"
                } else {
                    "off"
                },
                Style::new().fg(if app.snapshot.wifi_enabled { OK } else { WARN }),
            ));
            if !app.snapshot.connectivity.is_empty() {
                left_spans.push(Span::styled(" · ", Style::new().fg(FAINT)));
                left_spans.push(Span::styled(
                    app.snapshot.connectivity.clone(),
                    Style::new().fg(MUTED),
                ));
            }
            Line::from(left_spans)
        }
    } else {
        Line::from(Span::styled(
            " starting NetworkManager view…",
            Style::new().fg(FAINT),
        ))
    };
    frame.render_widget(Paragraph::new(left_line), second_left);

    let right = if let Some(busy) = &app.busy {
        Line::from(vec![
            Span::styled(format!("{} ", app.spinner_text()), Style::new().fg(WARN)),
            Span::styled(clip(&busy.label, 38), Style::new().fg(WARN)),
        ])
    } else if app.refreshing {
        Line::from(vec![
            Span::styled(format!("{} ", app.spinner_text()), Style::new().fg(ACCENT)),
            Span::styled("refreshing…", Style::new().fg(MUTED)),
        ])
    } else if let Some(updated) = app.last_snapshot_at {
        let seconds = updated.elapsed().as_secs();
        let text = if seconds < 1 {
            "updated just now".to_string()
        } else {
            format!("updated {seconds}s ago")
        };
        Line::from(Span::styled(text, Style::new().fg(FAINT)))
    } else {
        Line::from(Span::styled("connecting…", Style::new().fg(FAINT)))
    };
    frame.render_widget(
        Paragraph::new(right).alignment(Alignment::Right),
        second_right,
    );
}

fn render_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).areas(area);

    let mut tabs = Vec::new();
    for (index, tab) in [Tab::Wifi, Tab::Connections].into_iter().enumerate() {
        let active = app.tab == tab;
        let style = if active {
            Style::new().fg(Color::Black).bg(ACCENT).bold()
        } else {
            Style::new().fg(MUTED)
        };
        tabs.push(Span::styled(
            format!(" {} {} ", index + 1, tab_title(tab)),
            style,
        ));
        tabs.push(Span::raw(" "));
    }
    frame.render_widget(Paragraph::new(Line::from(tabs)), left);

    let mut right_spans = Vec::new();
    if !app.filter.is_empty() {
        right_spans.push(Span::styled("filter: ", Style::new().fg(FAINT)));
        right_spans.push(Span::styled(
            clip(&app.filter.display_value(), 24),
            Style::new().fg(ACCENT),
        ));
    }
    if !app.snapshot.warnings.is_empty() {
        if !right_spans.is_empty() {
            right_spans.push(Span::styled("  ", Style::new().fg(FAINT)));
        }
        right_spans.push(Span::styled("⚠ partial", Style::new().fg(WARN)));
    }
    frame.render_widget(
        Paragraph::new(Line::from(right_spans)).alignment(Alignment::Right),
        right,
    );
}

fn render_wifi(frame: &mut Frame, app: &mut App, area: Rect) {
    let [list_area, _gap, details_area] = Layout::horizontal([
        Constraint::Percentage(57),
        Constraint::Length(1),
        Constraint::Percentage(42),
    ])
    .areas(area);

    let indices = app.wifi_indices();
    let total = app.snapshot.networks.len();
    let title = if app.filter.is_empty() {
        format!(" Wi-Fi networks ({total}) ")
    } else {
        format!(" Wi-Fi ({}/{total}) ", indices.len())
    };

    if indices.is_empty() {
        let block = panel_block(&title);
        let message = if !app.loaded {
            "Loading Wi-Fi networks…".to_string()
        } else if let Some(error) = &app.load_error {
            format!("NetworkManager is unavailable.\n\n{error}\n\nPress r to retry.")
        } else if !app.snapshot.wifi_enabled {
            "Wi-Fi is off. Press w to turn it on.".to_string()
        } else if !app.filter.is_empty() {
            "No networks match this filter.".to_string()
        } else {
            "No networks found. Press s to scan.".to_string()
        };
        frame.render_widget(
            Paragraph::new(message)
                .alignment(Alignment::Center)
                .style(Style::new().fg(MUTED))
                .wrap(Wrap { trim: false })
                .block(block),
            list_area,
        );
    } else {
        let rows = indices
            .iter()
            .filter_map(|index| app.snapshot.networks.get(*index).map(wifi_row));
        let header = Row::new(vec!["", "Signal", "SSID", "Security", "Band"])
            .style(Style::new().fg(MUTED).bold());
        let table = Table::new(
            rows,
            [
                Constraint::Length(1),
                Constraint::Length(8),
                Constraint::Min(10),
                Constraint::Length(10),
                Constraint::Length(4),
            ],
        )
        .header(header)
        .block(panel_block(&title))
        .column_spacing(1)
        .row_highlight_style(
            Style::new()
                .bg(SELECTED_BG)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        );
        frame.render_stateful_widget(table, list_area, &mut app.wifi_state);
    }

    let selected = app.selected_wifi().cloned();
    let lines = match selected {
        Some(network) => wifi_detail_lines(&network),
        None if !app.loaded => vec![Line::from(Span::styled(
            "Loading NetworkManager state…",
            Style::new().fg(MUTED),
        ))],
        None if app.load_error.is_some() => vec![
            Line::from(Span::styled(
                "NetworkManager unavailable",
                Style::new().fg(WARN).bold(),
            )),
            Line::from(""),
            Line::from("Press r to retry."),
        ],
        None if !app.snapshot.wifi_enabled => vec![
            Line::from(Span::styled("Wi-Fi is off", Style::new().fg(WARN).bold())),
            Line::from(""),
            Line::from("Press w to turn the radio on."),
        ],
        None => vec![
            Line::from(Span::styled("No network selected", Style::new().fg(MUTED))),
            Line::from(""),
            Line::from("Move with j/k and press Enter to connect."),
        ],
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block("Selected network"))
            .wrap(Wrap { trim: false }),
        details_area,
    );
}

fn render_connections(frame: &mut Frame, app: &mut App, area: Rect) {
    let [list_area, _gap, details_area] = Layout::horizontal([
        Constraint::Percentage(57),
        Constraint::Length(1),
        Constraint::Percentage(42),
    ])
    .areas(area);

    let indices = app.conn_indices();
    let total = app.snapshot.connections.len();
    let title = if app.filter.is_empty() {
        format!(" Saved connections ({total}) ")
    } else {
        format!(" Connections ({}/{total}) ", indices.len())
    };

    if indices.is_empty() {
        let block = panel_block(&title);
        let message = if !app.loaded {
            "Loading saved connections…".to_string()
        } else if let Some(error) = &app.load_error {
            format!("NetworkManager is unavailable.\n\n{error}\n\nPress r to retry.")
        } else if !app.filter.is_empty() {
            "No connections match this filter.".to_string()
        } else {
            "No saved connections.".to_string()
        };
        frame.render_widget(
            Paragraph::new(message)
                .alignment(Alignment::Center)
                .style(Style::new().fg(MUTED))
                .wrap(Wrap { trim: false })
                .block(block),
            list_area,
        );
    } else {
        let rows = indices
            .iter()
            .filter_map(|index| app.snapshot.connections.get(*index).map(connection_row));
        let header = Row::new(vec!["", "Connection", "Type", "Device", "Auto"])
            .style(Style::new().fg(MUTED).bold());
        let table = Table::new(
            rows,
            [
                Constraint::Length(1),
                Constraint::Min(12),
                Constraint::Length(9),
                Constraint::Length(8),
                Constraint::Length(4),
            ],
        )
        .header(header)
        .block(panel_block(&title))
        .column_spacing(1)
        .row_highlight_style(
            Style::new()
                .bg(SELECTED_BG)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        );
        frame.render_stateful_widget(table, list_area, &mut app.conn_state);
    }

    let selected = app.selected_conn().cloned();
    let lines = match selected {
        Some(connection) => connection_detail_lines(&connection),
        None if !app.loaded => vec![Line::from(Span::styled(
            "Loading NetworkManager state…",
            Style::new().fg(MUTED),
        ))],
        None if app.load_error.is_some() => vec![
            Line::from(Span::styled(
                "NetworkManager unavailable",
                Style::new().fg(WARN).bold(),
            )),
            Line::from(""),
            Line::from("Press r to retry."),
        ],
        None => vec![
            Line::from(Span::styled(
                "No connection selected",
                Style::new().fg(MUTED),
            )),
            Line::from(""),
            Line::from("Press Enter to activate a saved profile."),
        ],
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block("Selected connection"))
            .wrap(Wrap { trim: false }),
        details_area,
    );
}

fn wifi_row(network: &WifiNetwork) -> Row<'static> {
    let marker = if network.active {
        Span::styled("●", Style::new().fg(OK))
    } else if network.saved() {
        Span::styled("◆", Style::new().fg(ACCENT))
    } else {
        Span::raw(" ")
    };

    let signal_style = signal_style(network.signal);
    let bars = signal_bars(network.signal);
    let signal = Line::from(vec![
        Span::styled(bars, signal_style),
        Span::styled(format!(" {:>3}", network.signal), Style::new().fg(MUTED)),
    ]);

    let ssid_style = if network.active {
        Style::new().fg(OK).bold()
    } else {
        Style::new()
    };
    let security_style = match network.security {
        Security::Open => Style::new().fg(MUTED),
        Security::Wep => Style::new().fg(ERR),
        Security::Enterprise => Style::new().fg(WARN),
        Security::Unknown => Style::new().fg(WARN),
        Security::Wpa => Style::new(),
    };

    Row::new(vec![
        Cell::from(Line::from(marker)),
        Cell::from(signal),
        Cell::from(Line::from(Span::styled(network.ssid.clone(), ssid_style))),
        Cell::from(Line::from(Span::styled(
            network.security_short().to_string(),
            security_style,
        ))),
        Cell::from(Line::from(Span::styled(
            network.band_short().to_string(),
            Style::new().fg(FAINT),
        ))),
    ])
}

fn connection_row(connection: &SavedConnection) -> Row<'static> {
    let marker = if connection.active {
        Span::styled("●", Style::new().fg(OK))
    } else {
        Span::raw(" ")
    };
    let name_style = if connection.active {
        Style::new().fg(OK).bold()
    } else {
        Style::new()
    };
    let device = connection.device.clone().unwrap_or_else(|| "—".to_string());
    Row::new(vec![
        Cell::from(Line::from(marker)),
        Cell::from(Line::from(Span::styled(
            connection.name.clone(),
            name_style,
        ))),
        Cell::from(Line::from(Span::styled(
            connection.kind.display().to_string(),
            Style::new().fg(MUTED),
        ))),
        Cell::from(Line::from(Span::styled(device, Style::new().fg(MUTED)))),
        Cell::from(Line::from(Span::styled(
            if connection.autoconnect { "✓" } else { "—" }.to_string(),
            Style::new().fg(if connection.autoconnect { OK } else { FAINT }),
        ))),
    ])
}

fn wifi_detail_lines(network: &WifiNetwork) -> Vec<Line<'static>> {
    let status = if network.active {
        Span::styled("● Connected", Style::new().fg(OK).bold())
    } else if network.saved() {
        Span::styled("◆ Saved profile", Style::new().fg(ACCENT))
    } else {
        Span::styled("Available", Style::new().fg(MUTED))
    };

    let mut lines = vec![
        Line::from(Span::styled(
            network.ssid.clone(),
            Style::new().fg(ACCENT).bold(),
        )),
        Line::from(status),
        Line::from(""),
        field_line("Security", network.security_short()),
        field_line(
            "Signal",
            &format!("{}%  {}", network.signal, signal_bars(network.signal)),
        ),
        field_line(
            "Band",
            &if network.freq_mhz > 0 {
                format!("{} · {} MHz", network.band(), network.freq_mhz)
            } else {
                "—".to_string()
            },
        ),
        field_line(
            "Rate",
            if network.rate.is_empty() {
                "—"
            } else {
                &network.rate
            },
        ),
        field_line(
            "BSSID",
            if network.bssid.is_empty() {
                "—"
            } else {
                &network.bssid
            },
        ),
        field_line(
            "Profile",
            if network.saved() {
                "saved"
            } else {
                "not saved"
            },
        ),
        Line::from(""),
        Line::from(Span::styled("Actions", Style::new().fg(MUTED).bold())),
    ];
    if network.active {
        lines.push(action_line("d", "disconnect"));
    } else {
        lines.push(action_line("Enter", "connect"));
    }
    lines.push(action_line("p", "share QR code"));
    lines.push(action_line("i", "network details"));

    if network.security == Security::Enterprise {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Enterprise (802.1X) needs nmtui or nmcli.",
            Style::new().fg(WARN),
        )));
    }
    lines
}

fn connection_detail_lines(connection: &SavedConnection) -> Vec<Line<'static>> {
    let state = if connection.active {
        Span::styled("● Active", Style::new().fg(OK).bold())
    } else {
        Span::styled("Inactive", Style::new().fg(MUTED))
    };
    let mut lines = vec![
        Line::from(Span::styled(
            connection.name.clone(),
            Style::new().fg(ACCENT).bold(),
        )),
        Line::from(state),
        Line::from(""),
        field_line("Type", connection.kind.display()),
        field_line("Device", connection.device.as_deref().unwrap_or("—")),
        field_line(
            "Autoconnect",
            if connection.autoconnect { "yes" } else { "no" },
        ),
    ];
    if let Some(ssid) = &connection.ssid {
        lines.push(field_line("SSID", ssid));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Actions",
        Style::new().fg(MUTED).bold(),
    )));
    if connection.active {
        lines.push(action_line("d", "deactivate"));
    } else {
        lines.push(action_line("Enter", "activate"));
    }
    lines.push(action_line("e", "edit settings"));
    lines.push(action_line("a", "toggle autoconnect"));
    lines.push(action_line("x", "delete profile"));
    if connection.kind.is_wifi() {
        lines.push(action_line("p", "share QR code"));
    }
    lines.push(action_line("i", "profile details"));
    lines
}

fn field_line(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<11}"), Style::new().fg(MUTED)),
        Span::raw(value.to_string()),
    ])
}

fn action_line(key: &str, description: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!(" {key:<7}"), Style::new().fg(ACCENT).bold()),
        Span::styled(description.to_string(), Style::new().fg(MUTED)),
    ])
}

fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let [first, second] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(area);

    let first_line = if !app.loaded {
        Line::from(vec![
            Span::styled(format!("{} ", app.spinner_text()), Style::new().fg(ACCENT)),
            Span::styled("Loading NetworkManager state…", Style::new().fg(MUTED)),
        ])
    } else if let Some(error) = &app.load_error {
        Line::from(vec![
            Span::styled("✗ ", Style::new().fg(ERR).bold()),
            Span::styled(
                clip(&format!("NetworkManager unavailable: {error}"), 90),
                Style::new().fg(ERR),
            ),
        ])
    } else if app.filtering {
        let mut spans = vec![Span::styled("Filter  ", Style::new().fg(ACCENT).bold())];
        spans.extend(input_spans(&app.filter, true));
        spans.push(Span::styled(
            format!(
                "   {} match{}",
                match app.tab {
                    Tab::Wifi => app.wifi_indices().len(),
                    Tab::Connections => app.conn_indices().len(),
                },
                if match app.tab {
                    Tab::Wifi => app.wifi_indices().len(),
                    Tab::Connections => app.conn_indices().len(),
                } == 1
                {
                    ""
                } else {
                    "es"
                }
            ),
            Style::new().fg(FAINT),
        ));
        Line::from(spans)
    } else if let Some(busy) = &app.busy {
        Line::from(vec![
            Span::styled(format!("{} ", app.spinner_text()), Style::new().fg(WARN)),
            Span::styled(clip(&busy.label, 70), Style::new().fg(WARN)),
        ])
    } else if let Some(status) = &app.status {
        let (icon, color) = match status.kind {
            StatusKind::Info => ("·", ACCENT),
            StatusKind::Success => ("✓", OK),
            StatusKind::Warning => ("!", WARN),
            StatusKind::Error => ("✗", ERR),
        };
        Line::from(vec![
            Span::styled(format!("{icon} "), Style::new().fg(color).bold()),
            Span::styled(clip(&status.text, 90), Style::new().fg(color)),
        ])
    } else {
        let hint = match app.tab {
            Tab::Wifi => "Select a network and press Enter to connect, or p to share it.",
            Tab::Connections => "Activate, edit, or delete saved NetworkManager profiles.",
        };
        Line::from(Span::styled(hint, Style::new().fg(FAINT)))
    };
    frame.render_widget(Paragraph::new(first_line), first);

    let hints = if app.filtering {
        hint_spans(&[("Enter", "apply"), ("Esc", "clear"), ("type", "to filter")])
    } else if app.busy.is_some() {
        hint_spans(&[("Ctrl+C", "quit"), ("wait", "for the operation")])
    } else if app.load_error.is_some() {
        hint_spans(&[("r", "retry"), ("q", "quit")])
    } else {
        match app.tab {
            Tab::Wifi => hint_spans(&[
                ("Enter", "connect"),
                ("p", "QR"),
                ("d", "disconnect"),
                ("s", "scan"),
                ("w", "radio"),
                ("n", "hidden"),
                ("/", "filter"),
                ("?", "help"),
                ("q", "quit"),
            ]),
            Tab::Connections => hint_spans(&[
                ("Enter", "activate"),
                ("d", "deactivate"),
                ("e", "edit"),
                ("a", "auto"),
                ("x", "delete"),
                ("p", "QR"),
                ("/", "filter"),
                ("?", "help"),
                ("q", "quit"),
            ]),
        }
    };
    frame.render_widget(Paragraph::new(Line::from(hints)), second);
}

fn hint_spans(hints: &[(&str, &str)]) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (index, (key, description)) in hints.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled("  ·  ", Style::new().fg(FAINT)));
        }
        spans.push(Span::styled(
            (*key).to_string(),
            Style::new().fg(ACCENT).bold(),
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            (*description).to_string(),
            Style::new().fg(MUTED),
        ));
    }
    spans
}

fn render_overlay(frame: &mut Frame, overlay: &Overlay, area: Rect) {
    match overlay {
        Overlay::Help { scroll } => render_help(frame, *scroll, area),
        Overlay::Confirm(confirm) => render_confirm(frame, confirm, area),
        Overlay::Prompt(prompt) => render_prompt(frame, prompt, area),
        Overlay::Edit(form) => render_edit(frame, form, area),
        Overlay::Qr(view) => render_qr(frame, view, area),
        Overlay::Details(view) => render_details(frame, view, area),
    }
}

fn render_help(frame: &mut Frame, scroll: u16, area: Rect) {
    let popup = popup_area(area, 72, 22);
    frame.render_widget(Clear, popup);
    let block = panel_block(" Help ").title_bottom(
        Line::from(" j/k scroll · Esc close ")
            .alignment(Alignment::Right)
            .style(Style::new().fg(FAINT)),
    );
    let paragraph = Paragraph::new(help_lines())
        .block(block)
        .scroll((scroll, 0));
    frame.render_widget(paragraph, popup);
}

fn help_lines() -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    push_section(&mut lines, "Navigation");
    lines.extend(help_rows(&[
        ("Tab / ← →", "switch between Wi-Fi and connections"),
        ("j k / ↑ ↓", "move selection"),
        ("g G", "first / last"),
        ("/", "filter the current list"),
        ("r", "refresh NetworkManager state"),
        ("?", "toggle this help"),
        ("q", "quit"),
    ]));
    push_section(&mut lines, "Wi-Fi");
    lines.extend(help_rows(&[
        ("Enter / c", "connect to the selected network"),
        ("d", "disconnect the selected active network"),
        ("p", "show a QR code for sharing"),
        ("s", "rescan access points"),
        ("w", "turn the Wi-Fi radio on or off"),
        ("n", "connect to a hidden network"),
        ("i", "show network details"),
    ]));
    push_section(&mut lines, "Connections");
    lines.extend(help_rows(&[
        ("Enter", "activate the saved profile"),
        ("d", "deactivate the saved profile"),
        ("e", "edit name, autoconnect, and IPv4"),
        ("a", "toggle autoconnect"),
        ("x / Delete", "delete the saved profile"),
        ("p", "share a saved Wi-Fi password as QR"),
        ("i", "show profile details"),
    ]));
    push_section(&mut lines, "Safety");
    lines.extend(help_rows(&[
        ("QR", "contains the Wi-Fi password; close it after use"),
        ("delete", "always asks for confirmation"),
        ("Ctrl+C", "quit even while an operation is running"),
    ]));
    lines
}

fn push_section(lines: &mut Vec<Line<'static>>, title: &str) {
    lines.push(Line::from(Span::styled(
        title.to_string(),
        Style::new().fg(ACCENT).bold(),
    )));
}

fn help_rows(rows: &[(&str, &str)]) -> Vec<Line<'static>> {
    rows.iter()
        .map(|(key, description)| {
            Line::from(vec![
                Span::styled(format!("  {key:<12}"), Style::new().fg(ACCENT)),
                Span::styled((*description).to_string(), Style::new().fg(MUTED)),
            ])
        })
        .collect()
}

fn render_confirm(frame: &mut Frame, confirm: &crate::app::Confirm, area: Rect) {
    let popup = popup_area(area, 62, 9);
    frame.render_widget(Clear, popup);
    let block = panel_block(&format!(" {} ", confirm.title));
    let lines = vec![
        Line::from(""),
        Line::from(Span::raw(confirm.body.clone())),
        Line::from(""),
        Line::from(""),
        Line::from(vec![
            Span::styled(" y ", Style::new().fg(Color::Black).bg(OK).bold()),
            Span::raw(" yes    "),
            Span::styled(" n ", Style::new().fg(Color::Black).bg(MUTED).bold()),
            Span::raw(" no"),
        ])
        .alignment(Alignment::Center),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn render_prompt(frame: &mut Frame, prompt: &Prompt, area: Rect) {
    let popup = popup_area(area, 64, 10);
    frame.render_widget(Clear, popup);
    let block = panel_block(&format!(" {} ", prompt.title));
    let mut lines = vec![
        Line::from(Span::styled(
            prompt.label.clone(),
            Style::new().fg(MUTED).bold(),
        )),
        Line::from(input_spans(&prompt.input, true)),
        Line::from(""),
    ];
    if let Some(error) = &prompt.error {
        lines.push(Line::from(Span::styled(
            error.clone(),
            Style::new().fg(ERR),
        )));
    } else if let Some(help) = &prompt.help {
        lines.push(Line::from(Span::styled(
            help.clone(),
            Style::new().fg(FAINT),
        )));
    }
    lines.push(Line::from(""));
    lines.push(
        Line::from(vec![
            Span::styled("Enter", Style::new().fg(ACCENT).bold()),
            Span::styled(" confirm  ·  ", Style::new().fg(FAINT)),
            Span::styled("Esc", Style::new().fg(ACCENT).bold()),
            Span::styled(" cancel", Style::new().fg(FAINT)),
        ])
        .alignment(Alignment::Center),
    );
    frame.render_widget(Paragraph::new(lines).block(block), popup);
}

fn render_edit(frame: &mut Frame, form: &EditForm, area: Rect) {
    let popup = popup_area(area, 76, 19);
    frame.render_widget(Clear, popup);
    let block = panel_block(" Edit connection ").title_bottom(
        Line::from(" Tab next · Space toggle · Enter apply · Esc cancel ")
            .alignment(Alignment::Right)
            .style(Style::new().fg(FAINT)),
    );

    let mut lines = Vec::new();
    lines.push(edit_field_line(
        form,
        EditForm::NAME,
        "Name",
        &form.name,
        false,
    ));
    lines.push(edit_toggle_line(
        form,
        EditForm::AUTOCONNECT,
        "Autoconnect",
        if form.autoconnect { "yes" } else { "no" },
    ));
    lines.push(edit_choice_line(
        form,
        EditForm::METHOD,
        "IPv4 method",
        form.method.label(),
    ));
    let manual = form.method == Ipv4Method::Manual;
    lines.push(edit_field_line(
        form,
        EditForm::ADDRESSES,
        "Addresses",
        &form.addresses,
        !manual,
    ));
    lines.push(edit_field_line(
        form,
        EditForm::GATEWAY,
        "Gateway",
        &form.gateway,
        !manual,
    ));
    lines.push(edit_field_line(
        form,
        EditForm::DNS,
        "DNS",
        &form.dns,
        !manual,
    ));
    lines.push(Line::from(""));
    if let Some(error) = &form.error {
        lines.push(Line::from(Span::styled(
            error.clone(),
            Style::new().fg(ERR),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            if manual {
                "Addresses use CIDR notation, e.g. 192.168.1.10/24"
            } else {
                "Static address fields are ignored unless the method is Manual."
            },
            Style::new().fg(FAINT),
        )));
    }

    frame.render_widget(Paragraph::new(lines).block(block), popup);
}

fn edit_field_line(
    form: &EditForm,
    index: usize,
    label: &str,
    input: &TextInput,
    dimmed: bool,
) -> Line<'static> {
    let focused = form.focus == index;
    let mut spans = vec![
        Span::styled(
            if focused { "▶ " } else { "  " },
            Style::new().fg(if focused { ACCENT } else { FAINT }),
        ),
        Span::styled(
            format!("{label:<12}"),
            Style::new().fg(if dimmed { FAINT } else { MUTED }).bold(),
        ),
    ];
    if focused {
        spans.extend(input_spans(input, true));
    } else if input.is_empty() {
        spans.push(Span::styled("—", Style::new().fg(FAINT)));
    } else {
        spans.push(Span::styled(
            input.display_value(),
            Style::new().fg(if dimmed { FAINT } else { Color::White }),
        ));
    }
    Line::from(spans)
}

fn edit_toggle_line(form: &EditForm, index: usize, label: &str, value: &str) -> Line<'static> {
    let focused = form.focus == index;
    Line::from(vec![
        Span::styled(
            if focused { "▶ " } else { "  " },
            Style::new().fg(if focused { ACCENT } else { FAINT }),
        ),
        Span::styled(format!("{label:<12}"), Style::new().fg(MUTED).bold()),
        Span::styled(
            format!(" {value} "),
            if focused {
                Style::new().fg(Color::Black).bg(ACCENT).bold()
            } else {
                Style::new().fg(OK)
            },
        ),
    ])
}

fn edit_choice_line(form: &EditForm, index: usize, label: &str, value: &str) -> Line<'static> {
    let focused = form.focus == index;
    Line::from(vec![
        Span::styled(
            if focused { "▶ " } else { "  " },
            Style::new().fg(if focused { ACCENT } else { FAINT }),
        ),
        Span::styled(format!("{label:<12}"), Style::new().fg(MUTED).bold()),
        Span::styled(
            format!(" {value} "),
            if focused {
                Style::new().fg(Color::Black).bg(ACCENT).bold()
            } else {
                Style::new().fg(ACCENT)
            },
        ),
        if focused {
            Span::styled("  ← →", Style::new().fg(FAINT))
        } else {
            Span::raw("")
        },
    ])
}

fn render_details(frame: &mut Frame, view: &DetailsView, area: Rect) {
    let width = ((area.width as u32 * 80 / 100) as u16)
        .max(50)
        .min(area.width);
    let height = ((area.height as u32 * 80 / 100) as u16)
        .max(12)
        .min(area.height);
    let popup = popup_area(area, width, height);
    frame.render_widget(Clear, popup);
    let block = panel_block(&format!(" {} ", view.title)).title_bottom(
        Line::from(" j/k scroll · Esc close ")
            .alignment(Alignment::Right)
            .style(Style::new().fg(FAINT)),
    );
    let lines: Vec<Line> = view
        .lines
        .iter()
        .map(|(label, value)| {
            Line::from(vec![
                Span::styled(format!("{label:<16}"), Style::new().fg(MUTED)),
                Span::raw(value.clone()),
            ])
        })
        .collect();
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((view.scroll, 0));
    frame.render_widget(paragraph, popup);
}

fn render_qr(frame: &mut Frame, view: &crate::qr::QrView, area: Rect) {
    frame.render_widget(Clear, area);
    frame.render_widget(Block::new().style(Style::new().bg(Color::Black)), area);

    let Some(layout) = view.layout(area.width, area.height) else {
        let (width, height) = view.minimum_size();
        let lines = vec![
            Line::from(Span::styled(
                "Terminal too small for this QR code",
                Style::new().fg(WARN).bold(),
            )),
            Line::from(""),
            Line::from(format!(
                "Need at least {width}×{height} cells (now {}×{}).",
                area.width, area.height
            )),
            Line::from("Resize the terminal, or press Esc and use a larger window."),
            Line::from(""),
            Line::from(Span::styled("Esc close", Style::new().fg(MUTED))),
        ];
        frame.render_widget(
            Paragraph::new(lines).alignment(Alignment::Center).block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(Style::new().fg(WARN))
                    .title(" Share Wi-Fi "),
            ),
            area,
        );
        return;
    };

    let title_area = Rect {
        y: area.y,
        height: 1,
        ..area
    };
    let title = if layout.footer {
        Line::from(vec![
            Span::styled(
                format!(" Share Wi-Fi · {} ", clip(&view.ssid, 32)),
                Style::new().fg(ACCENT).bold(),
            ),
            Span::styled(
                format!("{} · scan with your phone", view.security_label),
                Style::new().fg(MUTED),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                " ⚠ QR contains the Wi-Fi password ",
                Style::new().fg(WARN).bold(),
            ),
            Span::styled(
                format!("· {} · Esc close", clip(&view.ssid, 16)),
                Style::new().fg(MUTED),
            ),
        ])
    };
    frame.render_widget(
        Paragraph::new(title)
            .alignment(Alignment::Center)
            .style(Style::new().bg(Color::Black)),
        title_area,
    );

    let qr_width = layout.width;
    let qr_height = layout.height;
    let x = area.x + area.width.saturating_sub(qr_width) / 2;
    let top = area.y + 1;
    let bottom = area.y + area.height - if layout.footer { 1 } else { 0 };
    let y = top + bottom.saturating_sub(top).saturating_sub(qr_height) / 2;
    let qr_area = Rect {
        x,
        y,
        width: qr_width,
        height: qr_height,
    };
    let lines: Vec<Line> = view
        .lines(layout.quiet)
        .into_iter()
        .map(Line::from)
        .collect();
    frame.render_widget(
        Paragraph::new(lines).style(Style::new().fg(Color::Black).bg(Color::White)),
        qr_area,
    );

    if layout.footer {
        let footer_area = Rect {
            y: area.y + area.height - 1,
            height: 1,
            ..area
        };
        let footer = Line::from(vec![
            Span::styled("Esc", Style::new().fg(ACCENT).bold()),
            Span::styled(
                " close  ·  the QR code contains the Wi-Fi password",
                Style::new().fg(FAINT),
            ),
        ])
        .alignment(Alignment::Center);
        frame.render_widget(
            Paragraph::new(footer).style(Style::new().bg(Color::Black)),
            footer_area,
        );
    }
}

fn panel_block(title: &str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(Color::DarkGray))
        .title(Line::from(Span::styled(
            title.to_string(),
            Style::new().fg(MUTED).bold(),
        )))
}

fn popup_area(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

fn input_spans(input: &TextInput, focused: bool) -> Vec<Span<'static>> {
    if !focused {
        return vec![Span::styled(
            input.display_value(),
            Style::new().fg(Color::White),
        )];
    }
    let mut spans = vec![Span::raw(input.before_display())];
    let cursor = input.current_display().unwrap_or(' ');
    spans.push(Span::styled(
        cursor.to_string(),
        Style::new().add_modifier(Modifier::REVERSED),
    ));
    spans.push(Span::raw(input.after_display()));
    spans
}

fn signal_bars(signal: u8) -> String {
    let filled = match signal {
        0 => 0,
        1..=24 => 1,
        25..=49 => 2,
        50..=74 => 3,
        _ => 4,
    };
    let levels = ['▂', '▄', '▆', '█'];
    (0..4)
        .map(|index| if index < filled { levels[index] } else { '·' })
        .collect()
}

fn signal_style(signal: u8) -> Style {
    if signal >= 70 {
        Style::new().fg(OK)
    } else if signal >= 40 {
        Style::new().fg(WARN)
    } else {
        Style::new().fg(ERR)
    }
}

fn tab_title(tab: Tab) -> &'static str {
    match tab {
        Tab::Wifi => "Wi-Fi",
        Tab::Connections => "Connections",
    }
}

fn clip(value: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    let mut output = String::new();
    let mut width = 0;
    for character in value.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if width + character_width > max_width {
            if max_width > 1 {
                while width + 1 > max_width {
                    if let Some(last) = output.pop() {
                        width -= UnicodeWidthChar::width(last).unwrap_or(0);
                    } else {
                        break;
                    }
                }
                output.push('…');
            }
            break;
        }
        output.push(character);
        width += character_width;
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::backend::{ActiveLink, ConnectionKind, SavedConnection, Snapshot, WifiNetwork};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::sync::mpsc;

    fn snapshot() -> Snapshot {
        Snapshot {
            wifi_enabled: true,
            connectivity: "full".to_string(),
            hostname: "testhost".to_string(),
            wifi_device: Some("wlan0".to_string()),
            networks: vec![WifiNetwork {
                ssid: "Cafe".to_string(),
                signal: 80,
                security: Security::Wpa,
                security_label: "WPA2".to_string(),
                freq_mhz: 2437,
                rate: "1170 Mbit/s".to_string(),
                bssid: "AA:BB:CC:DD:EE:FF".to_string(),
                active: true,
                saved_uuid: None,
            }],
            connections: vec![SavedConnection {
                uuid: "uuid-Cafe".to_string(),
                name: "Cafe".to_string(),
                kind: ConnectionKind::Wifi,
                device: Some("wlan0".to_string()),
                active: true,
                autoconnect: true,
                ssid: Some("Cafe".to_string()),
                hidden: false,
            }],
            active: Some(ActiveLink {
                connection: "Cafe".to_string(),
                device: "wlan0".to_string(),
                kind: ConnectionKind::Wifi,
                ip4: vec!["192.168.1.42/24".to_string()],
                gateway: Some("192.168.1.1".to_string()),
                dns: vec!["1.1.1.1".to_string()],
            }),
            warnings: Vec::new(),
        }
    }

    fn app() -> App {
        let (tx, _rx) = mpsc::channel();
        let (_backend_tx, backend_rx) = mpsc::channel();
        let mut app = App::new(tx, backend_rx);
        app.apply_snapshot(snapshot());
        app.loaded = true;
        app
    }

    fn screen_size(app: &mut App, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, app)).unwrap();
        let buffer = terminal.backend().buffer();
        let mut text = String::new();
        for row in buffer.content().chunks(buffer.area.width as usize) {
            for cell in row {
                text.push_str(cell.symbol());
            }
            text.push('\n');
        }
        text
    }

    fn screen(app: &mut App) -> String {
        screen_size(app, 100, 30)
    }

    #[test]
    fn wifi_screen_renders_header_and_network() {
        let mut app = app();
        let text = screen(&mut app);
        assert!(text.contains("bnm"));
        assert!(text.contains("Wi-Fi networks"));
        assert!(text.contains("Cafe"));
        assert!(text.contains("WPA2"));
        assert!(text.contains("Enter"));
    }

    #[test]
    fn connections_tab_renders_saved_profile() {
        let mut app = app();
        app.tab = Tab::Connections;
        let text = screen(&mut app);
        assert!(text.contains("Saved connections"));
        assert!(text.contains("Cafe"));
        assert!(text.contains("Autoconnect"));
    }

    #[test]
    fn qr_overlay_renders_a_white_qr_block() {
        let mut app = app();
        let info = crate::backend::ShareInfo {
            ssid: "Cafe".to_string(),
            security: Security::Open,
            security_label: "Open".to_string(),
            password: None,
            hidden: false,
        };
        app.overlay = Some(Overlay::Qr(
            crate::qr::QrView::new(
                crate::qr::wifi_payload(
                    &info.ssid,
                    info.security,
                    info.password.as_deref(),
                    info.hidden,
                )
                .unwrap(),
                info.ssid,
                info.security_label,
            )
            .unwrap(),
        ));
        let text = screen(&mut app);
        assert!(text.contains("Share Wi-Fi"));
        assert!(text.contains('█') || text.contains('▀') || text.contains('▄'));
    }

    #[test]
    fn standard_80x24_terminal_shows_both_panes() {
        let mut app = app();
        let text = screen_size(&mut app, 80, 24);
        assert!(text.contains("Wi-Fi networks"));
        assert!(text.contains("Selected network"));
        assert!(text.contains("Enter"));
        assert!(text.contains("connect"));
    }

    #[test]
    fn small_terminal_has_a_clear_message() {
        let mut app = app();
        let backend = TestBackend::new(40, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, &mut app)).unwrap();
        let buffer = terminal.backend().buffer();
        let text: String = buffer.content().iter().map(|cell| cell.symbol()).collect();
        assert!(text.contains("Terminal too small"));
    }
}
