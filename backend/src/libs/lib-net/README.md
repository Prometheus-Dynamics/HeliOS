# lib-net

`lib-net` exposes helpers for configuring network interfaces and host
identity. It wraps `rtnetlink` so the rest of the stack can manage Linux link
state without shelling out to `ip`.

## Features

- `interface::get_interfaces()` enumerates ethernet/VLAN/bond interfaces and
  now returns rich metadata including:
  - per-interface IPv4/IPv6 assignments (`ipv4`/`ipv6`)
  - static vs. DHCP mode for both address families
  - discovered gateways, VLAN information and bonding mode
  - DNS configuration via the new `DnsConfig` helper
- `set_interface()` applies static and dynamic IPv4/IPv6 settings, installs
  gateways using rtnetlink APIs, and safely manages `dhclient` when switching to
  DHCP. DNS updates are written to `/etc/resolv.conf` (configurable via
  `HELIOS_DNS_CONFIG_PATH`).
- Convenience helpers for reading/setting the device hostname.
- mDNS and subnet-based discovery utilities for locating peers.

All public structs derive `utoipa::ToSchema`, making it easy to include the
network configuration contract in OpenAPI documents.

## Testing

The crate ships unit tests that exercise netlink parsing with synthetic
`LinkMessage` fixtures and verify DNS updates using a temporary resolv.conf.
Run the tests with:

```bash
cargo test -p lib-net
```
