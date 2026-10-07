//! Whether a process is the one listening on an address: the proof, taken
//! from the operating system and not from the answer, that the copy which
//! answered `GET /instance` is the process it names. Any local program can
//! listen on a free port and answer with another program's process id; that
//! id must never get stopped or ended.
//!
//! Windows reads the TCP listener table with its owning process ids
//! (`GetExtendedTcpTable`). Linux finds the listening socket's inode in
//! `/proc/net/tcp` or `/proc/net/tcp6` and requires it among the process's
//! open files (`/proc/{pid}/fd`), which only that process's own user (or
//! root) may read. Elsewhere nothing is ever confirmed.

use std::net::SocketAddr;

/// Whether `pid` alone listens on `address` (TCP, that exact address and
/// port): at least one listening socket there, and every one of them is
/// `pid`'s. `false` whenever it cannot be told.
pub fn owned_by(pid: u32, address: SocketAddr) -> bool {
    #[cfg(windows)]
    {
        super::win32::listening_pids(address).is_ok_and(|owners| sole_owner(&owners, pid))
    }
    #[cfg(target_os = "linux")]
    {
        proc::owned_by(pid, address)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (pid, address);
        false
    }
}

/// Whether `owners` (the owner of each listening socket on one address) is
/// not empty and all `pid`.
#[cfg_attr(not(windows), allow(dead_code))]
fn sole_owner(owners: &[u32], pid: u32) -> bool {
    !owners.is_empty() && owners.iter().all(|&owner| owner == pid)
}

/// `/proc` parsing (pure, tested on every platform) and, on Linux, the
/// lookups themselves.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod proc {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

    /// The `st` column of a listening socket (`TCP_LISTEN`).
    const LISTEN: &str = "0A";

    /// The inodes of the sockets listening on exactly `address` in the text
    /// of `/proc/net/tcp` (IPv4) or `/proc/net/tcp6` (IPv6). Columns:
    /// `sl local_address rem_address st tx:rx tr:when retrnsmt uid timeout
    /// inode ...`; the first line is the header.
    pub fn listening_inodes(table: &str, address: SocketAddr) -> Vec<u64> {
        table
            .lines()
            .skip(1)
            .filter_map(|line| {
                let fields: Vec<&str> = line.split_whitespace().collect();
                let (local, state, inode) = (fields.get(1)?, fields.get(3)?, fields.get(9)?);
                if *state != LISTEN || local_address(local)? != address {
                    return None;
                }
                inode.parse::<u64>().ok().filter(|&inode| inode != 0)
            })
            .collect()
    }

    /// `ADDRESS:PORT` as `/proc/net/tcp{,6}` prints it: the address as one
    /// (IPv4) or four (IPv6) 32-bit words, each in the host's byte order
    /// (little-endian on every platform the app ships for), and the port as
    /// a plain hexadecimal number.
    pub fn local_address(text: &str) -> Option<SocketAddr> {
        let (address, port) = text.split_once(':')?;
        if port.len() != 4 {
            return None;
        }
        let port = u16::from_str_radix(port, 16).ok()?;
        let ip = match address.len() {
            8 => IpAddr::V4(Ipv4Addr::from(word(address)?.to_ne_bytes())),
            32 => {
                let mut octets = [0u8; 16];
                for (index, chunk) in octets.chunks_mut(4).enumerate() {
                    let hex = address.get(index * 8..index * 8 + 8)?;
                    chunk.copy_from_slice(&word(hex)?.to_ne_bytes());
                }
                IpAddr::V6(Ipv6Addr::from(octets))
            }
            _ => return None,
        };
        Some(SocketAddr::new(ip, port))
    }

    /// Eight hexadecimal digits as a number.
    fn word(hex: &str) -> Option<u32> {
        if hex.len() != 8 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        u32::from_str_radix(hex, 16).ok()
    }

    /// The inode of a `/proc/{pid}/fd/*` link that names a socket
    /// (`socket:[12345]`).
    pub fn socket_inode(link: &str) -> Option<u64> {
        link.strip_prefix("socket:[")?
            .strip_suffix(']')?
            .parse()
            .ok()
    }

    /// Whether `pid` alone holds the sockets listening on `address`.
    #[cfg(target_os = "linux")]
    pub fn owned_by(pid: u32, address: SocketAddr) -> bool {
        use std::collections::HashSet;

        let table = match address {
            SocketAddr::V4(_) => "/proc/net/tcp",
            SocketAddr::V6(_) => "/proc/net/tcp6",
        };
        let Ok(text) = std::fs::read_to_string(table) else {
            return false;
        };
        let listening = listening_inodes(&text, address);
        if listening.is_empty() {
            return false;
        }
        // Another user's process: its open files cannot be read, so it is
        // not confirmed.
        let Ok(entries) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
            return false;
        };
        let held: HashSet<u64> = entries
            .filter_map(|entry| {
                let link = std::fs::read_link(entry.ok()?.path()).ok()?;
                socket_inode(link.to_str()?)
            })
            .collect();
        listening.iter().all(|inode| held.contains(inode))
    }
}

#[cfg(test)]
mod tests {
    use super::proc::{listening_inodes, local_address, socket_inode};
    use super::*;

