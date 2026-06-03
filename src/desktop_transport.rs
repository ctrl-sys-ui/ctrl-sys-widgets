use std::env;
use crate::config::AppConfig;

pub const DESKTOP_TRANSPORT_ENV: &str = "MYCELA_DESKTOP_TRANSPORT";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopTransport {
    Loopback,
    Ipc,
}

impl DesktopTransport {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "loopback" | "http" | "localhost" => Some(Self::Loopback),
            "ipc" | "bridge" => Some(Self::Ipc),
            _ => None,
        }
    }

    pub fn from_env() -> Self {
        match env::var(DESKTOP_TRANSPORT_ENV) {
            Ok(value) => Self::parse(&value).unwrap_or(Self::Loopback),
            Err(_) => Self::Loopback,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Loopback => "loopback",
            Self::Ipc => "ipc",
        }
    }

    pub fn resolve_from_sources(
        json_transport: Option<&str>,
        env_transport: Option<&str>,
        allow_env_override: bool,
    ) -> Self {
        let mut selected = json_transport
            .and_then(Self::parse)
            .unwrap_or(Self::Loopback);

        if allow_env_override {
            if let Some(env_value) = env_transport {
                if let Some(parsed) = Self::parse(env_value) {
                    selected = parsed;
                }
            }
        }

        selected
    }

    pub fn from_app_config(config: &AppConfig) -> Self {
        let json_transport = config.startup.desktop.transport.as_deref();
        let env_transport = env::var(DESKTOP_TRANSPORT_ENV).ok();
        Self::resolve_from_sources(
            json_transport,
            env_transport.as_deref(),
            config.startup.desktop.allow_env_transport_override,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::DesktopTransport;

    #[test]
    fn parse_known_values() {
        assert_eq!(DesktopTransport::parse("loopback"), Some(DesktopTransport::Loopback));
        assert_eq!(DesktopTransport::parse("HTTP"), Some(DesktopTransport::Loopback));
        assert_eq!(DesktopTransport::parse("localhost"), Some(DesktopTransport::Loopback));
        assert_eq!(DesktopTransport::parse("ipc"), Some(DesktopTransport::Ipc));
        assert_eq!(DesktopTransport::parse("bridge"), Some(DesktopTransport::Ipc));
    }

    #[test]
    fn parse_unknown_value() {
        assert_eq!(DesktopTransport::parse("unknown"), None);
    }

    #[test]
    fn resolve_prefers_json_when_env_override_disabled() {
        let resolved = DesktopTransport::resolve_from_sources(
            Some("loopback"),
            Some("ipc"),
            false,
        );
        assert_eq!(resolved, DesktopTransport::Loopback);
    }

    #[test]
    fn resolve_prefers_env_when_override_enabled() {
        let resolved = DesktopTransport::resolve_from_sources(
            Some("loopback"),
            Some("ipc"),
            true,
        );
        assert_eq!(resolved, DesktopTransport::Ipc);
    }

    #[test]
    fn resolve_defaults_to_loopback_for_invalid_values() {
        let resolved = DesktopTransport::resolve_from_sources(
            Some("invalid"),
            Some("also-invalid"),
            true,
        );
        assert_eq!(resolved, DesktopTransport::Loopback);
    }
}
