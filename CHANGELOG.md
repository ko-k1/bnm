# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Record user-visible behavior changes here; omit internal refactors, chores, and
documentation fixes that users cannot observe. Add new entries under
`Unreleased`, describe the outcome rather than the implementation, and move
entries into a dated version section when a release is tagged.

## [Unreleased]

### Added

- Saved-profile IPv4 DNS editing accepts plain addresses (e.g. `1.1.1.1`)
  without a prefix; only addresses require CIDR notation.
- Clearing the gateway or DNS field in the edit form now clears the value in the
  saved profile, and switching away from Manual clears all static fields.

### Fixed

- Manual IPv4 edits rejected every DNS server because DNS was validated like a
  CIDR address; DNS is now validated as a plain IPv4 address.
- Terse `nmcli` key/value parsing dropped newline-bearing values, truncating
  profile names and SSIDs that contain newlines; continuations are now joined.
- Clearing gateway or DNS in the edit form left the old value in the profile;
  both are now always sent (empty clears them).
- Modifier keys (Ctrl/Alt) triggered shortcuts and confirmations; only
  unmodified keys now trigger actions, with Ctrl+C the sole global control
  shortcut.
- A failed initial NetworkManager snapshot rendered a valid empty state instead
  of an error; load failures now show "NetworkManager unavailable" with a retry
  hint, and dead workers are retried on the auto-refresh interval.
- Scan throttling was reported as a successful scan; a warning is now shown and
  cached results are displayed.
- Overlays were invisible but still actionable below the minimum terminal size;
  overlays now render even when the terminal is too small.
- The active-link header hid Ethernet when Wi-Fi was off; the active link is
  now shown regardless of radio state.
- Retrying a saved hidden network after a missing-secret failure now honors the
  saved profile's hidden flag.
- Error classification no longer maps generic "not found" messages (e.g. a
  missing device) to "Network not found; try rescanning".

- A keyboard-driven NetworkManager TUI with separate Wi-Fi and saved-connection
  views, live link status, filtering, help, and confirmation dialogs.
- Wi-Fi scanning, connect/disconnect, radio toggle, rescanning, and hidden-network
  joining.
- Terminal Wi-Fi QR sharing for open, WEP, and WPA personal networks, including
  saved-password lookup and masked prompts for unsaved networks.
- Saved-profile activation, deactivation, deletion, autoconnect toggle, details,
  and editing of name, autoconnect, IPv4 method, addresses, gateway, and DNS.
- `bnm --check` for printing NetworkManager state without opening the TUI.
- Background `nmcli` worker so scans and connection attempts do not block input
  or rendering.

### Security

- QR views explicitly warn that the displayed code contains the Wi-Fi password;
  bnm does not write credentials to disk or log them.
