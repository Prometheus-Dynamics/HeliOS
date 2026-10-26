# SystemCore (guest profile): device facts

What a SystemCore unit on `limelightosr-2027.0.0-alpha18` (OS commit `aef8f2eb`) provides, read
from a unit over USB on 2026-10-26. The guest profile ([device-profiles.md](../device-profiles.md))
and the Lemnos SystemCore board definition are built from these facts.

## System

| | |
|---|---|
| SoC | Raspberry Pi Compute Module 5 Rev 1.0, 4 cores, 4 GB RAM, no swap |
| Kernel | `6.12.112-v8-16k`, `PREEMPT_RT`, **16K pages**, `ipv6.disable=1`, `net.ifnames=0` |
| libc | glibc 2.42 (Buildroot); no `ldd`, no `getconf` on the device |
| Storage | eMMC 14.6 GB: three FAT boot partitions, two 7 GB rootfs (A/B), `/` is `mmcblk0p5` ext4 |
| Governor | `ondemand` on all cores (`limelight_cpupower.service`) |
| User | `systemcore`, passwordless `sudo`, groups include `video`, `gpio`, `i2c`, `spi`, `dialout`, `disk` |

## Network

| Interface | Address | Use |
|---|---|---|
| `usb0` (ECM, configuration 1) | `172.27.0.1/24` | Linux/macOS hosts; DHCP hands out `172.27.0.x` |
| `usb1` (RNDIS, configuration 2) | `172.26.0.1/24` | Windows hosts |
| `wlan0` | `172.30.0.1/24` | access point |
| `eth0` | robot network | |
| `can_s0`..`can_s4` | | the five CAN buses (`robot.service` waits for them) |
| `can_d0`..`can_d19` | | virtual CAN devices |

The USB gadget is `1d6b:0104`, manufacturer `Limelight`, product `Raspberry Pi Compute Module 5
Rev 1.0` (`limelight_gadget.service`). `robot.local` resolves to `172.27.0.1` over mDNS.

**Linux host notes.** Any host interface that already holds `172.27.0.1` (a Docker bridge from
the default `172.16.0.0/12` pool is the usual cause) makes the unit unreachable, because the host
answers its own address. NetworkManager may also leave the link without a profile; a DHCP
profile bound to the gadget's host-side MAC with `ipv4.never-default yes` fixes it.

Ports in use by the host: 22, 53, 67, 1110/1115/1136 (udp), 1740/1741, 5800-5837 (Limelight
services, Elastic on 5803), 5808/5809 (udp), 9001 (Blocks). HeliOS must pick a port outside these.
HeliOS uses 5880 (its API and UI, `helios.api_port`) and 9200 (Orion's peer listener, signed
requests only). Discovery runs on `ethadapt0` (the Raze) and `eth0` (the robot network).

## Hardware HeliOS may see

| Device | Owner | Notes |
|---|---|---|
| USB cameras (UVC) | **host** by default | Limelight's four vision servers (`limelight_visionserver`, `1`, `2`, `3`) claim USB ports; HeliOS takes only ports the host's vision leaves free |
| `/dev/video19`..`35` | host | Pi ISP/codec nodes, not cameras |
| RT subsystem MCU | host | USB `cafe:4011` "Limelight RT Subsystem" (RP2350), through `limelight_iodaemon` |
| CAN, power distribution, radio, expansion hub, motioncore | host | `limelight_canbus*`, `powerdistribution`, `radiodaemon`, `expansionhubdaemon`, `limelight_motioncore*` |
| Fan, LEDs, power button | host | `cooling_fan`, `leds`, `pwr_button` device-tree nodes |
| `/dev/i2c-1`, `/dev/i2c-10`, `/dev/spidev10.0`, `/dev/gpiochip11`..`15`, `/dev/ttyAMA10`, `/dev/ttyS0` | host | `ttyAMA10` is the serial console |
| Hailo NPU | host | `hailort.service` runs; no `/dev/hailo*` and no PCIe device on this unit |
| Docker | host | `dockerd` runs; `systemcore` is not in the `docker` group |

Every device the board definition lists is **host-owned** on alpha18: Lemnos reads what the host
exposes and drives nothing.

## Packages (`opkg`)

- `opkg print-architecture`: `all`, `noarch`, `aarch64`. Native packages use `Architecture: aarch64`.
- Installed third-party packages (Blocks, Elastic) put their files under `/opt/<name>` or
  `/usr/share`, plus one unit at `/etc/systemd/system/<name>.service`.
- Their maintainer scripts: `postinst` runs `daemon-reload`, `enable` and `start`; `prerm` runs
  `stop` and `disable`; `postrm` runs `daemon-reload` and `reset-failed`.
- Host UI fields in `control`: `X-Port`, `X-Display-Name`, `X-Has-UI`, `X-Auto-Start`,
  `X-Icon-Path`. `limelight_packagemanager` reads them.

## Scheduling

Limelight's vision servers run in `vision.slice` with `CPUWeight=idle` (SCHED_IDLE). They get CPU
only when no control or safety service wants it. HeliOS is vision too, so its units take
`Slice=vision.slice` and stay below the robot's control paths. The robot program
(`robot.service`) runs as `systemcore` with `LimitRTPRIO=50`.
