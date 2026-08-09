#![cfg(feature = "ascii-tcp")]

mod test_ascii_tcp_server {
    use mycela;
    use mycela::config::AsciiLineEnding;
    use std::net::SocketAddr;
    use std::time::Duration;

    #[tokio::test]
    async fn ascii_tcp_server_entrypoint_is_available() {
        let handle = mycela::ascii_tcp_server::start_server(
            SocketAddr::from(([127, 0, 0, 1], 0)),
            |request, _peer| async move { Ok(request) },
        );
        handle.abort();
    }

    #[tokio::test]
    async fn configured_crlf_server_interoperates_with_crlf_client() {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe");
        let port = probe.local_addr().expect("probe address").port();
        drop(probe);

        let handle = mycela::ascii_tcp_server::start_server_with_line_ending(
            SocketAddr::from(([127, 0, 0, 1], port)),
            AsciiLineEnding::CrLf,
            |request, _peer| async move {
                assert_eq!(request, "READ VALUE");
                Ok("42.5".to_string())
            },
        );

        let config = ascii_tcp::AsciiTcpConfig {
            host: "127.0.0.1".to_string(),
            port,
            connect_timeout: Duration::from_millis(200),
            io_timeout: Duration::from_millis(200),
            line_ending: ascii_tcp::LineEnding::CrLf,
        };

        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let response = loop {
            match ascii_tcp::exchange_line(&config, "READ VALUE").await {
                Ok(response) => break response,
                Err(ascii_tcp::AsciiTcpError::ConnectTimeout) => {
                    assert!(std::time::Instant::now() < deadline, "server did not start");
                    tokio::task::yield_now().await;
                }
                Err(ascii_tcp::AsciiTcpError::Io(error))
                    if error.kind() == std::io::ErrorKind::ConnectionRefused =>
                {
                    assert!(std::time::Instant::now() < deadline, "server did not start");
                    tokio::task::yield_now().await;
                }
                Err(error) => panic!("CRLF exchange failed: {error}"),
            }
        };

        assert_eq!(response, "42.5");
        handle.abort();
    }
}
