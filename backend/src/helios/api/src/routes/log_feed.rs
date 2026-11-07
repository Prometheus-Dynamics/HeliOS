//! The live journal without a `journalctl -f` (about 8 MB each on the CM5) per filter.
//!
//! journald forwards every entry it gets (`ForwardToSyslog=yes`, HeliOS's journald.conf.d) to
//! `/run/systemd/journal/syslog`, with the sender's credentials. While anyone follows the journal
//! the API binds that socket, and reads the kernel's lines from `/dev/kmsg`: one feed in-process,
//! every viewer filtering it. Where the socket is someone else's (a syslog daemon), can't be bound,
//! or journald doesn't forward to it (a host journald without the setting), the caller falls back to
//! `journalctl -f`.

use std::{
    io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tokio::{
    io::{Interest, unix::AsyncFd},
    net::UnixDatagram,
    sync::broadcast,
    task::JoinHandle,
};

use crate::host::{self, LogLine};

/// Where journald forwards to (`ForwardToSyslog=yes`).
pub const SYSLOG_SOCKET: &str = "/run/systemd/journal/syslog";
/// How long a new feed waits for journald to forward its probe line.
const PROBE_WAIT: Duration = Duration::from_secs(2);
/// The largest datagram read; journald's lines are cut to `LINE_MAX` (2 KiB) by default.
const DATAGRAM_MAX: usize = 16 * 1024;

/// One journal entry, serialized once for every viewer.
pub struct Entry {
    /// Syslog priority, 0 (emergency) to 7 (debug).
    pub priority: u8,
    pub line: LogLine,
    pub json: Arc<str>,
}

impl Entry {
    fn new(priority: u8, line: LogLine) -> Self {
        let json = Arc::from(serde_json::to_string(&line).unwrap_or_default());
        Self { priority, line, json }
    }
}

/// The feed: the bound socket and `/dev/kmsg`, read by one task while it lives.
pub struct Feed {
    tx: broadcast::Sender<Arc<Entry>>,
    task: JoinHandle<()>,
    path: PathBuf,
}

impl Feed {
    /// Binds `path` and starts reading. `None` when the socket is someone else's or can't be bound
    /// here. `kmsg` also reads the kernel's lines (off in tests).
    pub fn bind(path: &Path, kmsg: bool) -> Option<Self> {
        let socket = bind_socket(path)?;
        let (tx, _) = broadcast::channel(256);
        let sender = tx.clone();
        let kmsg = if kmsg { open_kmsg().ok() } else { None };
        let task = tokio::spawn(read(socket, kmsg, sender));
        Some(Self { tx, task, path: path.to_path_buf() })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<Entry>> {
        self.tx.subscribe()
    }

    /// Whether journald forwards to the feed: logs `token` and waits for it to come back from this
    /// process.
    pub async fn probe(&self, token: &str) -> bool {
        let mut receiver = self.subscribe();
        tracing::info!(target: "helios_logs", "live log feed attached ({token})");
        let own = std::process::id();
        tokio::time::timeout(PROBE_WAIT, async {
            loop {
                match receiver.recv().await {
                    Ok(entry) if entry.line.pid == Some(own) && entry.line.message.contains(token) => return true,
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => return false,
                }
            }
        })
        .await
        .unwrap_or(false)
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        self.task.abort();
        // journald finds no socket again, and stops forwarding.
        let _ = std::fs::remove_file(&self.path);
    }
}

fn bind_socket(path: &Path) -> Option<UnixDatagram> {
    let socket = match UnixDatagram::bind(path) {
        Ok(socket) => socket,
        Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
            // A live owner (a syslog daemon) answers a connect; a stale file from an earlier API doesn't.
            let probe = std::os::unix::net::UnixDatagram::unbound().ok()?;
            if probe.connect(path).is_ok() {
                return None;
            }
            std::fs::remove_file(path).ok()?;
            UnixDatagram::bind(path).ok()?
        }
        Err(_) => return None,
    };
    let on: libc::c_int = 1;
    // SAFETY: a valid socket fd and an int option of the right size.
    #[allow(unsafe_code)]
    let set = unsafe { libc::setsockopt(socket.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PASSCRED, (&on as *const libc::c_int).cast(), std::mem::size_of::<libc::c_int>() as libc::socklen_t) };
    if set != 0 {
        let _ = std::fs::remove_file(path);
        return None;
    }
    Some(socket)
}

fn open_kmsg() -> io::Result<AsyncFd<OwnedFd>> {
    // SAFETY: a NUL-terminated path; the fd is owned from here on.
    #[allow(unsafe_code)]
    let fd = unsafe { libc::open(c"/dev/kmsg".as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    #[allow(unsafe_code)]
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    // Only lines from now on: the backlog comes from `journalctl -n`.
    #[allow(unsafe_code)]
    unsafe {
        libc::lseek(fd.as_raw_fd(), 0, libc::SEEK_END)
    };
    // SAFETY: the fd is owned by the AsyncFd and neither replaced nor closed while registered.
    #[allow(unsafe_code)]
    unsafe { AsyncFd::register_with_interest(fd, Interest::READABLE) }.map_err(|error| error.into_parts().1)
}

async fn read(socket: UnixDatagram, kmsg: Option<AsyncFd<OwnedFd>>, tx: broadcast::Sender<Arc<Entry>>) {
    let mut buf = vec![0u8; DATAGRAM_MAX];
    let mut kbuf = vec![0u8; 8192];
    loop {
        tokio::select! {
            received = socket.async_io(Interest::READABLE, || recv_with_pid(&socket, &mut buf)) => {
                let Ok((len, pid)) = received else { continue };
                if let Some(entry) = parse_forwarded(&buf[..len], pid) {
                    let _ = tx.send(Arc::new(entry));
                }
            }
            line = read_kmsg(kmsg.as_ref(), &mut kbuf) => {
                match line {
                    Ok(len) => {
                        if let Some(entry) = parse_kmsg(&kbuf[..len]) {
                            let _ = tx.send(Arc::new(entry));
                        }
                    }
                    // EPIPE: records were overwritten before they were read; go on with the next one.
                    Err(error) if error.raw_os_error() == Some(libc::EPIPE) => {}
                    Err(_) => return,
                }
            }
        }
    }
}

/// One kernel record; pending forever without `/dev/kmsg`.
async fn read_kmsg(kmsg: Option<&AsyncFd<OwnedFd>>, buf: &mut [u8]) -> io::Result<usize> {
    let Some(kmsg) = kmsg else { return std::future::pending().await };
    loop {
        let mut ready = kmsg.readable().await?;
        // SAFETY: reads into a buffer of its stated length.
        #[allow(unsafe_code)]
        let read = unsafe { libc::read(kmsg.get_ref().as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
        if read >= 0 {
            return Ok(read as usize);
        }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::WouldBlock {
            ready.clear_ready();
            continue;
        }
        return Err(error);
    }
}

/// One datagram and its sender's pid (`SCM_CREDENTIALS`: journald passes the original sender's).
fn recv_with_pid(socket: &UnixDatagram, buf: &mut [u8]) -> io::Result<(usize, Option<u32>)> {
    #[repr(C)]
    union Control {
        buf: [u8; 64],
        _align: libc::cmsghdr,
    }
    let mut control = Control { buf: [0; 64] };
    let mut iov = libc::iovec { iov_base: buf.as_mut_ptr().cast(), iov_len: buf.len() };
    // SAFETY: msghdr is plain data; every pointer set below outlives the call.
    #[allow(unsafe_code)]
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    #[allow(unsafe_code)]
    {
        msg.msg_control = unsafe { control.buf.as_mut_ptr() }.cast();
        msg.msg_controllen = std::mem::size_of::<Control>() as _;
    }
    // SAFETY: a valid socket fd and msghdr.
    #[allow(unsafe_code)]
    let len = unsafe { libc::recvmsg(socket.as_raw_fd(), &mut msg, libc::MSG_DONTWAIT | libc::MSG_CMSG_CLOEXEC) };
    if len < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut pid = None;
    // SAFETY: walks the control messages the kernel wrote into `control`.
    #[allow(unsafe_code)]
    unsafe {
        let mut cmsg = libc::CMSG_FIRSTHDR(&msg);
        while !cmsg.is_null() {
            if (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_CREDENTIALS {
                let cred = std::ptr::read_unaligned(libc::CMSG_DATA(cmsg).cast::<libc::ucred>());
                pid = u32::try_from(cred.pid).ok().filter(|pid| *pid > 0);
            }
            cmsg = libc::CMSG_NXTHDR(&msg, cmsg);
        }
    }
    Ok((len as usize, pid))
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or_default()
}

fn level_for(priority: u8) -> &'static str {
    match priority {
        0..=3 => "error",
        4 => "warn",
        7 => "debug",
        _ => "info",
    }
}

/// A forwarded line, `<PRI>Mmm dd hh:mm:ss IDENT[PID]: MESSAGE`, from `pid`. The unit is the
/// sender's (its cgroup), else the identifier.
pub fn parse_forwarded(datagram: &[u8], pid: Option<u32>) -> Option<Entry> {
    let text = String::from_utf8_lossy(datagram);
    let text = text.trim_end_matches(['\n', '\0']);
    let rest = text.strip_prefix('<')?;
    let (pri, rest) = rest.split_once('>')?;
    let priority = (pri.parse::<u32>().ok()? & 7) as u8;
    // The timestamp is journald's `%h %e %T ` (16 bytes); a line without one starts at the identifier.
    let rest = match rest.get(..16) {
        Some(stamp) if stamp.as_bytes().get(15) == Some(&b' ') && stamp.as_bytes().get(9) == Some(&b':') => &rest[16..],
        _ => rest,
    };
    let (header, message) = rest.split_once(": ").unwrap_or(("", rest));
    let ident = header.split('[').next().unwrap_or_default();
    let header_pid = header.split_once('[').and_then(|(_, pid)| pid.trim_end_matches(']').parse().ok());
    let unit_of = |pid: Option<u32>| pid.and_then(host::unit_of).map(|unit| unit.trim_end_matches(".service").to_string());
    let mut pid = pid.or(header_pid);
    let mut unit = unit_of(pid);
    // journald passes its own credentials when the sender is already gone (`logger`): then the
    // line's own pid and identifier say who it was.
    if unit.as_deref() == Some("systemd-journald") && !ident.is_empty() && ident != "systemd-journald" {
        pid = header_pid;
        unit = unit_of(header_pid);
    }
    let unit = unit.unwrap_or_else(|| if ident.is_empty() { "unknown".into() } else { ident.to_string() });
    let line = LogLine { at_ms: now_ms(), level: level_for(priority), unit, message: host::strip_ansi(message), pid };
    Some(Entry::new(priority, line))
}

/// A `/dev/kmsg` record, `PRI,SEQ,USEC,FLAGS;MESSAGE` (continuation lines after it are dropped).
pub fn parse_kmsg(record: &[u8]) -> Option<Entry> {
    let text = String::from_utf8_lossy(record);
    let (header, rest) = text.split_once(';')?;
    let priority = (header.split(',').next()?.parse::<u32>().ok()? & 7) as u8;
    let message = rest.lines().next().unwrap_or_default();
    let line = LogLine { at_ms: now_ms(), level: level_for(priority), unit: "kernel".into(), message: host::strip_ansi(message), pid: None };
    Some(Entry::new(priority, line))
}

/// What one viewer follows: `journalctl -u UNIT --priority=N`, matched in-process.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Filter {
    /// A unit (`helios-engine.service`) or a glob of them (`helios-*.service`); all when absent.
    pub unit: Option<String>,
    pub max_priority: Option<u8>,
}

impl Filter {
    pub fn matches(&self, entry: &Entry) -> bool {
        if self.max_priority.is_some_and(|max| entry.priority > max) {
            return false;
        }
        let Some(unit) = &self.unit else { return true };
        let unit = unit.trim_end_matches(".service");
        if glob(unit, &entry.line.unit) {
            return true;
        }
        // `journalctl -u` also shows what systemd says about the unit ("helios-engine.service: ...").
        entry.line.pid == Some(1) && entry.line.message.split([' ', ':']).any(|word| glob(unit, word.trim_end_matches(".service")))
    }
}

/// `*` matches any run of characters; nothing else is special.
fn glob(pattern: &str, text: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = text.strip_prefix(first) else { return false };
    let parts: Vec<&str> = parts.collect();
    let Some((last, middle)) = parts.split_last() else { return rest.is_empty() };
    for part in middle {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.len() >= last.len() && rest.ends_with(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_forwarded_line_has_its_priority_identifier_and_message() {
        let entry = parse_forwarded(b"<28>Nov  7 17:21:59 helios-engine[4000000001]: camera stalled\n", None).expect("entry");
        assert_eq!(entry.priority, 4);
        assert_eq!(entry.line.level, "warn");
        assert_eq!(entry.line.pid, Some(4000000001));
        assert_eq!(entry.line.message, "camera stalled");
        // No such process here: the identifier names the unit.
        assert_eq!(entry.line.unit, "helios-engine");
        assert!(entry.json.contains("camera stalled"));
    }

    #[test]
    fn a_kernel_record_is_the_kernel_unit() {
        let entry = parse_kmsg(b"3,1234,5678901,-;usb 1-1: reset high-speed USB device\n SUBSYSTEM=usb\n").expect("entry");
        assert_eq!(entry.priority, 3);
        assert_eq!(entry.line.unit, "kernel");
        assert_eq!(entry.line.message, "usb 1-1: reset high-speed USB device");
    }

    #[test]
    fn filters_match_units_globs_systemd_lines_and_priorities() {
        let line =
            |unit: &str, pid: u32, message: &str, priority: u8| Entry::new(priority, LogLine { at_ms: 0, level: level_for(priority), unit: unit.into(), message: message.into(), pid: Some(pid) });
        let engine = Filter { unit: Some("helios-engine.service".into()), max_priority: None };
        assert!(engine.matches(&line("helios-engine", 10, "x", 6)));
        assert!(!engine.matches(&line("helios-api", 11, "x", 6)));
        assert!(engine.matches(&line("systemd", 1, "helios-engine.service: Deactivated successfully.", 6)));
        assert!(!engine.matches(&line("systemd", 1, "Started sshd.service.", 6)));
        let guest = Filter { unit: Some("helios-*.service".into()), max_priority: Some(4) };
        assert!(guest.matches(&line("helios-api", 11, "x", 3)));
        assert!(!guest.matches(&line("helios-api", 11, "x", 6)));
        assert!(!guest.matches(&line("orion-node", 12, "x", 3)));
        assert!(Filter::default().matches(&line("kernel", 0, "x", 7)));
    }

    #[tokio::test]
    async fn the_feed_reads_datagrams_with_the_senders_pid_and_removes_its_socket() {
        let dir = std::env::temp_dir().join(format!("helios-log-feed-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("syslog");
        let feed = Feed::bind(&path, false).expect("bind");
        // A second feed finds the socket in use.
        assert!(Feed::bind(&path, false).is_none());
        let mut receiver = feed.subscribe();
        let sender = std::os::unix::net::UnixDatagram::unbound().expect("socket");
        sender.send_to(b"<14>Nov  7 17:21:59 tester: hello feed", &path).expect("send");
        let entry = tokio::time::timeout(Duration::from_secs(2), receiver.recv()).await.expect("in time").expect("entry");
        assert_eq!(entry.line.message, "hello feed");
        assert_eq!(entry.line.pid, Some(std::process::id()));
        drop(feed);
        assert!(!path.exists());
        // A stale socket file (no one reading) is taken over.
        let stale = std::os::unix::net::UnixDatagram::bind(&path).expect("stale");
        drop(stale);
        assert!(Feed::bind(&path, false).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
