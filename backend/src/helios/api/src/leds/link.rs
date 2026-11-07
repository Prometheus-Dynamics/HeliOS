//! Whether the link to robot code's NetworkTables server is physically down, so the ring shows
//! the loss within a second instead of when the TCP session finally dies.
//!
//! The interface the kernel routes the server's address through must have carrier. When it is the
//! USB gadget (a `usb0`-like function, or the bridge over them), the gadget's controller must be
//! `configured` too: a host whose hub drops leaves the board powered with the cable in, and the
//! controller goes `suspended` or `not attached` while the function's carrier can stay up.

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
    path::Path,
};

/// The address of a server written as `host`, `host:port`, `[v6]:port` or a bare IP.
pub fn server_ip(server: &str) -> Option<IpAddr> {
    let server = server.trim();
    if let Ok(ip) = server.parse::<IpAddr>() {
        return Some(ip);
    }
    if let Ok(addr) = server.parse::<SocketAddr>() {
        return Some(addr.ip());
    }
    server.rsplit_once(':').and_then(|(host, _)| host.parse().ok())
}

/// Whether the route to `server` is down: no route at all, or its interface without a live link.
/// `sys` is `/sys`.
pub fn route_down(server: IpAddr, sys: &Path) -> bool {
    match route_interface(server) {
        Some(interface) => interface_down(sys, &interface),
        None => true,
    }
}

/// The interface the kernel would send to `server` from (a UDP connect sends nothing). `None`
/// without a route; loopback counts as up and returns `None` only when unrouted.
fn route_interface(server: IpAddr) -> Option<String> {
    let bind: SocketAddr = match server {
        IpAddr::V4(_) => (Ipv4Addr::UNSPECIFIED, 0).into(),
        IpAddr::V6(_) => (Ipv6Addr::UNSPECIFIED, 0).into(),
    };
    let socket = UdpSocket::bind(bind).ok()?;
    socket.connect((server, 9)).ok()?;
    let local = socket.local_addr().ok()?.ip();
    interface_with(local)
}

/// The interface holding `address` (`getifaddrs`).
fn interface_with(address: IpAddr) -> Option<String> {
    let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs fills `list`, freed below.
    #[allow(unsafe_code)]
    if unsafe { libc::getifaddrs(&mut list) } != 0 {
        return None;
    }
    let mut found = None;
    let mut entry = list;
    // SAFETY: walks the list getifaddrs returned; every pointer is checked before use.
    #[allow(unsafe_code)]
    unsafe {
        while !entry.is_null() {
            let addr = (*entry).ifa_addr;
            if !addr.is_null() {
                let ip = match i32::from((*addr).sa_family) {
                    libc::AF_INET => Some(IpAddr::V4(Ipv4Addr::from(u32::from_be((*addr.cast::<libc::sockaddr_in>()).sin_addr.s_addr)))),
                    libc::AF_INET6 => Some(IpAddr::V6(Ipv6Addr::from((*addr.cast::<libc::sockaddr_in6>()).sin6_addr.s6_addr))),
                    _ => None,
                };
                if ip == Some(address) {
                    found = std::ffi::CStr::from_ptr((*entry).ifa_name).to_str().ok().map(str::to_string);
                    break;
                }
            }
            entry = (*entry).ifa_next;
        }
        libc::freeifaddrs(list);
    }
    found
}

/// Whether `interface` has no live link: no carrier, or a USB gadget function (itself or a bridge
/// member) whose controller is not `configured`.
pub fn interface_down(sys: &Path, interface: &str) -> bool {
    let net = sys.join("class/net").join(interface);
    if !net.exists() {
        // Loopback and other addresses with no sysfs entry are local.
        return false;
    }
    if interface == "lo" {
        return false;
    }
    if read(&net.join("carrier")).as_deref() != Some("1") {
        return true;
    }
    let mut functions: Vec<String> = vec![interface.to_string()];
    if let Ok(members) = std::fs::read_dir(net.join("brif")) {
        functions = members.filter_map(Result::ok).map(|member| member.file_name().to_string_lossy().into_owned()).collect();
    }
    let controllers: Vec<String> = functions.iter().filter_map(|function| gadget_controller(sys, function)).collect();
    // Not a gadget: the carrier says it all. A gadget: its controller must be configured.
    !controllers.is_empty() && !controllers.iter().any(|controller| read(&sys.join("class/udc").join(controller).join("state")).as_deref() == Some("configured"))
}

