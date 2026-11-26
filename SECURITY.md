# Security Policy

## Reporting a Vulnerability

Do not disclose suspected vulnerabilities in public issues or discussions.

Report privately through the repository host's private vulnerability reporting
feature, if enabled, or through this project's designated private contact:

- Contact: <add a monitored private security contact>

## Scope and Response

- Affected versions: 0.1.x
- Expected acknowledgement: <set a realistic target>
- Disclosure process: <describe coordination and release policy>

Replace every placeholder before inviting security reports. This file describes
a reporting process; it does not replace the project's technical security model.

## Technical Security Model

- bnm uses NetworkManager as the credential store. It does not create config
  files, caches, or logs containing Wi-Fi passwords.
- QR sharing reads an existing saved secret with
  `nmcli --show-secrets`; the secret is held in memory only long enough to build
  the payload and render the matrix.
- The QR view intentionally exposes the Wi-Fi password on screen. The footer
  warns about this; users should close the view and avoid screen sharing while
  it is open.
- Connecting to a new network passes the password to `nmcli` as an argument.
  Local users may be able to observe it in the process list while the command
  runs. This is an `nmcli` interface limitation.
- bnm does not implement a PolicyKit agent. Operations are authorized by the
  user's existing NetworkManager/PolicyKit configuration.
- NetworkManager output is treated as data. bnm does not execute content from
  SSIDs, profile names, or error messages.
