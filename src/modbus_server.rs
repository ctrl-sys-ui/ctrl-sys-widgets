// Modbus TCP server infrastructure for mycela.
//
// Provides a shared `RegisterBank` (a `DashMap<u16, u16>`) and a
// `start_modbus_server` helper that binds a Modbus TCP listener and spawns
// a tokio task to service connections.
//
// The caller supplies an `on_write` callback:
//   - Return `Ok(())` to accept the write (bank is updated automatically).
//   - Return `Err(ExceptionCode)` to reject it (bank unchanged; client receives exception).
//
// Read requests are served directly from the bank,
// returning false/0 for any address that has not been written yet.

use std::future;
use std::net::SocketAddr;
use std::sync::Arc;

use dashmap::DashMap;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_modbus::prelude::*;
use tokio_modbus::server::tcp::{accept_tcp_connection, Server};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteAccessKind {
    SingleCoil,
    SingleRegister,
    MultiCoil,
    MultiRegister,
}

/// Shared virtual Modbus memory bank (coils and registers).
/// Clone freely — all clones share the same underlying map.
pub type RegisterBank = Arc<DashMap<u16, u16>>;

/// Create a new empty register bank.
pub fn new_register_bank() -> RegisterBank {
    Arc::new(DashMap::new())
}

// ── Internal service impl ─────────────────────────────────────────────────────

struct BankService<F> {
    bank: RegisterBank,
    on_write: Arc<F>,
}

impl<F> tokio_modbus::server::Service for BankService<F>
where
    F: Fn(u16, u16, WriteAccessKind) -> Result<(), ExceptionCode> + Send + Sync + 'static,
{
    type Request = Request<'static>;
    type Response = Response;
    type Exception = ExceptionCode;
    type Future = future::Ready<Result<Response, ExceptionCode>>;

    fn call(&self, req: Request<'static>) -> Self::Future {
        let result = match req {
            Request::ReadCoils(addr, cnt) => {
                let values: Vec<bool> = (addr..addr.saturating_add(cnt))
                    .map(|a| self.bank.get(&a).map(|v| *v != 0).unwrap_or(false))
                    .collect();
                Ok(Response::ReadCoils(values))
            }

            Request::ReadDiscreteInputs(addr, cnt) => {
                let values: Vec<bool> = (addr..addr.saturating_add(cnt))
                    .map(|a| self.bank.get(&a).map(|v| *v != 0).unwrap_or(false))
                    .collect();
                Ok(Response::ReadDiscreteInputs(values))
            }

            Request::ReadHoldingRegisters(addr, cnt) => {
                let values: Vec<u16> = (addr..addr.saturating_add(cnt))
                    .map(|a| self.bank.get(&a).map(|v| *v).unwrap_or(0))
                    .collect();
                Ok(Response::ReadHoldingRegisters(values))
            }

            Request::ReadInputRegisters(addr, cnt) => {
                let values: Vec<u16> = (addr..addr.saturating_add(cnt))
                    .map(|a| self.bank.get(&a).map(|v| *v).unwrap_or(0))
                    .collect();
                Ok(Response::ReadInputRegisters(values))
            }

            Request::WriteSingleCoil(addr, val) => {
                tracing::debug!("[modbus-server] WriteSingleCoil addr={} value={}", addr, val);
                let raw = if val { 1 } else { 0 };
                match (self.on_write)(addr, raw, WriteAccessKind::SingleCoil) {
                    Ok(()) => {
                        self.bank.insert(addr, raw);
                        Ok(Response::WriteSingleCoil(addr, val))
                    }
                    Err(e) => Err(e),
                }
            }

            Request::WriteSingleRegister(addr, val) => match (self.on_write)(
                addr,
                val,
                WriteAccessKind::SingleRegister,
            ) {
                Ok(()) => {
                    tracing::debug!("[modbus-server] WriteSingleRegister addr={} value={}", addr, val);
                    self.bank.insert(addr, val);
                    Ok(Response::WriteSingleRegister(addr, val))
                }
                Err(e) => Err(e),
            },

            Request::WriteMultipleCoils(addr, data) => {
                let count = data.len() as u16;
                let mut validated: Vec<(u16, u16)> = Vec::with_capacity(data.len());
                for (i, &val) in data.iter().enumerate() {
                    let reg = match addr.checked_add(i as u16) {
                        Some(r) => r,
                        None => return future::ready(Err(ExceptionCode::IllegalDataAddress)),
                    };
                    let raw = if val { 1 } else { 0 };
                    match (self.on_write)(reg, raw, WriteAccessKind::MultiCoil) {
                        Ok(()) => validated.push((reg, raw)),
                        Err(e) => return future::ready(Err(e)),
                    }
                }
                for (reg, val) in validated {
                    self.bank.insert(reg, val);
                }
                Ok(Response::WriteMultipleCoils(addr, count))
            }

            Request::WriteMultipleRegisters(addr, data) => {
                let count = data.len() as u16;
                // Validate all registers via on_write and compute addresses.
                // on_write is treated as a pure validator — no bank changes yet.
                // This ensures that a rejection on any register leaves the bank untouched.
                let mut validated: Vec<(u16, u16)> = Vec::with_capacity(data.len());
                for (i, &val) in data.iter().enumerate() {
                    let reg = match addr.checked_add(i as u16) {
                        Some(r) => r,
                        None => return future::ready(Err(ExceptionCode::IllegalDataAddress)),
                    };
                    match (self.on_write)(reg, val, WriteAccessKind::MultiRegister) {
                        Ok(()) => validated.push((reg, val)),
                        Err(e) => return future::ready(Err(e)),
                    }
                }
                // All on_write calls succeeded — commit to bank atomically.
                for (reg, val) in validated {
                    self.bank.insert(reg, val);
                }
                Ok(Response::WriteMultipleRegisters(addr, count))
            }

            _ => Err(ExceptionCode::IllegalFunction),
        };
        future::ready(result)
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Start a Modbus TCP server that exposes `bank` to Modbus clients.
///
/// `on_write(register, value)` is called for every write request before the
/// bank is updated.  Return `Ok(())` to accept, or an [`ExceptionCode`] to
/// reject (the bank is not updated and the client receives an exception).
///
/// The server runs in a background tokio task.  The returned [`JoinHandle`]
/// can be used to await or abort the server.
pub fn start_modbus_server<F>(addr: SocketAddr, bank: RegisterBank, on_write: F) -> JoinHandle<()>
where
    F: Fn(u16, u16, WriteAccessKind) -> Result<(), ExceptionCode> + Send + Sync + 'static,
{
    let on_write = Arc::new(on_write);
    tokio::spawn(async move {
        let listener = match TcpListener::bind(addr).await {
            Ok(l) => {
                tracing::info!("[modbus-server] listening on {addr}");
                l
            }
            Err(e) => {
                tracing::error!("[modbus-server] failed to bind on {addr}: {e}");
                return;
            }
        };

        let server = Server::new(listener);
        let on_connected = |stream, peer| {
            tracing::info!("[modbus-server] peer client connected: {}", peer);
            let bank = bank.clone();
            let on_write = on_write.clone();
            std::future::ready(accept_tcp_connection(stream, peer, move |_peer_addr| {
                Ok(Some(BankService {
                    bank: bank.clone(),
                    on_write: on_write.clone(),
                }))
            }))
        };

        let result = server
            .serve(&on_connected, |err| {
                tracing::error!("[modbus-server] connection error: {err}");
            })
            .await;

        if let Err(e) = result {
            tracing::error!("[modbus-server] stopped with error: {e}");
        }
    })
}
