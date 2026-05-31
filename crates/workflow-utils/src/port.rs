#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::process::Command;
#[cfg(target_os = "linux")]
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    time::Duration,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeOptions {
    pub endpoint: String,
    pub timeout: Duration,
}

impl Default for ProbeOptions {
    fn default() -> Self {
        Self {
            endpoint: "/.well-known/workflow/v1/flow?__health".to_owned(),
            timeout: Duration::from_millis(500),
        }
    }
}

pub fn get_all_ports() -> Vec<u16> {
    #[cfg(target_os = "linux")]
    {
        get_linux_ports(std::process::id())
    }
    #[cfg(target_os = "macos")]
    {
        get_darwin_ports(std::process::id())
    }
    #[cfg(target_os = "windows")]
    {
        get_windows_ports(std::process::id())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Vec::new()
    }
}

pub fn get_port() -> Option<u16> {
    get_all_ports().into_iter().next()
}

pub fn get_workflow_port(options: Option<&ProbeOptions>) -> Option<u16> {
    let ports = get_all_ports();
    if ports.is_empty() {
        return None;
    }
    if ports.len() == 1 {
        return ports.first().copied();
    }

    let default_options;
    let options = match options {
        Some(options) => options,
        None => {
            default_options = ProbeOptions::default();
            &default_options
        }
    };

    ports
        .iter()
        .copied()
        .find(|port| probe_port(*port, options))
        .or_else(|| ports.first().copied())
}

fn parse_port(value: &str, radix: u32) -> Option<u16> {
    let port = u32::from_str_radix(value, radix).ok()?;
    u16::try_from(port).ok()
}

#[cfg(target_os = "linux")]
fn get_linux_ports(pid: u32) -> Vec<u16> {
    let fd_path = format!("/proc/{pid}/fd");
    let mut fds = match fs::read_dir(fd_path) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let fd = entry.file_name().to_string_lossy().parse::<u32>().ok()?;
                Some((fd, entry.path()))
            })
            .collect::<Vec<_>>(),
        Err(_) => return Vec::new(),
    };
    fds.sort_by_key(|(fd, _)| *fd);

    let mut socket_inodes = Vec::new();
    let mut socket_inode_set = BTreeSet::new();
    for (_, path) in fds {
        let Ok(link) = fs::read_link(path) else {
            continue;
        };
        let link = link.to_string_lossy();
        let Some(inode) = link
            .strip_prefix("socket:[")
            .and_then(|value| value.strip_suffix(']'))
        else {
            continue;
        };
        socket_inodes.push(inode.to_owned());
        socket_inode_set.insert(inode.to_owned());
    }

    let mut inode_to_port = BTreeMap::new();
    for tcp_file in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let Ok(content) = fs::read_to_string(tcp_file) else {
            continue;
        };
        for line in content.lines().skip(1) {
            let parts = line.split_whitespace().collect::<Vec<_>>();
            if parts.len() < 10 || parts[3] != "0A" || !socket_inode_set.contains(parts[9]) {
                continue;
            }
            let Some(port_hex) = parts[1].split(':').next_back() else {
                continue;
            };
            if let Some(port) = parse_port(port_hex, 16) {
                inode_to_port.insert(parts[9].to_owned(), port);
            }
        }
    }

    socket_inodes
        .into_iter()
        .filter_map(|inode| inode_to_port.get(&inode).copied())
        .collect()
}

#[cfg(target_os = "macos")]
fn get_darwin_ports(pid: u32) -> Vec<u16> {
    let Ok(output) = Command::new("lsof")
        .args(["-a", "-i", "-P", "-n", "-p", &pid.to_string()])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut ports = Vec::new();
    for line in stdout.lines() {
        if !line.contains("LISTEN") {
            continue;
        }
        let parts = line.split_whitespace().collect::<Vec<_>>();
        let Some(address) = parts.get(8) else {
            continue;
        };
        let Some(port_text) = address.rsplit(':').next() else {
            continue;
        };
        if let Some(port) = parse_port(port_text, 10) {
            ports.push(port);
        }
    }
    ports
}

