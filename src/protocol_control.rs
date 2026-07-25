use crate::app::AppState;

#[cfg(feature = "epics-pvxs")]
use crate::server_setup::setup_server_pvs;

#[derive(Debug)]
pub enum ProtocolControlError {
    AlreadyRunning(&'static str),
    NotRunning(&'static str),
    Operation(String),
    Internal(String),
}

impl std::fmt::Display for ProtocolControlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyRunning(msg) => write!(f, "{}", msg),
            Self::NotRunning(msg) => write!(f, "{}", msg),
            Self::Operation(msg) => write!(f, "{}", msg),
            Self::Internal(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for ProtocolControlError {}

#[cfg(feature = "epics-pvxs")]
pub async fn start_epics_server(state: &AppState) -> Result<pvxs_sys::Server, ProtocolControlError> {
    if state.is_server_running() {
        return Err(ProtocolControlError::AlreadyRunning("EPICS server already running"));
    }

    let config = state.config.clone();
    let result = tokio::task::spawn_blocking(move || -> pvxs_sys::Result<pvxs_sys::Server> {
        let server = pvxs_sys::Server::start_from_env()?;
        for screen in &config.screens {
            setup_server_pvs(&server, &screen.widgets)?;
        }
        Ok(server)
    })
    .await;

    match result {
        Ok(Ok(server)) => Ok(server),
        Ok(Err(e)) => Err(ProtocolControlError::Operation(e.to_string())),
        Err(e) => Err(ProtocolControlError::Internal(e.to_string())),
    }
}

#[cfg(feature = "epics-pvxs")]
pub async fn start_epics_runtime(state: &AppState) -> Result<(), ProtocolControlError> {
    let server = start_epics_server(state).await?;

    if let Some(hook) = state.epics_start_hook.as_ref() {
        if let Err(error) = hook(state, &server) {
            let _ = server.stop_drop();
            return Err(error);
        }
    }

    set_epics_server(state, server);
    Ok(())
}

#[cfg(feature = "epics-pvxs")]
pub fn set_epics_server(state: &AppState, server: pvxs_sys::Server) {
    *state.pv_server.lock().unwrap() = Some(server);
}

#[cfg(feature = "epics-pvxs")]
pub async fn stop_epics_server(state: &AppState) -> Result<(), ProtocolControlError> {
    let server = state.pv_server.lock().unwrap().take();
    let Some(server) = server else {
        return Err(ProtocolControlError::NotRunning("EPICS server is not running"));
    };

    match tokio::task::spawn_blocking(move || server.stop_drop()).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(ProtocolControlError::Operation(e.to_string())),
        Err(e) => Err(ProtocolControlError::Internal(e.to_string())),
    }
}

#[cfg(not(feature = "epics-pvxs"))]
pub async fn start_epics_runtime(_state: &AppState) -> Result<(), ProtocolControlError> {
    Err(ProtocolControlError::Operation(
        "EPICS feature is not enabled".to_string(),
    ))
}

#[cfg(not(feature = "epics-pvxs"))]
pub async fn stop_epics_server(_state: &AppState) -> Result<(), ProtocolControlError> {
    Err(ProtocolControlError::Operation(
        "EPICS feature is not enabled".to_string(),
    ))
}

#[cfg(feature = "modbus")]
pub fn start_modbus_tasks(
    state: &AppState,
    tasks: Vec<tokio::task::JoinHandle<()>>,
) -> Result<(), ProtocolControlError> {
    if state.is_modbus_running() {
        return Err(ProtocolControlError::AlreadyRunning("Modbus simulator already running"));
    }
    *state.modbus_task.lock().unwrap() = Some(tasks);
    Ok(())
}

#[cfg(feature = "modbus")]
pub fn start_modbus_runtime(state: &AppState) -> Result<(), ProtocolControlError> {
    if state.is_modbus_running() {
        return Err(ProtocolControlError::AlreadyRunning("Modbus simulator already running"));
    }

    let Some(hook) = state.modbus_start_hook.as_ref() else {
        return Err(ProtocolControlError::Operation(
            "Modbus start hook is not configured".to_string(),
        ));
    };

    let tasks = hook(state)?;
    *state.modbus_task.lock().unwrap() = Some(tasks);
    Ok(())
}

#[cfg(not(feature = "modbus"))]
pub fn start_modbus_tasks(
    _state: &AppState,
    _tasks: Vec<tokio::task::JoinHandle<()>>,
) -> Result<(), ProtocolControlError> {
    Err(ProtocolControlError::Operation(
        "Modbus feature is not enabled".to_string(),
    ))
}

#[cfg(not(feature = "modbus"))]
pub fn start_modbus_runtime(_state: &AppState) -> Result<(), ProtocolControlError> {
    Err(ProtocolControlError::Operation(
        "Modbus feature is not enabled".to_string(),
    ))
}

#[cfg(feature = "modbus")]
pub fn stop_modbus_tasks(state: &AppState) -> Result<(), ProtocolControlError> {
    let handles = state.modbus_task.lock().unwrap().take();
    let Some(handles) = handles else {
        return Err(ProtocolControlError::NotRunning("Modbus simulator is not running"));
    };

    for handle in handles {
        handle.abort();
    }
    state.channel_ctx.modbus_pool.disconnect_all();
    Ok(())
}

#[cfg(not(feature = "modbus"))]
pub fn stop_modbus_tasks(_state: &AppState) -> Result<(), ProtocolControlError> {
    Err(ProtocolControlError::Operation(
        "Modbus feature is not enabled".to_string(),
    ))
}
