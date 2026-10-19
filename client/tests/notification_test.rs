//! Notification handling across the client transports.
//!
//! `McpClient::initialize` sends `notifications/initialized`, and the server answers a
//! request-shaped notification with nothing on stdio (`src/protocol/json_rpc.rs`) and an
//! empty 204 on HTTP (`src/transport/http.rs`). A transport that waits for a JSON-RPC
//! envelope therefore cannot complete the handshake on those two legs; the shared memory
//! transports are not affected because the shmem loop does reply.
//!
//! The stub tests need no server and run in CI. The server-backed tests are `#[ignore]`d
//! because they launch the built binary: `server_exe` looks for
//! `target/release/velocity_mcp[.exe]` in the workspace root above this crate, or set
//! `VELOCITY_MCP_SERVER` to point at a different build. Run them with:
//! `cargo test -p velocity-mcp-client --test notification_test -- --ignored`

use std::future::Future;
use std::time::Duration;
use velocity_mcp_client::{
    Error, HttpTransport, JsonRpcRequest, McpClient, StdioTransport, Transport,
};

/// The handshake leg that touches the transport's reply handling, wrapped so a transport
/// that waits forever for an envelope reports a failure instead of hanging the suite.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);

fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to build runtime")
        .block_on(future)
}

fn initialize(transport: impl Transport + 'static) -> Result<String, Error> {
    block_on(async {
        let mut client = McpClient::new(transport);
        match tokio::time::timeout(HANDSHAKE_TIMEOUT, client.initialize()).await {
            Ok(result) => result.map(|init| init.protocol_version),
            Err(_) => Err(Error::Server(format!(
                "initialize did not return within {}s",
                HANDSHAKE_TIMEOUT.as_secs()
            ))),
        }
    })
}

fn notification() -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "notifications/initialized".to_string(),
        params: None,
        id: None,
    }
}

fn request() -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "ping".to_string(),
        params: None,
        id: Some(serde_json::json!(7)),
    }
}

/// Answer exactly like the server does: an empty 204 for a body with no `"id"`, a JSON-RPC
/// envelope for everything else, one connection per request.
fn stub_server(listener: std::net::TcpListener) -> std::io::Result<()> {
    use std::io::{Read, Write};
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let mut raw = Vec::new();
        let mut byte = [0u8; 1];
        let mut head_len = None;
        while raw.len() < 64 * 1024 {
            if stream.read(&mut byte)? == 0 {
                break;
            }
            raw.push(byte[0]);
            if raw.ends_with(b"\r\n\r\n") {
                head_len = Some(raw.len());
                break;
            }
        }
        let Some(head_len) = head_len else { continue };

        let declared = String::from_utf8_lossy(&raw[..head_len])
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                if name.trim().eq_ignore_ascii_case("content-length") {
                    value.trim().parse::<usize>().ok()
                } else {
                    None
                }
            })
            .unwrap_or(0);
        while raw.len() < head_len + declared {
            if stream.read(&mut byte)? == 0 {
                break;
            }
            raw.push(byte[0]);
        }

        let response = if raw[head_len..].windows(4).any(|w| w == *b"\"id\"") {
            let payload = br#"{"jsonrpc":"2.0","id":7,"result":{}}"#;
            format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                payload.len()
            )
            .into_bytes()
            .into_iter()
            .chain(payload.iter().copied())
            .collect::<Vec<u8>>()
        } else {
            b"HTTP/1.1 204 No Content\r\nconnection: close\r\n\r\n".to_vec()
        };
        stream.write_all(&response)?;
        stream.flush()?;
    }
    Ok(())
}

fn http_transport(url: &str) -> HttpTransport {
    HttpTransport::new(url, None).expect("Failed to create transport")
}

fn bind_stub(
    handler: impl FnOnce(std::net::TcpListener) -> std::io::Result<()> + Send + 'static,
) -> u16 {
    let listener =
        std::net::TcpListener::bind("127.0.0.1:0").expect("Failed to bind the stub server");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let _ = handler(listener);
    });
    port
}

