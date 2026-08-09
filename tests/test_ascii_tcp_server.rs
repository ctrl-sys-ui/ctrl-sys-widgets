#![cfg(feature = "ascii-tcp")]

mod test_ascii_tcp_server {
    use mycela;
    use std::net::SocketAddr;

    #[tokio::test]
    async fn ascii_tcp_server_entrypoint_is_available() {
        let handle = mycela::ascii_tcp_server::start_server(
            SocketAddr::from(([127, 0, 0, 1], 0)),
            |request, _peer| async move { Ok(request) },
        );
        handle.abort();
    }
}
