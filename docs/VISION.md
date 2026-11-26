# Vision

## Purpose

`bnm` makes everyday NetworkManager control fast and pleasant from a terminal,
without hiding the details that matter. It should be the tool you reach for when
`nmtui` feels too sparse and a desktop network applet feels too heavy.

## Problem

Linux users already have NetworkManager, but its terminal interface is limited
in presentation and feedback. Sharing a Wi-Fi password usually means running
`nmcli` by hand, reading a secret, and finding a separate QR generator. Existing
GUIs work, but they pull in a desktop session and are awkward over SSH or in a
minimal window manager.

## Goals

- Connect, disconnect, scan, and manage saved Wi-Fi profiles from one keyboard
  driven screen.
- Share the selected or active Wi-Fi network as a scannable QR code.
- Make the current network state obvious: active link, IP, gateway, DNS, signal,
  security, and whether a profile is saved.
- Keep the interface responsive while NetworkManager scans or activates a
  connection.
- Use NetworkManager as the source of truth; never keep a second configuration
  database or write credentials to disk.
- Fail clearly and safely, with confirmations for destructive actions.

## Non-goals

- Replacing NetworkManager or implementing a networking stack.
- Full parity with every `nmcli` setting, VPN editor, or enterprise 802.1X
  workflow.
- Managing non-Linux systems.
- Acting as a password manager or storing Wi-Fi secrets outside
  NetworkManager.
- Requiring a daemon, a desktop environment, or a configuration file.

## Principles

- **Keyboard first.** Every primary action has a single-key shortcut and a
  visible hint.
- **NetworkManager owns state.** bnm displays and edits NetworkManager profiles;
  it does not invent a parallel model.
- **Never block the UI.** Slow commands run on a worker thread and report back.
- **Confirm irreversible actions.** Deletion and disconnecting an active link
  require an explicit choice.
- **Secrets stay local.** bnm does not log, persist, or transmit credentials.
  The QR view is an intentional, temporary exposure.
- **Degrade honestly.** Partial data and backend errors are shown as warnings,
  not hidden.

## Success Criteria

- A user can find, connect to, and disconnect from a Wi-Fi network without
  leaving the terminal.
- A phone can scan the displayed QR code and join the network.
- The active link and its IPv4 details are visible at a glance.
- Saved profiles can be activated, edited, and deleted with clear confirmation.
- The UI remains responsive during scans and connection attempts.
- No credential is written to a file or emitted in logs by bnm.