    /// `/proc/net/tcp` with a listener on 127.0.0.1:8080 (inode 4242), one on
    /// 0.0.0.0:8080 (4343), an established connection to 127.0.0.1:8080
    /// (4444) and a listener on 127.0.0.1:9090 (4545).
    const TCP: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 4242 1 0000000000000000 100 0 0 10 0
   1: 00000000:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 4343 1 0000000000000000 100 0 0 10 0
   2: 0100007F:1F90 0100007F:D431 01 00000000:00000000 00:00000000 00000000  1000        0 4444 1 0000000000000000 20 4 30 10 -1
   3: 0100007F:2382 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 4545 1 0000000000000000 100 0 0 10 0
";

    /// `/proc/net/tcp6` with a listener on [::1]:8080 (5151) and one on
    /// [::]:8080 (5252).
    const TCP6: &str = "  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000000000000000000001000000:1F90 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 5151 1 0000000000000000 100 0 0 10 0
   1: 00000000000000000000000000000000:1F90 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 5252 1 0000000000000000 100 0 0 10 0
";

    fn at(address: &str) -> SocketAddr {
        address.parse().unwrap()
    }

    #[test]
    fn proc_addresses_are_host_order_words() {
        assert_eq!(local_address("0100007F:1F90"), Some(at("127.0.0.1:8080")));
        assert_eq!(local_address("0A01A8C0:0050"), Some(at("192.168.1.10:80")));
        assert_eq!(
            local_address("00000000000000000000000001000000:1F90"),
            Some(at("[::1]:8080"))
        );
        assert_eq!(
            local_address("B80D0120000000000000000001000000:01BB"),
            Some(at("[2001:db8::1]:443"))
        );
        for bad in [
            "",
            "0100007F",
            "0100007F:",
            "0100007F:1F9",
            "0100007F:1F900",
            "0100007G:1F90",
            "100007F:1F90",
            "0000000000000000000000000100000:1F90",
            "+100007F:1F90",
        ] {
            assert_eq!(local_address(bad), None, "{bad}");
        }
    }

    #[test]
    fn only_listeners_on_the_exact_address_count() {
        assert_eq!(listening_inodes(TCP, at("127.0.0.1:8080")), [4242]);
        assert_eq!(listening_inodes(TCP, at("0.0.0.0:8080")), [4343]);
        assert_eq!(listening_inodes(TCP, at("127.0.0.1:9090")), [4545]);
        assert!(listening_inodes(TCP, at("127.0.0.1:8081")).is_empty());
        assert!(listening_inodes(TCP, at("127.0.0.2:8080")).is_empty());
        assert_eq!(listening_inodes(TCP6, at("[::1]:8080")), [5151]);
        assert_eq!(listening_inodes(TCP6, at("[::]:8080")), [5252]);
        assert!(listening_inodes(TCP6, at("[::1]:9090")).is_empty());
        // The header, blank lines and short or garbled rows are skipped.
        assert!(listening_inodes("", at("127.0.0.1:8080")).is_empty());
        let garbled =
            "header\n\n   0: 0100007F:1F90 00000000:0000 0A\n   1: zz:1F90 0 0A 0 0 0 0 0 77\n";
        assert!(listening_inodes(garbled, at("127.0.0.1:8080")).is_empty());
    }

    #[test]
    fn socket_links_name_their_inode() {
        assert_eq!(socket_inode("socket:[4242]"), Some(4242));
        for other in [
            "pipe:[4242]",
            "socket:[]",
            "socket:[42x]",
            "socket:4242",
            "/dev/null",
            "anon_inode:[eventpoll]",
        ] {
            assert_eq!(socket_inode(other), None, "{other}");
        }
    }

    #[test]
    fn every_listener_must_be_the_named_process() {
        assert!(sole_owner(&[42], 42));
        assert!(sole_owner(&[42, 42], 42));
        assert!(!sole_owner(&[], 42));
        assert!(!sole_owner(&[43], 42));
        // A second listener on the same address (port sharing) is another
        // process's: not confirmed.
        assert!(!sole_owner(&[42, 43], 42));
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn this_process_is_found_listening_where_it_listens() {
        let own = std::process::id();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        assert!(owned_by(own, address), "{address}");
        // Another process (not `own + 1`: on Linux that may be one of this
        // process's threads, which share its open files).
        assert!(!owned_by(1, address));
        // The same port on another address, and nothing listening.
        let elsewhere = SocketAddr::new("127.0.0.2".parse().unwrap(), address.port());
        assert!(!owned_by(own, elsewhere));
        // A connected socket is not a listener.
        let client = std::net::TcpStream::connect(address).unwrap();
        assert!(!owned_by(own, client.local_addr().unwrap()));
        drop(client);
        drop(listener);
        assert!(!owned_by(own, address));
        if let Ok(v6) = std::net::TcpListener::bind("[::1]:0") {
            let address = v6.local_addr().unwrap();
            assert!(owned_by(own, address), "{address}");
            assert!(!owned_by(
                own,
                SocketAddr::new("::".parse().unwrap(), address.port())
            ));
        }
    }
}
