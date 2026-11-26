# bnm

A keyboard-first NetworkManager TUI for Linux, built with Rust and
[ratatui](https://ratatui.rs). It covers the everyday `nmtui` workflows and adds
a terminal Wi-Fi QR code for sharing a network with a phone.

```
 bnm  better network manager                    ● Home  192.168.1.42/24
 koki-laptop · wlan0 · Wi-Fi on · full                     updated just now
 1 Wi-Fi   2 Connections
╭ Wi-Fi networks (12) ─────────────────────────────────╮ ╭ Selected network ───╮
│  Signal   SSID                    Security   Band    │ │ Home                │
│● ▂▄▆█ 100 Home                    WPA2/3     5G      │ │ ● Connected         │
│  ▂▄▆·  72 Cafe                    WPA2       2.4G    │ │ Security   WPA2/3   │
│  ▂▄··  41 Guest                   Open       2.4G    │ │ Signal     100%     │
│  ...                                                 │ │ ...                 │
╰──────────────────────────────────────────────────────╯ ╰─────────────────────╯
Enter connect · p QR · d disconnect · s scan · w radio · n hidden · ? help · q quit
```

## Features

- **Wi-Fi scan and connect** — signal, security, band, active and saved markers;
  open, WPA/WPA2/WPA3 personal, and legacy WEP networks.
- **Share with QR** — render a standard `WIFI:` QR code in the terminal. Saved
  passwords are read from NetworkManager; unsaved networks prompt for one.
- **Everyday NetworkManager control** — disconnect, rescan, enable/disable the
  radio, and join hidden networks.
- **Saved profiles** — activate, deactivate, edit name/autoconnect/IPv4, toggle
  autoconnect, view details, and delete with confirmation.
- **Live status** — active link, IP address, gateway, DNS, connectivity, and
  NetworkManager warnings.
- **Fast, non-blocking UI** — `nmcli` work runs on a worker thread; the UI keeps
  rendering and accepts input during scans and connection attempts.
- **No config or state files** — NetworkManager is the single source of truth.
  bnm does not write credentials to disk.

## Requirements

- Linux with NetworkManager and `nmcli` on `PATH`.
- Rust 1.88 or newer to build.
- A UTF-8 terminal with Unicode block characters for the QR view.
- Permission to manage connections through NetworkManager/PolicyKit. Reading a
  saved Wi-Fi password may require additional authorization.

Tested against NetworkManager 1.58 / `nmcli` 1.58.

## Build and install

```sh
cargo build --release
install -Dm755 target/release/bnm ~/.local/bin/bnm
```

Or run it directly:

```sh
cargo run --release
```

Diagnose the backend without opening the TUI:

```sh
bnm --check
```

Use a non-standard `nmcli` path:

```sh
BNM_NMCLI=/usr/local/bin/nmcli bnm
```

## Keys

| Key | Action |
| --- | --- |
| `Tab`, `←`, `→` | Switch between Wi-Fi and saved connections |
| `j`/`k`, `↑`/`↓` | Move selection |
| `g`/`G` | First / last item |
| `Enter` or `c` | Connect or activate |
| `d` | Disconnect or deactivate |
| `p` | Show a Wi-Fi QR code |
| `s` | Rescan Wi-Fi (Wi-Fi tab) |
| `w` | Toggle the Wi-Fi radio |
| `n` | Connect to a hidden network (Wi-Fi tab) |
| `e` | Edit a saved connection (Connections tab) |
| `a` | Toggle autoconnect (Connections tab) |
| `x` or `Delete` | Delete a saved connection (Connections tab) |
| `i` | Show details |
| `/` | Filter the current list |
| `r` | Refresh |
| `?` | Help |
| `q` | Quit |
| `Ctrl+C` | Quit immediately |

## QR sharing

Select a network and press `p`.

- **Saved profile** — bnm asks NetworkManager for the saved PSK (WPA) or WEP key.
- **Unsaved network** — bnm prompts for the password and does not save it.
- **Open network** — the QR code uses `T:nopass`.

The QR payload uses the common format understood by Android and iOS camera apps:

```text
WIFI:T:WPA;S:NetworkName;P:password;;
```

Special characters in the SSID and password are escaped. Hidden profiles set
`H:true`. The terminal view prefers the standard four-module quiet zone and
falls back to two or one when the window is short; if even the minimum does not
fit, it tells you the required size instead of drawing an unscannable code.

The QR code contains the Wi-Fi password. Close the view when you are done and
avoid screen sharing while it is open.

## How it works

- `nmcli` terse output is parsed with escaping-aware helpers. Connection names
  and SSIDs containing newlines are reconstructed rather than silently
  misaligned.
- A worker thread executes NetworkManager commands sequentially and sends
  snapshots/actions back to the UI over channels.
- Saved profile SSIDs are cached across refreshes; the cache is discarded when
  profiles disappear.
- QR matrices are generated in-process with the `qrcode` crate and rendered with
  Unicode half blocks.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the component boundaries.

## Limitations

- Creating or editing WPA-Enterprise (802.1X) connections is not supported.
  Existing enterprise profiles can still be activated, but their passwords
  cannot be encoded in a password-only QR code.
- WEP is legacy; some `nmcli` versions need an explicit key type that bnm does
  not prompt for.
- The edit form covers name, autoconnect, IPv4 method/addresses/gateway/DNS.
  IPv6, routes, Wi-Fi security changes, VPNs, and hotspot creation are not
  editable yet.
- When connecting to a new network, the password is passed to `nmcli` as a
  command-line argument, so it is visible to local process listings while the
  command runs. This is an `nmcli` interface limitation; bnm does not log it or
  write it to disk.
- If NetworkManager or PolicyKit denies an operation, bnm shows the error but
  does not launch a separate authentication agent.
- bnm targets Linux/NetworkManager; it does not implement its own networking
  stack.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo run -- --check
```

The test suite covers terse-output parsing, escaped and newline-bearing values,
snapshot composition against a fake `nmcli`, security classification, QR payload
escaping and layout, input editing, key flows, and ratatui screen rendering.

An ignored QR round-trip test requires ImageMagick and `zbarimg`:

```sh
cargo test qr::tests::qr_matrix_decodes_to_the_wifi_payload -- --ignored
```

## Documentation

- [Vision](docs/VISION.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Status](docs/STATUS.md)
- [Roadmap](docs/ROADMAP.md)
- [Changelog](CHANGELOG.md)
- [Security](SECURITY.md)
- [Contributing](CONTRIBUTING.md)

## License

MIT. See [LICENSE](LICENSE).
