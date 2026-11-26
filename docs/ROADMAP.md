# Roadmap

## Direction

Deepen the useful NetworkManager surface without turning bnm into a full
settings editor. Keep the terminal UI fast, keyboard-first, and honest about
what it cannot do.

## Next

- **Edit Wi-Fi security** — change a saved profile's WPA password and key
  management from the edit form, with validation and a clear "this updates the
  saved profile" message.
- **Better saved-secret recovery** — update a saved profile's secret and retry
  activation when NetworkManager reports that the stored secret is missing.
- **Command timeouts** — bound every `nmcli` call and show a recoverable error
  instead of leaving the worker stuck.
- **QR verification** — add a decode test (PNG or terminal matrix to a QR
  decoder) and test with at least one phone camera.

## Later

- Add a new Wi-Fi profile explicitly (SSID, security, password, autoconnect)
  instead of relying on `nmcli device wifi connect`.
- IPv6 editing, routes, Wi-Fi security changes, VPNs, bridges, and hotspot
  creation.
- Hotspot creation and a dedicated status view for Ethernet/VPN links.
- Optional `zbus` backend for operations where `nmcli` argument exposure or
  output parsing is a material limitation.
- Clipboard export of the QR payload or a PNG file, behind an explicit warning.
- Theme selection and a high-contrast mode.

Priorities may change as requirements and evidence change. Move completed work
to [STATUS.md](STATUS.md) rather than keeping a second status ledger here.
