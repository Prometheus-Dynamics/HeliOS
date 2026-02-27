use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::archive::{ensure_dir, try_glob_copy, write_bytes, write_str};
use crate::cmd::{run_cmd, CmdResult, CmdSpec};
use crate::error::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Meta {
    pub id: String,
    pub created_utc: DateTime<Utc>,
    pub host: Option<String>,
    pub trigger: String,
    pub unit: Option<String>,
    pub tag: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub bundle_dir: String,
    pub archive: Option<String>,
    pub meta: Meta,
    pub commands: Vec<CmdResult>,
}

#[derive(Debug, Clone)]
pub struct SnapshotConfig {
    pub base_dir: PathBuf,
    pub run_dir: PathBuf,
    pub tar: bool,
    pub max_cmd_bytes: usize,
    pub cmd_timeout: Duration,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            base_dir: PathBuf::from("/var/lib/helios/diagnostics"),
            run_dir: PathBuf::from("/run/helios-diagnostics"),
            tar: true,
            max_cmd_bytes: 1_000_000, // 1 MiB per stream
            cmd_timeout: Duration::from_secs(15),
        }
    }
}

pub struct SnapshotBuilder {
    cfg: SnapshotConfig,
    meta: Meta,
    out_dir: PathBuf,
    commands: Vec<CmdResult>,
}

impl SnapshotBuilder {
    pub fn new(cfg: SnapshotConfig, trigger: &str, unit: Option<String>, tag: Option<String>) -> Result<Self> {
        let ts = Utc::now().format("%Y%m%d-%H%M%S");
        let host = hostname();
        let id = match (&unit, &tag) {
            (Some(u), _) => format!("{}-{}-{}", ts, trigger, sanitize_name(u)),
            (None, Some(t)) => format!("{}-{}-{}", ts, trigger, sanitize_name(t)),
            _ => format!("{}-{}", ts, trigger),
        };
        let out_dir = cfg.base_dir.join(&id);
        ensure_dir(&out_dir)?;
        Ok(Self { cfg, meta: Meta { id, created_utc: Utc::now(), host, trigger: trigger.to_string(), unit, tag, version: None }, out_dir, commands: Vec::new() })
    }

    pub fn out_dir(&self) -> &Path {
        &self.out_dir
    }

