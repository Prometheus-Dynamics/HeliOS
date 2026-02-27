# Assets Layout

Non-TOML content used by the builder lives here.

- `buildroot/` – Buildroot integration assets (packages, board scripts, overlays, patches, config files).
- `services/` – systemd units, drop-ins, helper scripts, and service-side config assets.
- `overlays/` – rootfs overlay trees copied into stage/rootfs.
- `templates/` – pipeline/template JSON assets.
- `pipelines/` – pipeline template packs.
- `libcamera/` – camera tuning/IPA JSON assets.
- `sdk/` – optional offline SDK/cache content.
