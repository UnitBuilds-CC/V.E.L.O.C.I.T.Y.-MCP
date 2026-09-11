//! WASI socket networking for WASM runtimes (prototype).
//!
//! Provides real TCP socket operations behind the `wasm-networking` feature flag.
//! Without the flag, sock_* functions remain stubbed (errno 28/ENOSYS).
//!
//! # Architecture
//!
//! WASI's `wasi_snapshot_preview1` defines `sock_recv`/`sock_send`/`sock_shutdown`
//! but has no standard way to *create* a socket connection. WASIX adds this, but
//! pulls in heavy dependencies (Tokio runtime, full filesystem). Instead, we provide
//! a lightweight custom host function `velocity_net::tcp_connect` that creates a
//! TCP connection and returns a fd, then the standard WASI sock_* functions operate
//! on that fd.
//!
//! # Security
//!
//! Socket access is opt-in per runtime instance. The seccomp filter on Linux must
//! also allow socket syscalls (handled automatically when `wasm-networking` is enabled).
//! Connection targets can be restricted via an allowlist configured on `WasiSocketState`.

use std::collections::HashMap;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

pub const SOCKET_FD_BASE: i32 = 1000;
const ERRNO_ENOSYS: i32 = 52;
pub const ERRNO_BADF: i32 = 8;
const ERRNO_CONNREFUSED: i32 = 61;
const ERRNO_CONNABORTED: i32 = 62;
const ERRNO_TIMEDOUT: i32 = 73;
const DEFAULT_CONNECT_TIMEOUT_MS: u64 = 5000;

/// Socket state tracked per WASM instance.
/// Maps virtual fd numbers (starting at SOCKET_FD_BASE) to host TCP streams and listeners.
#[derive(Default)]
pub struct WasiSocketState {
    pub sockets: HashMap<i32, TcpStream>,
    pub listeners: HashMap<i32, TcpListener>,
    next_fd: i32,
    allowed_hosts: Option<Vec<String>>,
    connect_timeout: Duration,
}

impl WasiSocketState {
    pub fn new() -> Self {
        Self {
            sockets: HashMap::new(),
            listeners: HashMap::new(),
            next_fd: SOCKET_FD_BASE,
            allowed_hosts: None,
            connect_timeout: Duration::from_millis(DEFAULT_CONNECT_TIMEOUT_MS),
        }
    }

    /// Restrict connections to these hosts. None means allow all.
    pub fn with_allowed_hosts(mut self, hosts: Vec<String>) -> Self {
        self.allowed_hosts = Some(hosts);
        self
    }

    fn is_host_allowed(&self, host: &str) -> bool {
        match &self.allowed_hosts {
            None => true,
            Some(allowed) => allowed.iter().any(|h| h == host),
        }
    }

    fn alloc_fd(&mut self) -> i32 {
        let fd = self.next_fd;
        self.next_fd += 1;
        fd
    }

    /// Set the TCP connect timeout.
    pub fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    pub fn connect_tcp(&mut self, host: &str, port: u16) -> Result<i32, i32> {
        if !self.is_host_allowed(host) {
            return Err(ERRNO_CONNREFUSED);
        }

        let addr = format!("{}:{}", host, port);
        let stream = TcpStream::connect_timeout(
            &addr.parse().unwrap_or_else(|_| {
                use std::net::ToSocketAddrs;
                addr.to_socket_addrs()
                    .ok()
                    .and_then(|mut a| a.next())
                    .unwrap_or_else(|| SocketAddr::from(([0, 0, 0, 0], port)))
            }),
            self.connect_timeout,
        )
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::ConnectionRefused => ERRNO_CONNREFUSED,
            std::io::ErrorKind::TimedOut => ERRNO_TIMEDOUT,
            _ => ERRNO_CONNABORTED,
        })?;

        stream.set_nonblocking(true).map_err(|_| ERRNO_CONNABORTED)?;

        let fd = self.alloc_fd();
        self.sockets.insert(fd, stream);
        Ok(fd)
    }

    pub fn listen_tcp(&mut self, host: &str, port: u16) -> Result<i32, i32> {
        if !self.is_host_allowed(host) {
            return Err(ERRNO_CONNREFUSED);
        }

        let addr = format!("{}:{}", host, port);
        let listener = TcpListener::bind(&addr).map_err(|e| match e.kind() {
            std::io::ErrorKind::AddrInUse => ERRNO_CONNABORTED,
            std::io::ErrorKind::PermissionDenied => ERRNO_CONNREFUSED,
            _ => ERRNO_CONNABORTED,
        })?;

        listener.set_nonblocking(true).map_err(|_| ERRNO_CONNABORTED)?;

        let fd = self.alloc_fd();
        self.listeners.insert(fd, listener);
        Ok(fd)
    }

    pub fn accept_tcp(&mut self, listener_fd: i32) -> Result<i32, i32> {
        let listener = self.listeners.get(&listener_fd).ok_or(ERRNO_BADF)?;
        let (stream, _addr) = listener.accept().map_err(|e| match e.kind() {
            std::io::ErrorKind::WouldBlock => ERRNO_ENOSYS,
            _ => ERRNO_CONNABORTED,
        })?;

        stream.set_nonblocking(true).map_err(|_| ERRNO_CONNABORTED)?;

        let fd = self.alloc_fd();
        self.sockets.insert(fd, stream);
        Ok(fd)
    }

    pub fn close(&mut self, fd: i32) -> bool {
        self.sockets.remove(&fd).is_some() || self.listeners.remove(&fd).is_some()
    }

    pub fn is_socket_fd(&self, fd: i32) -> bool {
        self.sockets.contains_key(&fd)
    }

    pub fn is_listener_fd(&self, fd: i32) -> bool {
        self.listeners.contains_key(&fd)
    }
}