    pub async fn collect(mut self) -> Result<Summary> {
        // Write meta
        let meta_path = self.out_dir.join("meta.json");
        write_bytes(&meta_path, &serde_json::to_vec_pretty(&self.meta)?)?;

        // Collect commands
        self.run("uname", "uname", &["-a"]).await?;
        self.run("os-release", "cat", &["/etc/os-release"]).await?;
        self.run("cmdline", "cat", &["/proc/cmdline"]).await?;
        self.run("uptime", "uptime", &[]).await?;
        self.run("date", "date", &[]).await?;
        self.run("df", "df", &["-h"]).await?;
        self.run("mount", "mount", &[]).await?;
        self.run("free", "free", &["-h"]).await?;
        self.run("ps", "ps", &["-ef"]).await?;
        self.run("systemd-blame", "systemd-analyze", &["blame"]).await.ok();
        self.run("systemd-critical-chain", "systemd-analyze", &["critical-chain"]).await.ok();
        // Optional utilities (not always present on minimal images)
        self.run("lsblk", "lsblk", &["-o", "NAME,SIZE,TYPE,MOUNTPOINT"]).await.ok();
        self.run("blkid", "blkid", &[]).await.ok();
        self.run("df-inodes", "df", &["-ih"]).await.ok();
        self.run("smartctl", "sh", &["-lc", "(command -v smartctl >/dev/null 2>&1 && smartctl -a /dev/sda) || true"]).await.ok();

        self.run("dmesg", "dmesg", &["-T"]).await.ok();
        self.run("lsmod", "lsmod", &[]).await.ok();
        self.run("systemd-status", "systemctl", &["status", "--no-pager", "--full"]).await.ok();
        self.run("systemd-failed", "systemctl", &["list-units", "--failed"]).await.ok();
        self.run("journalctl-boot", "journalctl", &["-b", "-n", "2000", "--no-pager", "-o", "short-precise"]).await.ok();

        self.run("ip-addr", "ip", &["addr"]).await.ok();
        self.run("ip-link", "ip", &["-s", "link"]).await.ok();
        self.run("ip-route", "ip", &["route"]).await.ok();
        self.run("ss-listen", "ss", &["-tulpn"]).await.ok();
        self.run("resolvectl", "resolvectl", &["status"]).await.ok();
        self.run("networkd-journal", "journalctl", &["-u", "systemd-networkd.service", "-b", "-n", "400", "--no-pager", "-o", "short-precise"]).await.ok();
        self.run("wifi-iw", "sh", &["-lc", "(command -v iw >/dev/null 2>&1 && iw dev && iw wlan0 link && iw wlan0 station dump) || true"]).await.ok();
        self.run("ethtool", "sh", &["-lc", "(command -v ethtool >/dev/null 2>&1 && for i in $(ls /sys/class/net); do echo ==== $i ====; ethtool $i || true; ethtool -S $i || true; done) || true"])
            .await
            .ok();

        // USB gadget/UDC diagnostics (best-effort; may be absent if gadget disabled)
        self.run("usb-udc", "sh", &["-lc", "ls -l /sys/class/udc || true"]).await.ok();
        self.run("usb-dr_mode", "sh", &["-lc", "for f in /proc/device-tree/**/dr_mode; do echo \"FILE: $f\"; tr -d '\\0' < \"$f\"; echo; done 2>/dev/null || true"]).await.ok();
        self.run("usb-role", "sh", &["-lc", "for f in /sys/bus/platform/devices/*usb*/role /sys/class/usb_role/*/role; do [ -r \"$f\" ] && echo \"$f: $(cat $f)\"; done 2>/dev/null || true"])
            .await
            .ok();
        self.run("usb-gadget-tree", "sh", &["-lc", "ls -lR /sys/kernel/config/usb_gadget || true"]).await.ok();
        self.run("lsusb", "sh", &["-lc", "(command -v lsusb >/dev/null 2>&1 && lsusb -vvv) || true"]).await.ok();
        // Journal: prefer SyslogIdentifier tag and also include the explicit unit if present
        self.run("usb-gadget-journal-tag", "journalctl", &["-t", "usb-gadget", "-b", "-n", "300", "--no-pager", "-o", "short-precise"]).await.ok();
        self.run("usb-gadget-journal-unit", "journalctl", &["-u", "helios-usb-gadget.service", "-b", "-n", "300", "--no-pager", "-o", "short-precise"]).await.ok();
        // Watchdog (auto-reset) logs, if present
        self.run("usb-gadget-watchdog-journal-tag", "journalctl", &["-t", "usb-gadget-watchdog", "-b", "-n", "300", "--no-pager", "-o", "short-precise"]).await.ok();
        self.run("usb-gadget-watchdog-journal-unit", "journalctl", &["-u", "helios-usb-gadget-watchdog.service", "-b", "-n", "300", "--no-pager", "-o", "short-precise"]).await.ok();
        self.run("usb-gadget-watchdog-logfile", "sh", &["-lc", "tail -n 500 /var/log/usb-gadget-watchdog.log 2>/dev/null || true"]).await.ok();
        // Focused kernel logs for USB/UDC/gadget
        self.run("usb-kernel", "sh", &["-lc", "journalctl -k -b -g 'dwc\\|udc\\|gadget\\|configfs' -n 500 --no-pager -o short-precise || true"]).await.ok();
        // Focused kernel logs for camera/unicam/ISP
        self.run("camera-kernel", "sh", &["-lc", "journalctl -k -b -g 'unicam\\|bcm2835-isp\\|vc4\\|libcamera' -n 1000 --no-pager -o short-precise || true"]).await.ok();
        // Show unit state and installed/enablement for the gadget unit
        /* self.run(
            "usb-gadget-status",
            "sh",
            &["-lc", "systemctl status helios-usb-gadget.service --no-pager --full || true; systemctl list-unit-files | grep -E '^helios-usb-gadget\\.service' || true; systemctl list-units | grep -E 'helios-usb-gadget\\.service' || true"],
        )
        .await
        .ok(); */
        // Networkd and link state around the USB bridge
        self.run("networkctl-list", "networkctl", &["list"]).await.ok();
        self.run("networkctl-usbbr0", "networkctl", &["status", "usbbr0"]).await.ok();
        self.run("ip-link-usb", "sh", &["-lc", "ip -d link show dev usbbr0 || true; ip -d link show dev usb0 || true; ip -d link show dev end0 || true"]).await.ok();
        // Kernel .config if available (Buildroot can expose /proc/config.gz)
        /* self.run(
            "kernel-config-usb",
            "sh",
            &["-lc", "zcat /proc/config.gz 2>/dev/null | egrep -i 'CONFIG_USB_(GADGET|DWC2|DWC3|LIBCOMPOSITE|CONFIGFS|ROLE_SWITCH)|CONFIG_USB_CONFIGFS_(ECM|RNDIS|NCM)' || true"],
        )
        .await
        .ok(); */
        // Driver bind directories (helps see if dwc2/dwc3 is present and bound)
        self.run("dwc-drivers", "sh", &["-lc", "echo 'dwc2:'; ls -l /sys/bus/platform/drivers/dwc2 2>/dev/null || true; echo; echo 'dwc3:'; ls -l /sys/bus/platform/drivers/dwc3 2>/dev/null || true"])
            .await
            .ok();
        // Dnsmasq (USB gadget DHCP) status and logs
        self.run("dnsmasq-status", "sh", &["-lc", "systemctl status helios-dnsmasq.service --no-pager --full || true"]).await.ok();
        self.run("dnsmasq-journal", "journalctl", &["-u", "helios-dnsmasq.service", "-b", "-n", "300", "--no-pager", "-o", "short-precise"]).await.ok();

        self.run("cpuinfo", "cat", &["/proc/cpuinfo"]).await.ok();
        self.run("model", "cat", &["/sys/firmware/devicetree/base/model"]).await.ok();
        self.run(
            "hwmon-thermal",
            "sh",
            &["-lc", "for z in /sys/class/thermal/thermal_zone*; do [ -e \"$z\" ] || continue; echo \"=== $z\"; cat \"$z/type\" 2>/dev/null; cat \"$z/temp\" 2>/dev/null; done"],
        )
        .await
        .ok();
        self.run("hwmon-cpu", "sh", &["-lc", "for h in /sys/class/hwmon/hwmon*; do [ -e \"$h\" ] || continue; echo \"=== $h\"; cat \"$h/name\" 2>/dev/null; for t in $h/temp*_input; do [ -f \"$t\" ] && echo \"$t=$(cat $t)\"; done; done"]).await.ok();
        self.run(
            "gpu-busy",
            "sh",
            &["-lc", "for f in /sys/class/drm/*/device/gpu_busy_percent /sys/class/drm/*/device/engine/*/busy_percent; do [ -f \"$f\" ] && echo \"$f=$(cat $f)\"; done 2>/dev/null || true"],
        )
        .await
        .ok();
        self.run(
            "vcgencmd",
            "sh",
            &["-lc", "(command -v vcgencmd >/dev/null 2>&1 && vcgencmd measure_temp && vcgencmd measure_clock arm && vcgencmd measure_clock core && vcgencmd measure_volts) || true"],
        )
        .await
        .ok();
        // Device-tree overlays and camera-related nodes (best-effort)
        self.run("dt-overlays", "sh", &["-lc", "echo '== chosen overlays ==' ; ls -laR /proc/device-tree/chosen/overlays 2>/dev/null || true"]).await.ok();
        // /self.run(
        //     "dt-cam-nodes",
        //     "sh",
        //     &[
        //         "-lc",
        //         "for d in /sys/firmware/devicetree/base/soc/*/csi@* /sys/firmicetree/base/soc/i2c0mux/*/ov* /sys/firmware/devicetree/base/soc/i2c*/ov*; do \
        //              [ -d \"$d\" ] || continue; echo '=== ' $d; \
        //              for f in compatible status reg clocks clock-frequency clock-names; do \
        //                  [ -f \"$d/$f\" ] && { printf '%s: ' "$f"; tr -d '\\0' < \"$d/$f\"; echo; }; \
        //              done; echo; done 2>/dev/null || true",
        //     ],
        // )
        // .await
        // .ok();
        // I2C probe on common camera buses (best-effort)
        self.run("i2c-detect-cam", "sh", &["-lc", "for b in 0 1 10 22; do echo ==== i2c-$b ====; (command -v i2cdetect >/dev/null 2>&1 && i2cdetect -y $b) || echo 'i2cdetect not available'; done"])
            .await
            .ok();

        // Media/camera if available
        self.run("v4l2-devices", "v4l2-ctl", &["--list-devices"]).await.ok();
        self.run("media-ctl", "media-ctl", &["-p"]).await.ok();
        // Also dump additional media devices if present
        self.run("media-ctl-all", "sh", &["-lc", "for n in /dev/media0 /dev/media1 /dev/media2 /dev/media3; do [ -e \"$n\" ] && echo ==== $n ==== && media-ctl -p -d $n || true; done"]).await.ok();
        // Device nodes and udev details (best-effort)
        self.run("media-dev", "sh", &["-lc", "ls -l /dev/media* 2>/dev/null || true"]).await.ok();
        self.run("video-devs", "sh", &["-lc", "ls -l /dev/video* 2>/dev/null || true"]).await.ok();
        self.run("udevadm-video", "sh", &["-lc", "for n in /dev/video*; do [ -e \"$n\" ] || continue; echo '====' $n; udevadm info --query=all --name=$n 2>/dev/null || true; done"]).await.ok();
        // libcamera installation layout and dependencies (best-effort)
        self.run("libcamera-layout", "sh", &["-lc", "for d in /usr/lib/libcamera /usr/libexec/libcamera /usr/share/libcamera; do echo '== ' $d ' =='; [ -d \"$d\" ] && ls -laR \"$d\" || true; done"])
            .await
            .ok();
        self.run("ipa-ldd-proxies", "sh", &["-lc", "for f in /usr/libexec/libcamera/*_ipa_proxy; do [ -x \"$f\" ] && { echo '-- ' $f ' --'; ldd \"$f\" || true; }; done || true"]).await.ok();
        self.run("ipa-ldd-so", "sh", &["-lc", "for f in /usr/lib/libcamera/ipa/ipa_*.so; do [ -f \"$f\" ] && { echo '-- ' $f ' --'; ldd \"$f\" || true; }; done || true"]).await.ok();
        // Fallback when ldd is not present: use readelf -d to list NEEDED entries
        self.run(
            "ipa-readelf-proxies",
            "sh",
            &["-lc", "for f in /usr/libexec/libcamera/*_ipa_proxy; do [ -x \"$f\" ] && { echo '-- ' $f ' --'; readelf -d \"$f\" 2>/dev/null | grep NEEDED || true; }; done || true"],
        )
        .await
        .ok();
        self.run(
            "ipa-readelf-so",
            "sh",
            &["-lc", "for f in /usr/lib/libcamera/ipa/ipa_*.so; do [ -f \"$f\" ] && { echo '-- ' $f ' --'; readelf -d \"$f\" 2>/dev/null | grep NEEDED || true; }; done || true"],
        )
        .await
        .ok();
        // Hashes for cross-bundle comparison
        self.run("ipa-sha256", "sh", &["-lc", "(command -v sha256sum >/dev/null 2>&1 && sha256sum /usr/lib/libcamera/ipa/ipa_*.so /usr/libexec/libcamera/*_ipa_proxy) || true"]).await.ok();
        // Verify libcamera libs uniqueness across lib dirs
        self
            .run(
                "libcamera-libs",
                "sh",
                &["-lc", "for d in /lib /lib64 /usr/lib /usr/lib64; do for f in $d/libcamera*.so*; do [ -e \"$f\" ] && echo -- $f && ls -l \"$f\" && (command -v sha256sum >/dev/null 2>&1 && sha256sum \"$f\") || true; done; done 2>/dev/null"],
            )
            .await
            .ok();
        // Helios backend logs and journal (best-effort)
        self.run("helios-journal", "journalctl", &["-u", "helios-backend.service", "-b", "-n", "2000", "--no-pager", "-o", "short-precise"]).await.ok();
        self.run("helios-engine-journal", "journalctl", &["-u", "helios-engine.service", "-b", "-n", "2000", "--no-pager", "-o", "short-precise"]).await.ok();
        self.run("helios-api-journal", "journalctl", &["-u", "helios-api.service", "-b", "-n", "2000", "--no-pager", "-o", "short-precise"]).await.ok();
        self.run("helios-peripherals-journal", "journalctl", &["-u", "helios-peripherals.service", "-b", "-n", "800", "--no-pager", "-o", "short-precise"]).await.ok();
        self.run("helios-updater-journal", "journalctl", &["-u", "helios-updater.service", "-b", "-n", "400", "--no-pager", "-o", "short-precise"]).await.ok();
        self.run("helios-metrics-scrape", "sh", &["-lc", "curl -m 3 -fsS http://127.0.0.1:9100/metrics || true"]).await.ok();

        // Helios state snapshots (best-effort)
        self.run("helios-pipelines", "sh", &["-lc", "cat /var/lib/helios/pipelines/*.json 2>/dev/null || true"]).await.ok();
        self.run("helios-sessions", "sh", &["-lc", "ls -l /var/lib/helios/sessions 2>/dev/null && cat /var/lib/helios/sessions/*.json 2>/dev/null || true"]).await.ok();
        self.run("helios-resource-telemetry", "sh", &["-lc", "cat /run/helios/resource-telemetry.json 2>/dev/null || true"]).await.ok();

        // Copy key config files and dirs (best-effort)
        let files_out = self.out_dir.join("files");
        let _ = try_glob_copy(
            &[
                "/boot/config.txt",
                "/boot/cmdline.txt",
                "/etc/hostname",
                "/etc/systemd/network/*.network",
                "/etc/systemd/network/*.netdev",
                "/etc/systemd/network/*.link",
                // Backend service + env for verification
                "/etc/systemd/system/helios-backend.service",
                "/etc/systemd/system/helios-backend.service.d/*.conf",
                "/etc/default/helios-backend",
                // Crash artifacts and panic logs
                "/var/log/helios/panics/*.log",
                "/var/log/helios/core-*",
                "/var/lib/systemd/coredump/*",
                "/etc/modules-load.d/*.conf",
                "/etc/modprobe.d/*.conf",
                "/etc/dnsmasq*.conf",
                "/etc/dnsmasq.d/*",
                "/etc/udev/rules.d/*.rules",
                "/etc/helios/*",
                "/var/log/usb-gadget.log",
                // Helios logs (default location set by builder)
                "/var/log/helios/*",
            ],
            &files_out,
        );

        // For each /dev/video*, include a v4l2-ctl --all dump (best-effort)
        for dev in (glob::glob("/dev/video*")?).flatten() {
            if dev.is_file() {
                self.run_dev_dump("v4l2-all", &dev).await.ok();
            }
        }

        // Write commands summary
        let summary = Summary { bundle_dir: self.out_dir.display().to_string(), archive: None, meta: self.meta.clone(), commands: self.commands.clone() };
        write_bytes(&self.out_dir.join("summary.json"), &serde_json::to_vec_pretty(&summary)?)?;
        Ok(summary)
    }

