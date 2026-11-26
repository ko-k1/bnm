# ADR-0001: Use nmcli as the NetworkManager backend

- Status: Accepted
- Date: 2026-11-26

## Context

`bnm` needs to list devices, scan Wi-Fi, activate and deactivate profiles, edit
basic IPv4 settings, delete profiles, and read saved Wi-Fi secrets for QR
sharing. The two realistic interfaces are:

1. Shell out to `nmcli` and parse terse output.
2. Talk to NetworkManager over D-Bus (for example with `zbus`).

The app should remain a small single binary and work on minimal systems where
NetworkManager is installed but a desktop session is not. The initial version
needs to ship a broad feature set quickly and be testable without a live
NetworkManager.

## Decision

Use `nmcli` as the only NetworkManager interface. Run it from a dedicated worker
thread, parse terse output with escaping-aware helpers, and keep the UI behind
typed request/response messages.

## Consequences

Benefits:

- No additional D-Bus dependency or async runtime.
- Commands map directly to documented `nmcli` operations, making behavior easy
  to audit and reproduce by hand.
- A fake `nmcli` script can exercise snapshot parsing and command composition in
  tests.
- The binary stays small and works over SSH and in minimal environments.

Costs and risks:

- Passwords for new connections are passed as `nmcli` command-line arguments and
  are briefly visible in local process listings.
- Terse output needs careful parsing, especially for names and SSIDs containing
  colons, backslashes, or newlines.
- Some operations are awkward or unavailable without richer D-Bus settings
  objects (enterprise authentication, complex IPv4/IPv6 configuration).
- Error messages are whatever `nmcli` returns, normalized only heuristically.

Follow-up work may introduce a `zbus` backend for operations where argument
exposure or parsing becomes a material limitation. The UI/backend request
boundary is intended to make that replacement local to `src/backend.rs`.

## Alternatives

- **D-Bus via `zbus`:** more robust data model and no secret in `argv`, but a
  larger dependency and considerably more code for settings dictionaries,
  activation paths, and secret agents. Deferred until the feature set requires
  it.
- **Direct netlink/wpa_supplicant:** bypasses NetworkManager and duplicates
  policy, secret storage, and profile management. Rejected.
- **Embed `nmtui`:** not a library and does not expose the data or QR workflow
  needed by bnm. Rejected.
