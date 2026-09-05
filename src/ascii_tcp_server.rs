use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinHandle;

use crate::config::{AsciiLineEnding, AsciiTcpConfig};

fn to_line_ending(ending: AsciiLineEnding) -> ascii_tcp::LineEnding {
    match ending {
        AsciiLineEnding::Lf => ascii_tcp::LineEnding::Lf,
        AsciiLineEnding::CrLf => ascii_tcp::LineEnding::CrLf,
        AsciiLineEnding::Cr => ascii_tcp::LineEnding::Cr,
    }
}

/// Start an ASCII TCP line-protocol server that responds to incoming requests.
///
/// The server accepts a request line from each client, passes it to the provided
/// handler, and writes the handler's response back with the configured line ending.
///
/// The handler receives the request text and the peer socket address and should
/// return a response string.
pub fn start_server<F, Fut>(addr: SocketAddr, handler: F) -> JoinHandle<()>
where
    F: Fn(String, SocketAddr) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<String, String>> + Send + 'static,
{
    start_server_with_line_ending(addr, AsciiLineEnding::Lf, handler)
}

/// Start an ASCII TCP server with an explicit request/response line ending.
pub fn start_server_with_line_ending<F, Fut>(
    addr: SocketAddr,
    line_ending: AsciiLineEnding,
    handler: F,
) -> JoinHandle<()>
where
    F: Fn(String, SocketAddr) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<String, String>> + Send + 'static,
{
    let handler = Arc::new(handler);

    tokio::spawn(async move {
        let config = ascii_tcp::server::ServerConfig {
            bind_addr: addr,
            io_timeout: Duration::from_secs(2),
            line_ending: to_line_ending(line_ending),
            max_line_length: 8 * 1024,
        };

        let server = match ascii_tcp::server::Server::start(config, {
            let handler = handler.clone();
            move |request, peer| {
                let handler = handler.clone();
                async move {
                    match handler(request, peer).await {
                        Ok(response) => Ok(response),
                        Err(err) => Err(ascii_tcp::AsciiTcpError::Io(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            err,
                        ))),
                    }
                }
            }
        })
        .await
        {
            Ok(server) => server,
            Err(err) => {
                tracing::error!("[ascii-tcp-server] failed to start: {err}");
                return;
            }
        };

        let _ = std::future::pending::<()>().await;
        let _ = server;
    })
}

/// Start an ASCII TCP server using the widget's transport config.
pub fn start_from_config<F, Fut>(config: &AsciiTcpConfig, handler: F) -> JoinHandle<()>
where
    F: Fn(String, SocketAddr) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<String, String>> + Send + 'static,
{
    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    start_server_with_line_ending(addr, config.line_ending, handler)
}