#[test]
fn http_transport_notification_has_no_envelope_to_decode() {
    let port = bind_stub(stub_server);
    let transport = http_transport(&format!("http://127.0.0.1:{}", port));

    // A notification is answered with an empty 204: no envelope, and not a failure either.
    match block_on(transport.send(notification())) {
        Ok(response) => assert!(
            response.error.is_none(),
            "notification reported as an error: {:?}",
            response.error
        ),
        Err(e) => panic!("notification rejected by the transport: {}", e),
    }

    // An id-bearing request still gets its envelope decoded.
    let response = block_on(transport.send(request())).expect("request");
    assert_eq!(response.id, Some(serde_json::json!(7)));
    assert!(response.result.is_some());
}

#[test]
fn http_transport_still_reports_a_failed_notification_status() {
    let port = bind_stub(|listener| {
        use std::io::{Read, Write};
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            // Read the request before rejecting it, so the reply is not racing the body.
            let mut raw = [0u8; 4096];
            let _ = stream.read(&mut raw);
            let _ = stream.write_all(
                b"HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
            );
        }
        Ok(())
    });

    match block_on(http_transport(&format!("http://127.0.0.1:{}", port)).send(notification())) {
        Err(Error::Server(_)) => {}
        other => panic!(
            "a rejected notification must stay an error, got {:?}",
            other
        ),
    }
}

mod server_backed {
    use super::*;
    use std::process::{Child, Command, Stdio};

    struct ServerGuard(Option<Child>);

    impl Drop for ServerGuard {
        fn drop(&mut self) {
            if let Some(mut child) = self.0.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    fn server_exe() -> String {
        std::env::var("VELOCITY_MCP_SERVER").unwrap_or_else(|_| {
            let manifest_dir = env!("CARGO_MANIFEST_DIR");
            let workspace = std::path::Path::new(manifest_dir).parent().unwrap();
            let exe = if cfg!(windows) {
                workspace.join("target\\release\\velocity_mcp.exe")
            } else {
                workspace.join("target/release/velocity_mcp")
            };
            exe.to_string_lossy().into_owned()
        })
    }

    fn spawn_http_server() -> (String, ServerGuard) {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .expect("Failed to find a free port")
            .local_addr()
            .unwrap()
            .port();
        let addr = format!("127.0.0.1:{}", port);
        let mut child = Command::new(server_exe())
            .args(["--mode", "http", "--addr", &addr])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to start the server. Set VELOCITY_MCP_SERVER to a built binary.");

        for _ in 0..100 {
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                // The server nests its JSON-RPC endpoint under the `/v1` prefix.
                let url = format!("http://127.0.0.1:{}/v1/mcp", port);
                return (url, ServerGuard(Some(child)));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = child.kill();
        let _ = child.wait();
        panic!("The HTTP server did not accept a connection within 10s");
    }

    #[test]
    #[ignore]
    fn http_initialize_completes_against_the_real_server() {
        let (url, _server) = spawn_http_server();
        let protocol_version = initialize(http_transport(&url)).expect("HTTP initialize failed");
        assert_eq!(protocol_version, "2024-11-05");
    }

    #[test]
    #[ignore]
    fn stdio_initialize_completes_against_the_real_server() {
        let transport = StdioTransport::new(&server_exe(), &["--mode", "stdio"])
            .expect("Failed to spawn the server over stdio");
        let protocol_version = initialize(transport).expect("stdio initialize failed");
        assert_eq!(protocol_version, "2024-11-05");
    }
}

/// A 204 for an id-bearing request is not a notification ack: the client asked for an
/// envelope and got none, so it is reported as a transport failure, the way the Python,
/// TypeScript and Go clients report the same shape.
#[test]
fn http_transport_reports_a_request_answered_without_an_envelope() {
    let port = bind_stub(|listener| {
        use std::io::{Read, Write};
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut raw = [0u8; 4096];
            let _ = stream.read(&mut raw);
            let _ = stream.write_all(b"HTTP/1.1 204 No Content\r\nconnection: close\r\n\r\n");
        }
        Ok(())
    });
    let transport = http_transport(&format!("http://127.0.0.1:{}", port));

    match block_on(transport.send(request())) {
        Err(Error::Transport(message)) => assert!(
            message.contains("No response envelope for request 7"),
            "unexpected message: {}",
            message
        ),
        Err(other) => panic!("want Error::Transport, got {:?}", other),
        Ok(_) => panic!("a request answered with an empty 204 reported success"),
    }

    // A notification on the same stub is still accepted, so the guard did not turn every
    // 204 into a failure.
    block_on(transport.send(notification())).expect("notification");
}
