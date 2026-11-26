# Architecture

## Overview

`bnm` is a single Rust binary with four layers:

1. **Backend** (`src/backend.rs`) — talks to NetworkManager through `nmcli`,
   parses terse output, and produces snapshots, profile details, and QR sharing
   data.
2. **Application state** (`src/app.rs`) — owns the current snapshot, selections,
   filters, overlays, status messages, and key handling. It translates user
   intent into backend requests.
3. **QR** (`src/qr.rs`) — builds the Wi-Fi payload and encodes/renders a QR
   matrix.
4. **UI** (`src/ui.rs`, `src/input.rs`) — renders ratatui widgets and handles
   single-line text editing.

`src/main.rs` wires these together, initializes the terminal, runs the event
loop, and offers `--check` diagnostics.

## Components and Ownership

| Component | Owns |
| --- | --- |
| `backend::Nmcli` | Every `nmcli` invocation, terse parsing, SSID cache, and NetworkManager error translation |
| `backend::spawn` | The worker thread and request/response channels |
| `app::App` | Selection, filtering, overlays, confirmations, prompts, status, and refresh policy |
| `qr` | `WIFI:` payload escaping, QR encoding, quiet-zone/layout selection, half-block rows |
| `ui` | Layout, colors, tables, panels, popups, and visible key hints |
| `input::TextInput` | Cursor movement, masking, editing shortcuts |

## Boundaries

- The UI never invokes `nmcli`. It sends a `Request` and receives a `Response`.
- The backend never renders and never reads terminal input. It runs on a worker
  thread and has no direct access to `App`.
- `App` does not parse `nmcli` output. It consumes typed `Snapshot`,
  `DetailsData`, and `ShareInfo` values.
- `qr` depends only on the small `Security` enum and returns strings/layout
  decisions. It does not know about NetworkManager commands.
- No component writes credentials to disk. NetworkManager remains the owner of
  saved secrets.

## Important Flows

### Startup and refresh

1. `main` starts the backend worker and calls `App::start`.
2. `App` sends `Request::Refresh` and sets `refreshing`.
3. The worker runs `Nmcli::snapshot`, which collects general status, devices,
   saved connections, cached SSIDs, Wi-Fi access points, and active-link IPv4
   details.
4. `Response::Snapshot` replaces the UI snapshot while preserving the selected
   SSID/profile UUID. Auto-refresh repeats every five seconds when the UI is idle.

### Connect

- A visible open network sends `Request::Connect` with the BSSID when available.
- A saved profile sends `Request::Activate` so NetworkManager uses its stored
  secret. If activation reports a missing password, the UI prompts and retries
  with the password.
- An unsaved secured network opens a masked password prompt, then sends
  `Request::Connect`.
- A hidden network asks for SSID then password and sends `Request::Connect` with
  `hidden=true`.

### QR sharing

1. A saved profile sends `Request::Share`.
2. The worker reads SSID, key management, hidden flag, and (for WPA/WEP) the
   secret with `nmcli --show-secrets`.
3. `qr::wifi_payload` escapes the fields; `QrView` encodes the matrix.
4. The UI chooses a quiet zone and renders the matrix with half blocks on a
   white background.

### Destructive actions

Delete, deactivate, disconnect, and turning off Wi-Fi while connected create a
`Confirm` overlay. Only `y`/`Enter` sends the corresponding backend request.

## Constraints

- `nmcli` is the only external interface. This keeps the dependency surface
  small and works over SSH, but it means secrets used for a new connection are
  passed as command-line arguments.
- Terse `nmcli` output escapes colons and backslashes but not newlines. The
  parser reconstructs connection/SSID records using UUID/`IN-USE` anchors and
  joins continuation lines. Pathological values can still be ambiguous.
- A terminal QR code needs enough rows. The UI prefers a four-module quiet zone,
  falls back to two or one, and refuses to draw when even the minimum does not
  fit.
- NetworkManager commands are synchronous inside the worker. There is no cancel
  button; quitting the UI is always allowed.
- The app does not implement a PolicyKit agent. Denied operations surface as
  errors.

## Related Decisions

- [ADR 0001: Use nmcli as the backend](adr/0001-nmcli-backend.md)
