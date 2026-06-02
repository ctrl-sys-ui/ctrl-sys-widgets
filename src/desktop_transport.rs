use std::env;

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
}