    async fn run(&mut self, name: &str, program: &str, args: &[&str]) -> Result<()> {
        let spec = CmdSpec { program, args, env: &[], cwd: None, name, max_bytes: self.cfg.max_cmd_bytes, timeout: self.cfg.cmd_timeout };
        let (meta, stdout, stderr) = run_cmd(&spec).await?;
        let dir = self.out_dir.join("commands").join(name);
        ensure_dir(&dir)?;
        write_str(&dir.join("cmd.txt"), &format!("{} {:?}\nstatus={:?}\n", program, args, meta.status))?;
        if !stdout.is_empty() {
            write_bytes(&dir.join("stdout.txt"), &stdout)?;
        }
        if !stderr.is_empty() {
            write_bytes(&dir.join("stderr.txt"), &stderr)?;
        }
        self.commands.push(meta);
        Ok(())
    }

    async fn run_dev_dump(&mut self, name: &str, dev: &Path) -> Result<()> {
        let devstr = dev.to_string_lossy().to_string();
        let spec = CmdSpec { program: "v4l2-ctl", args: &["--all", "-d", &devstr], env: &[], cwd: None, name, max_bytes: self.cfg.max_cmd_bytes, timeout: self.cfg.cmd_timeout };
        let (meta, stdout, stderr) = run_cmd(&spec).await?;
        let out = self.out_dir.join("v4l2").join(dev.file_name().unwrap_or_default());
        ensure_dir(&out)?;
        if !stdout.is_empty() {
            write_bytes(&out.join("all.txt"), &stdout)?;
        }
        if !stderr.is_empty() {
            write_bytes(&out.join("stderr.txt"), &stderr)?;
        }
        self.commands.push(meta);
        Ok(())
    }
}

fn sanitize_name<S: AsRef<str>>(s: S) -> String {
    s.as_ref().chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect()
}

fn hostname() -> Option<String> {
    match std::fs::read_to_string("/etc/hostname") {
        Ok(s) => Some(s.trim().to_string()),
        Err(_) => None,
    }
}
