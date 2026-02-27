---
title: Networking
---

Networking is where you set the device identity (hostname/team number), configure network interfaces (DHCP/static), and configure NetworkTables (NT4).

## Identity (Hostname + Team)

- **Hostname** is required. It is used by the UI, APIs, and as the default NT4 publish prefix.
- **Team number** is optional. If set, HeliOS derives an NT4 roboRIO host as `10.TE.AM.2`.

Examples:

- `6390` -> `10.63.90.2`
- `6364` -> `10.63.64.2`
- `9180` -> `10.91.80.2`
- `10527` -> `10.105.27.2`

If your network supports mDNS, the Web UI is often reachable at:

- `http://<hostname>.local:5800/` (example: `http://helios.local:5800/`)

If mDNS does not work, use the device IP address (from your router/DHCP leases), or scan your team subnet.

Example scan (replace the subnet with your team number):

```bash
# Example team 6390 (10.63.90.0/24)
nmap -p 5800 --open 10.63.90.0/24
```

## Interfaces (DHCP vs Static)

In the UI, pick an interface from the **Interface** dropdown, then choose:

- **Dynamic (DHCP)**: requests a DHCP lease.
- **Static**: applies a manual IPv4 override until you clear/change it.

Notes:

- When **Static** is selected, the UI requires:
  - **Address**
  - **Prefix** (`1-32`)
  - **Gateway**
- When **Dynamic (DHCP)** is selected and the device has a lease, the UI shows the current lease (address/prefix and optional gateway) so you can confirm what the device actually received.
- Buttons:
  - **Request lease** (DHCP)
  - **Apply static** (Static)

## NetworkTables (NT4)

HeliOS can connect to an NT4 server (commonly the roboRIO) and publish/subscribe topics.

### Connection Settings

- **Enable NT4 publishing**: toggles publishing from the device to the NT4 server.
- **Enable NT4 subscriptions**: toggles subscription reads (required for the Explorer).
- **Server host override**: if empty, the UI uses the derived roboRIO host from your team number (when available).
- **Server port**: defaults to `5810`.

### Publish Prefix + Topics

- **Publish prefix** is derived from hostname:
  - If hostname is set: `/<hostname>`
  - Otherwise: `/helios`
- The UI shows the topics it publishes under the prefix, including:
  - `<prefix>/info/hostname`
  - `<prefix>/info/api_url`
  - `<prefix>/info/ui_url`
  - `<prefix>/info/streams`

### Public URLs

- **Public API URL (optional)**: a URL you want published for other systems to call the device API.
- **Public UI URL**: published automatically by the device (the UI describes this as the device IP plus the API port).

### NT4 Explorer

The Explorer button is disabled unless:

- **Enable NT4 subscriptions** is on, and
- the UI has an effective NT4 host (team number derived host or a server host override).

When opened, the Explorer periodically scans topics and lets you read individual topic values.
