//! Integration tests for shared memory transports.
//!
//! These tests require a running VELOCITY-MCP server with shmem enabled.
//! Run with: `cargo test --test shmem_integration -- --ignored`
//!
//! Or set VELOCITY_SHMEM_TEST=1 to run automatically.

#[cfg(target_os = "windows")]
mod shmem_tests {
    use velocity_mcp_client::{JsonShmemTransport, JsonRpcRequest, McpClient, ShmemTransport, Transport};
    use std::process::{Child, Command, Stdio};
    use std::time::Duration;

    struct ServerGuard {
        child: Option<Child>,
        buffer_path: Option<String>,
    }

    impl ServerGuard {
        fn kill(&mut self) {
            if let Some(mut child) = self.child.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    impl Drop for ServerGuard {
        fn drop(&mut self) {
            self.kill();
            if let Some(path) = self.buffer_path.take() {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    fn spawn_server(buffer_path: &str) -> ServerGuard {
        let server_exe = std::env::var("VELOCITY_MCP_SERVER")
            .unwrap_or_else(|_| {
                let manifest_dir = env!("CARGO_MANIFEST_DIR");
                let workspace = std::path::Path::new(manifest_dir).parent().unwrap();
                if cfg!(windows) {
                    workspace.join("target\\release\\velocity_mcp.exe").to_string_lossy().into_owned()
                } else {
                    workspace.join("target/release/velocity_mcp").to_string_lossy().into_owned()
                }
            });

        let _ = std::fs::remove_file(buffer_path);

        let child = Command::new(&server_exe)
            .arg("--mode")
            .arg("shmem")
            .arg("--buffer-path")
            .arg(buffer_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to start server. Set VELOCITY_MCP_SERVER env var.");

        ServerGuard {
            child: Some(child),
            buffer_path: Some(buffer_path.to_string()),
        }
    }

    fn wait_for_buffer(path: &str) {
        for _ in 0..500 {
            if std::path::Path::new(path).exists() {
                std::thread::sleep(Duration::from_millis(300));
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("Server did not create buffer file '{}' within 5s", path);
    }

    #[tokio::test]
    #[ignore]
    async fn test_nda_shmem_transport_initialize() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let buffer_path = format!("test_nda_init_{}.bin", ts);

        let _server = spawn_server(&buffer_path);
        wait_for_buffer(&buffer_path);

        let transport = ShmemTransport::new(&buffer_path).expect("Failed to create transport");
        let mut client = McpClient::new(transport);

        let result = client.initialize().await.expect("Initialize failed");
        assert_eq!(result.protocol_version, "2024-11-05");
        assert!(!result.server_info.name.is_empty());
    }

    #[tokio::test]
    #[ignore]
    async fn test_nda_shmem_transport_list_tools() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let buffer_path = format!("test_nda_tools_{}.bin", ts);

        let _server = spawn_server(&buffer_path);
        wait_for_buffer(&buffer_path);

        let transport = ShmemTransport::new(&buffer_path).expect("Failed to create transport");
        let mut client = McpClient::new(transport);
        client.initialize().await.expect("Initialize failed");

        let tools = client.list_tools().await.expect("list_tools failed");
        assert!(!tools.is_empty(), "Server should have at least one tool");

        let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(
            tool_names.contains(&"read_nda"),
            "Missing built-in read_nda tool"
        );
    }

    #[tokio::test]
    #[ignore]
    async fn test_nda_shmem_transport_ping() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let buffer_path = format!("test_nda_ping_{}.bin", ts);

        let _server = spawn_server(&buffer_path);
        wait_for_buffer(&buffer_path);

        let transport = ShmemTransport::new(&buffer_path).expect("Failed to create transport");
        let mut client = McpClient::new(transport);
        client.initialize().await.expect("Initialize failed");

        client.ping().await.expect("Ping failed");
    }

    #[tokio::test]
    async fn test_json_shmem_transport_initialize() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let buffer_path = format!("test_json_init_{}.bin", ts);

        let _server = spawn_server(&buffer_path);
        wait_for_buffer(&buffer_path);

        let transport = JsonShmemTransport::new(&buffer_path).expect("Failed to create transport");
        let mut client = McpClient::new(transport);

        let result = client.initialize().await.expect("Initialize failed");
        assert_eq!(result.protocol_version, "2024-11-05");
    }

    #[tokio::test]
    async fn test_json_shmem_transport_list_tools() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let buffer_path = format!("test_json_tools_{}.bin", ts);

        let _server = spawn_server(&buffer_path);
        wait_for_buffer(&buffer_path);

        let transport = JsonShmemTransport::new(&buffer_path).expect("Failed to create transport");
        let mut client = McpClient::new(transport);
        client.initialize().await.expect("Initialize failed");

        let tools = client.list_tools().await.expect("list_tools failed");
        assert!(!tools.is_empty(), "Server should have at least one tool");
    }

    #[tokio::test]
    async fn test_cross_transport_consistency() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let buffer_path = format!("test_cross_{}.bin", ts);

        let _server = spawn_server(&buffer_path);
        wait_for_buffer(&buffer_path);

        let nda_transport = ShmemTransport::new(&buffer_path).expect("Failed to create NDA transport");
        let mut nda_client = McpClient::new(nda_transport);
        nda_client.initialize().await.expect("NDA init failed");
        let nda_tools = nda_client.list_tools().await.expect("NDA list_tools failed");
        let nda_names: Vec<String> = nda_tools.iter().map(|t| t.name.clone()).collect();
        drop(nda_client);

        let json_transport = JsonShmemTransport::new(&buffer_path).expect("Failed to create JSON transport");
        let mut json_client = McpClient::new(json_transport);
        json_client.initialize().await.expect("JSON init failed");
        let json_tools = json_client.list_tools().await.expect("JSON list_tools failed");
        let json_names: Vec<String> = json_tools.iter().map(|t| t.name.clone()).collect();

        assert_eq!(
            nda_names, json_names,
            "NDA and JSON transports should see the same tools"
        );
    }

    #[tokio::test]
    async fn test_invalid_buffer_path() {
        let result = JsonShmemTransport::new("nonexistent_path_12345.bin");
        assert!(result.is_err(), "Should fail for nonexistent buffer");
        let err = match result {
            Err(e) => e.to_string(),
            Ok(_) => unreachable!(),
        };
        assert!(
            err.contains("Failed to open buffer") || err.contains("cannot find"),
            "Error should mention file open failure: {}", err
        );
    }

    #[tokio::test]
    async fn test_undersized_buffer() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = format!("test_undersized_{}.bin", ts);
        std::fs::write(&path, b"too small").unwrap();

        let result = JsonShmemTransport::new(&path);
        assert!(result.is_err(), "Should fail for undersized buffer");
        let err = match result {
            Err(e) => e.to_string(),
            Ok(_) => unreachable!(),
        };
        assert!(
            err.contains("expected at least") || err.contains("bytes"),
            "Error should mention size mismatch: {}", err
        );

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn test_send_after_close() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let buffer_path = format!("test_close_{}.bin", ts);

        let _server = spawn_server(&buffer_path);
        wait_for_buffer(&buffer_path);

        let transport = JsonShmemTransport::new(&buffer_path).expect("Failed to create transport");
        transport.close().await.expect("close failed");

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "ping".to_string(),
            params: None,
            id: Some(serde_json::json!(1)),
        };
        let result = transport.send(request).await;
        assert!(result.is_err(), "send after close should fail");
        let err = match result {
            Err(e) => e.to_string(),
            Ok(_) => unreachable!(),
        };
        assert!(
            err.contains("Connection closed"),
            "Should be ConnectionClosed error"
        );
    }

    #[tokio::test]
    async fn test_close_is_idempotent() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let buffer_path = format!("test_idem_{}.bin", ts);

        let _server = spawn_server(&buffer_path);
        wait_for_buffer(&buffer_path);

        let transport = JsonShmemTransport::new(&buffer_path).expect("Failed to create transport");
        transport.close().await.expect("first close failed");
        transport.close().await.expect("second close should succeed");
    }
}

#[cfg(not(target_os = "windows"))]
mod shmem_tests {
    #[tokio::test]
    async fn test_shmem_not_supported_on_this_platform() {
        use velocity_mcp_client::ShmemTransport;
        let result = ShmemTransport::new("test.bin");
        assert!(result.is_err(), "ShmemTransport should fail on non-Windows");
    }
}