impl Clone for WasiSocketState {
    fn clone(&self) -> Self {
        Self {
            sockets: HashMap::new(),
            listeners: HashMap::new(),
            next_fd: self.next_fd,
            allowed_hosts: self.allowed_hosts.clone(),
            connect_timeout: self.connect_timeout,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn test_socket_state_alloc_fd() {
        let mut state = WasiSocketState::new();
        let fd1 = state.alloc_fd();
        let fd2 = state.alloc_fd();
        assert_eq!(fd1, SOCKET_FD_BASE);
        assert_eq!(fd2, SOCKET_FD_BASE + 1);
    }

    #[test]
    fn test_socket_state_allowed_hosts() {
        let state = WasiSocketState::new().with_allowed_hosts(vec!["example.com".into()]);
        assert!(state.is_host_allowed("example.com"));
        assert!(!state.is_host_allowed("evil.com"));
    }

    #[test]
    fn test_socket_state_no_restrictions() {
        let state = WasiSocketState::new();
        assert!(state.is_host_allowed("anything.com"));
    }

    #[test]
    fn test_socket_state_close() {
        let mut state = WasiSocketState::new();
        let fd = state.alloc_fd();
        assert!(!state.close(fd));
    }

    #[test]
    fn test_connect_blocked_host() {
        let mut state = WasiSocketState::new().with_allowed_hosts(vec!["allowed.com".into()]);
        let result = state.connect_tcp("blocked.com", 80);
        assert_eq!(result, Err(ERRNO_CONNREFUSED));
    }

    #[test]
    fn test_listen_connect_accept_send_recv() {
        let mut state = WasiSocketState::new();

        let listener_fd = state.listen_tcp("127.0.0.1", 0).expect("listen failed");
        assert!(state.is_listener_fd(listener_fd));
        assert!(!state.is_socket_fd(listener_fd));

        let port = {
            let listener = state.listeners.get(&listener_fd).unwrap();
            listener.local_addr().unwrap().port()
        };

        let client_fd = state.connect_tcp("127.0.0.1", port).expect("connect failed");
        assert!(state.is_socket_fd(client_fd));

        let server_fd = state.accept_tcp(listener_fd).expect("accept failed");
        assert!(state.is_socket_fd(server_fd));
        assert_ne!(client_fd, server_fd);

        let msg = b"hello from WASM";
        {
            let stream = state.sockets.get_mut(&client_fd).unwrap();
            stream.set_nonblocking(false).unwrap();
            stream.write_all(msg).unwrap();
        }

        std::thread::sleep(std::time::Duration::from_millis(50));

        {
            let stream = state.sockets.get_mut(&server_fd).unwrap();
            stream.set_nonblocking(false).unwrap();
            let mut buf = [0u8; 64];
            let n = stream.read(&mut buf).unwrap();
            assert_eq!(&buf[..n], msg);
        }

        let msg2 = b"hello from server";
        {
            let stream = state.sockets.get_mut(&server_fd).unwrap();
            stream.write_all(msg2).unwrap();
        }

        std::thread::sleep(std::time::Duration::from_millis(50));

        {
            let stream = state.sockets.get_mut(&client_fd).unwrap();
            let mut buf = [0u8; 64];
            let n = stream.read(&mut buf).unwrap();
            assert_eq!(&buf[..n], msg2);
        }
    }

    #[test]
    fn test_close_socket_and_listener() {
        let mut state = WasiSocketState::new();

        let listener_fd = state.listen_tcp("127.0.0.1", 0).unwrap();
        assert!(state.is_listener_fd(listener_fd));
        assert!(state.close(listener_fd));
        assert!(!state.is_listener_fd(listener_fd));
        assert!(!state.close(listener_fd));

        let listener_fd2 = state.listen_tcp("127.0.0.1", 0).unwrap();
        let port = {
            let listener = state.listeners.get(&listener_fd2).unwrap();
            listener.local_addr().unwrap().port()
        };
        let client_fd = state.connect_tcp("127.0.0.1", port).unwrap();
        assert!(state.is_socket_fd(client_fd));
        assert!(state.close(client_fd));
        assert!(!state.is_socket_fd(client_fd));
        assert!(!state.close(client_fd));

        state.close(listener_fd2);
    }

    #[test]
    fn test_connect_timeout() {
        let mut state = WasiSocketState::new()
            .with_connect_timeout(Duration::from_millis(100));

        let result = state.connect_tcp("192.0.2.1", 80);
        assert!(result.is_err());
    }

    #[test]
    fn test_connect_refused() {
        let mut state = WasiSocketState::new();
        let result = state.connect_tcp("127.0.0.1", 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_accept_on_invalid_fd() {
        let mut state = WasiSocketState::new();
        let result = state.accept_tcp(9999);
        assert_eq!(result, Err(ERRNO_BADF));
    }

    #[test]
    fn test_accept_on_socket_fd_fails() {
        let mut state = WasiSocketState::new();
        let listener_fd = state.listen_tcp("127.0.0.1", 0).unwrap();
        let port = {
            let listener = state.listeners.get(&listener_fd).unwrap();
            listener.local_addr().unwrap().port()
        };
        let client_fd = state.connect_tcp("127.0.0.1", port).unwrap();

        let result = state.accept_tcp(client_fd);
        assert_eq!(result, Err(ERRNO_BADF));

        state.close(client_fd);
        state.close(listener_fd);
    }

    #[test]
    fn test_multiple_connections() {
        let mut state = WasiSocketState::new();
        let listener_fd = state.listen_tcp("127.0.0.1", 0).unwrap();
        let port = {
            let listener = state.listeners.get(&listener_fd).unwrap();
            listener.local_addr().unwrap().port()
        };

        let fd1 = state.connect_tcp("127.0.0.1", port).unwrap();
        let fd2 = state.connect_tcp("127.0.0.1", port).unwrap();
        let fd3 = state.connect_tcp("127.0.0.1", port).unwrap();

        let acc1 = state.accept_tcp(listener_fd).unwrap();
        let acc2 = state.accept_tcp(listener_fd).unwrap();
        let acc3 = state.accept_tcp(listener_fd).unwrap();

        assert_eq!(state.sockets.len(), 6);
        assert!(state.is_listener_fd(listener_fd));

        for fd in [fd1, fd2, fd3, acc1, acc2, acc3] {
            assert!(state.close(fd));
        }
        assert!(state.close(listener_fd));
        assert_eq!(state.sockets.len(), 0);
    }

    #[test]
    fn test_is_socket_fd_and_is_listener_fd() {
        let mut state = WasiSocketState::new();
        assert!(!state.is_socket_fd(0));
        assert!(!state.is_listener_fd(0));
        assert!(!state.is_socket_fd(SOCKET_FD_BASE));
        assert!(!state.is_listener_fd(SOCKET_FD_BASE));

        let listener_fd = state.listen_tcp("127.0.0.1", 0).unwrap();
        assert!(state.is_listener_fd(listener_fd));
        assert!(!state.is_socket_fd(listener_fd));

        let port = {
            let listener = state.listeners.get(&listener_fd).unwrap();
            listener.local_addr().unwrap().port()
        };
        let client_fd = state.connect_tcp("127.0.0.1", port).unwrap();
        assert!(state.is_socket_fd(client_fd));
        assert!(!state.is_listener_fd(client_fd));

        state.close(client_fd);
        state.close(listener_fd);
    }

    #[test]
    fn test_default_connect_timeout() {
        let state = WasiSocketState::new();
        assert_eq!(state.connect_timeout, Duration::from_millis(DEFAULT_CONNECT_TIMEOUT_MS));
    }

    #[test]
    fn test_with_connect_timeout_builder() {
        let state = WasiSocketState::new().with_connect_timeout(Duration::from_secs(10));
        assert_eq!(state.connect_timeout, Duration::from_secs(10));
    }
}
