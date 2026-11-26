mod app;
mod backend;
mod input;
mod qr;
mod ui;

use std::io;
use std::time::Duration;

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyEventKind};

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        print_help();
        return Ok(());
    }
    if args.iter().any(|arg| arg == "-V" || arg == "--version") {
        println!("bnm {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--check") {
        return check();
    }
    if let Some(unknown) = args.first() {
        eprintln!("bnm: unknown argument: {unknown}");
        eprintln!("Try 'bnm --help'.");
        std::process::exit(2);
    }

    let backend = backend::Nmcli::new();
    let (tx, rx) = backend::spawn(backend);
    let mut app = app::App::new(tx, rx);
    let mut terminal = ratatui::init();
    app.start();
    let result = run(&mut terminal, &mut app);
    ratatui::restore();
    result
}

fn run(terminal: &mut DefaultTerminal, app: &mut app::App) -> io::Result<()> {
    while !app.should_quit {
        app.tick();
        terminal.draw(|frame| ui::render(frame, app))?;

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key)
                    if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
                {
                    app.handle_key(key);
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn check() -> io::Result<()> {
    let mut backend = backend::Nmcli::new();
    match backend.snapshot() {
        Ok(snapshot) => {
            println!(
                "Wi-Fi: {} ({})",
                if snapshot.wifi_enabled {
                    "enabled"
                } else {
                    "disabled"
                },
                snapshot.wifi_device.as_deref().unwrap_or("no device")
            );
            println!(
                "Host: {}  Connectivity: {}",
                snapshot.hostname, snapshot.connectivity
            );
            if let Some(active) = &snapshot.active {
                println!(
                    "Active: {} on {}  IP: {}",
                    active.connection,
                    active.device,
                    active.primary_ip().unwrap_or("—")
                );
            } else {
                println!("Active: none");
            }
            println!(
                "Networks: {}  Saved connections: {}",
                snapshot.networks.len(),
                snapshot.connections.len()
            );
            for warning in snapshot.warnings {
                println!("Warning: {warning}");
            }
            Ok(())
        }
        Err(error) => {
            eprintln!("bnm: {error}");
            std::process::exit(1);
        }
    }
}

fn print_help() {
    println!(
        "bnm {} — a better NetworkManager TUI

USAGE:
    bnm             Open the interactive network manager
    bnm --check     Print NetworkManager state without opening the TUI
    bnm --help      Show this help
    bnm --version   Show the version

REQUIREMENTS:
    NetworkManager and its nmcli command. Set BNM_NMCLI to use another binary.

KEYS:
    Tab / ← →      switch between Wi-Fi and Connections
    j k / ↑ ↓      move selection
    Enter          connect or activate
    d              disconnect or deactivate
    p              show a Wi-Fi QR code
    s              rescan Wi-Fi networks
    w              toggle the Wi-Fi radio
    n              connect to a hidden network
    e              edit a saved connection
    a              toggle autoconnect
    x / Delete     delete a saved connection
    i              show details
    /              filter the current list
    r              refresh
    ?              help
    q              quit
    Ctrl+C         quit immediately

QR codes contain the saved Wi-Fi password. Close the QR view after use.",
        env!("CARGO_PKG_VERSION")
    );
}