#[cfg(target_os = "windows")]
fn get_windows_ports(pid: u32) -> Vec<u16> {
    let Ok(output) = Command::new("cmd")
        .args([
            "/c",
            &format!("netstat -ano | findstr {pid} | findstr LISTENING"),
        ])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter_map(|line| {
            let parts = line.split_whitespace().collect::<Vec<_>>();
            if parts.len() < 2 || !parts[0].eq_ignore_ascii_case("TCP") {
                return None;
            }
            let local = parts[1];
            let port_text = local.rsplit(':').next()?;
            parse_port(port_text, 10)
        })
        .collect()
}

fn probe_port(port: u16, options: &ProbeOptions) -> bool {
    let addresses = match ("127.0.0.1", port).to_socket_addrs() {
        Ok(addresses) => addresses.collect::<Vec<_>>(),
        Err(_) => return false,
    };

    addresses
        .into_iter()
        .any(|address| probe_address(address, &options.endpoint, options.timeout))
}

fn probe_address(address: SocketAddr, endpoint: &str, timeout: Duration) -> bool {
    let Ok(mut stream) = TcpStream::connect_timeout(&address, timeout) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));

    let request =
        format!("HEAD {endpoint} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }

    let mut response = [0_u8; 64];
    let Ok(read) = stream.read(&mut response) else {
        return false;
    };
    let response = String::from_utf8_lossy(&response[..read]);
    response.starts_with("HTTP/1.1 200") || response.starts_with("HTTP/1.0 200")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        },
        thread::{self, JoinHandle},
        time::Instant,
    };

    static PORT_TEST_LOCK: Mutex<()> = Mutex::new(());

    struct TestServer {
        port: u16,
        stop: Arc<AtomicBool>,
        handle: Option<JoinHandle<()>>,
    }

    impl TestServer {
        fn new(response: fn(&str) -> Option<(u16, &'static str)>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let port = listener.local_addr().unwrap().port();
            let stop = Arc::new(AtomicBool::new(false));
            let stop_thread = Arc::clone(&stop);
            let handle = thread::spawn(move || {
                while !stop_thread.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let mut request = [0_u8; 1024];
                            let read = stream.read(&mut request).unwrap_or(0);
                            let request = String::from_utf8_lossy(&request[..read]);
                            if let Some((status, body)) = response(&request) {
                                let response = format!(
                                    "HTTP/1.1 {status} OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                    body.len()
                                );
                                let _ = stream.write_all(response.as_bytes());
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(_) => break,
                    }
                }
            });
            Self {
                port,
                stop,
                handle: Some(handle),
            }
        }

        fn port(&self) -> u16 {
            self.port
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            let _ = TcpStream::connect(("127.0.0.1", self.port));
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    fn workflow_response(request: &str) -> Option<(u16, &'static str)> {
        if request.contains("__health") {
            Some((200, "Workflow SDK endpoint is healthy"))
        } else if request.contains("/.well-known/workflow/v1/") {
            Some((400, "{\"error\":\"Missing required headers\"}"))
        } else {
            Some((404, ""))
        }
    }

    fn not_workflow_response(_: &str) -> Option<(u16, &'static str)> {
        Some((404, ""))
    }

    fn slow_response(_: &str) -> Option<(u16, &'static str)> {
        None
    }

    #[test]
    fn upstream_get_port_cases() {
        let _guard = PORT_TEST_LOCK.lock().unwrap();

        assert_eq!(get_port(), None);

        let specific_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let specific_port = specific_listener.local_addr().unwrap().port();
        assert_eq!(get_port(), Some(specific_port));
        drop(specific_listener);

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let listener_port = listener.local_addr().unwrap().port();
        assert_eq!(get_port(), Some(listener_port));
        drop(listener);

        let first = TcpListener::bind("127.0.0.1:0").unwrap();
        let second = TcpListener::bind("127.0.0.1:0").unwrap();
        let first_port = first.local_addr().unwrap().port();
        assert_eq!(get_port(), Some(first_port));
        drop(second);
        drop(first);

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let listener_port = listener.local_addr().unwrap().port();
        assert_eq!(get_port(), Some(listener_port));
        assert_eq!(get_port(), Some(listener_port));
        assert_eq!(get_port(), Some(listener_port));
        drop(listener);

        if let Ok(ipv6_listener) = TcpListener::bind("[::1]:0") {
            let ipv6_port = ipv6_listener.local_addr().unwrap().port();
            assert_eq!(get_port(), Some(ipv6_port));
            drop(ipv6_listener);
        }

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let listener_port = listener.local_addr().unwrap().port();
        assert_eq!(get_port(), Some(listener_port));
        assert_eq!(get_port(), Some(listener_port));
        drop(listener);

        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let closed_port = closed.local_addr().unwrap().port();
        drop(closed);
        assert_ne!(get_port(), Some(closed_port));

        let restart = TcpListener::bind("127.0.0.1:0").unwrap();
        let restart_port = restart.local_addr().unwrap().port();
        assert_eq!(get_port(), Some(restart_port));
        drop(restart);
        let rebound = TcpListener::bind(("127.0.0.1", restart_port)).unwrap();
        assert_eq!(get_port(), Some(restart_port));
        drop(rebound);

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let listener_port = listener.local_addr().unwrap().port();
        let results = (0..10).map(|_| get_port()).collect::<Vec<_>>();
        assert!(results.into_iter().all(|port| port == Some(listener_port)));
        drop(listener);
    }

    #[test]
    fn upstream_get_all_ports_cases() {
        let _guard = PORT_TEST_LOCK.lock().unwrap();

        assert_eq!(get_all_ports(), Vec::<u16>::new());

        let first = TcpListener::bind("127.0.0.1:0").unwrap();
        let second = TcpListener::bind("127.0.0.1:0").unwrap();
        let first_port = first.local_addr().unwrap().port();
        let second_port = second.local_addr().unwrap().port();
        let ports = get_all_ports();
        assert!(ports.contains(&first_port));
        assert!(ports.contains(&second_port));
        assert!(ports.len() >= 2);

        let ports1 = get_all_ports();
        let ports2 = get_all_ports();
        let ports3 = get_all_ports();
        assert_eq!(ports1, ports2);
        assert_eq!(ports2, ports3);
        drop(second);
        drop(first);
    }

    #[test]
    fn upstream_get_workflow_port_cases() {
        let _guard = PORT_TEST_LOCK.lock().unwrap();

        assert_eq!(get_workflow_port(None), None);

        let single = TcpListener::bind("127.0.0.1:0").unwrap();
        let single_port = single.local_addr().unwrap().port();
        assert_eq!(get_workflow_port(None), Some(single_port));
        drop(single);

        let non_workflow = TestServer::new(not_workflow_response);
        let workflow = TestServer::new(workflow_response);
        assert_eq!(get_workflow_port(None), Some(workflow.port()));
        drop(workflow);
        drop(non_workflow);

        let first = TestServer::new(not_workflow_response);
        let second = TestServer::new(not_workflow_response);
        assert_eq!(get_workflow_port(None), Some(first.port()));
        drop(second);
        drop(first);

        let slow = TestServer::new(slow_response);
        let fast = TestServer::new(workflow_response);
        let options = ProbeOptions {
            timeout: Duration::from_millis(100),
            ..ProbeOptions::default()
        };
        let start = Instant::now();
        assert_eq!(get_workflow_port(Some(&options)), Some(fast.port()));
        assert!(start.elapsed() < Duration::from_secs(2));
        drop(fast);
        drop(slow);

        let workflow = TestServer::new(workflow_response);
        let results = (0..5).map(|_| get_workflow_port(None)).collect::<Vec<_>>();
        assert!(
            results
                .into_iter()
                .all(|port| port == Some(workflow.port()))
        );
    }
}