/// The UDC behind a gadget function's netdev: its device is `<controller>/gadget.N`.
fn gadget_controller(sys: &Path, function: &str) -> Option<String> {
    let device = std::fs::canonicalize(sys.join("class/net").join(function).join("device")).ok()?;
    let name = device.file_name()?.to_str()?;
    if !name.starts_with("gadget") {
        return None;
    }
    device.parent()?.file_name()?.to_str().map(str::to_string)
}

fn read(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|text| text.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    struct Sys(std::path::PathBuf);

    impl Sys {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!("helios-link-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("class/udc/1000480000.usb")).unwrap();
            std::fs::create_dir_all(root.join("devices/axi/1000480000.usb/gadget.0")).unwrap();
            std::fs::create_dir_all(root.join("devices/axi/end0")).unwrap();
            Self(root)
        }
        fn netdev(&self, name: &str, carrier: &str, device: Option<&str>) {
            let net = self.0.join("class/net").join(name);
            std::fs::create_dir_all(&net).unwrap();
            std::fs::write(net.join("carrier"), carrier).unwrap();
            if let Some(device) = device {
                symlink(self.0.join(device), net.join("device")).unwrap();
            }
        }
        fn bridge(&self, name: &str, carrier: &str, members: &[&str]) {
            self.netdev(name, carrier, None);
            let brif = self.0.join("class/net").join(name).join("brif");
            std::fs::create_dir_all(&brif).unwrap();
            for member in members {
                std::fs::create_dir_all(brif.join(member)).unwrap();
            }
        }
        fn udc(&self, state: &str) {
            std::fs::write(self.0.join("class/udc/1000480000.usb/state"), state).unwrap();
        }
    }

    impl Drop for Sys {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn servers_parse_with_or_without_ports() {
        assert_eq!(server_ip("10.12.34.2"), Some("10.12.34.2".parse().unwrap()));
        assert_eq!(server_ip("10.12.34.2:5810"), Some("10.12.34.2".parse().unwrap()));
        assert_eq!(server_ip("[fe80::1]:5810"), Some("fe80::1".parse().unwrap()));
        assert_eq!(server_ip("roborio-1234-frc.local"), None);
    }

    #[test]
    fn the_gadget_bridge_is_down_when_its_controller_is_not_configured() {
        let sys = Sys::new("gadget");
        sys.netdev("usb0", "1", Some("devices/axi/1000480000.usb/gadget.0"));
        sys.netdev("usb1", "0", Some("devices/axi/1000480000.usb/gadget.0"));
        sys.bridge("usbbr0", "1", &["usb0", "usb1"]);
        sys.udc("configured");
        assert!(!interface_down(&sys.0, "usbbr0"));
        assert!(!interface_down(&sys.0, "usb0"));
        // The host's hub dropped: the board is still powered, the carrier still up.
        sys.udc("suspended");
        assert!(interface_down(&sys.0, "usbbr0"));
        sys.udc("not attached");
        assert!(interface_down(&sys.0, "usb0"));
        // The cable out: no carrier.
        sys.udc("configured");
        std::fs::write(sys.0.join("class/net/usbbr0/carrier"), "0").unwrap();
        assert!(interface_down(&sys.0, "usbbr0"));
    }

    #[test]
    fn other_interfaces_follow_their_carrier() {
        let sys = Sys::new("ethernet");
        sys.netdev("end0", "1", Some("devices/axi/end0"));
        sys.udc("not attached");
        assert!(!interface_down(&sys.0, "end0"));
        std::fs::write(sys.0.join("class/net/end0/carrier"), "0").unwrap();
        assert!(interface_down(&sys.0, "end0"));
        assert!(!interface_down(&sys.0, "lo"));
    }

    #[test]
    fn loopback_routes_are_up() {
        assert!(!route_down("127.0.0.1".parse().unwrap(), Path::new("/sys")));
    }
}
