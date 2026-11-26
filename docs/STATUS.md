# Project Status

Last reviewed: 2026-11-26

## Current State

`bnm` is a working NetworkManager TUI. It scans Wi-Fi networks, connects and
disconnects, manages saved profiles, shows active-link details, and renders
Wi-Fi QR codes in the terminal. The backend runs on a worker thread; the UI
remains responsive during scans and connection attempts.

## Verified

- `cargo test` passes 46 default tests covering terse-output escaping,
  newline-bearing names/SSIDs, snapshot composition against a fake `nmcli`,
  security classification, QR payload escaping and layout, input editing, key
  flows with modifier handling, IPv4 DNS validation, load-error rendering, and
  ratatui rendering at normal, 80×24, and small terminal sizes. A 47th test is
  ignored by default because it needs external tools.
- `cargo clippy --all-targets -- -D warnings` passes.
- `cargo fmt --check` passes.
- `bnm --check` reads the live NetworkManager on the development machine and
  reports Wi-Fi state, active link, IP address, network count, and saved
  connection count.
- A pseudo-terminal smoke test against the live NetworkManager exercised the
  initial load, help, Wi-Fi QR view, network details, disconnect confirmation,
  connections tab, saved-profile QR, profile details, edit form, delete
  confirmation, hidden-network prompt, and quit. The test used read-only flows
  and canceled every confirmation.
- QR rendering was verified structurally (module rows, quiet zone, half-block
  output, minimum size) and visually in the pseudo-terminal.
- The ignored `qr_matrix_decodes_to_the_wifi_payload` test writes the generated
  matrix as a scaled PNG and decodes it with `zbarimg`; it returned the exact
  `WIFI:T:WPA;S:Cafe\;Guest;P:pa\:ss;;` payload.

## Partial or Missing

- Creating or editing WPA-Enterprise (802.1X) connections is not implemented.
  Saved enterprise profiles can be activated, but QR sharing is rejected.
- WEP QR sharing and connection are implemented but not exercised against a
  live WEP access point; some `nmcli` versions may require an explicit WEP key
  type.
- The edit form covers name, autoconnect, IPv4 method, addresses, gateway, and
  IPv4 DNS (plain addresses, no prefix required). Static fields are always sent
  for manual profiles and cleared when switching away from manual. IPv6, routes,
  Wi-Fi security changes, VPNs, bridges, and hotspot creation are not editable.
- If activating a saved profile fails because its secret is missing, bnm prompts
  and retries as a new connection, now honoring the saved profile's hidden flag.
  This fallback has not been tested against a profile with deliberately missing
  credentials.
- There is no timeout wrapper around individual `nmcli` calls. A hung
  NetworkManager/D-Bus call would leave the worker busy until it returns.
- There is no integrated PolicyKit agent; operations denied by PolicyKit show an
  error and must be authorized elsewhere.

## Unverified

- Scanning a bnm QR code with an actual phone camera. The payload format and
  matrix are covered by tests (including a `zbarimg` round trip), but no
  physical phone scan was performed.
- Live connect, disconnect, radio toggle, profile edit, and delete mutations.
  These were deliberately avoided during verification to avoid disrupting the
  development machine's network.
- Behavior on NetworkManager versions other than 1.58 and on non-UTF-8 SSIDs.
- WPA3-only (`sae`) QR interoperability on older scanners; bnm emits the
  standard `T:WPA` type.
- Light terminal themes after removing forced panel backgrounds; explicit
  colors are used for selected rows, QR codes, and status badges.
