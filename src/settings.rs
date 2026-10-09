//! Process configuration from environment variables.

use crate::js::number::string_to_number;

pub const DEFAULT_PORT: u16 = 38471;
pub const DEFAULT_DB_PATH: &str = "data/sublink.aof";
pub const DEFAULT_CONFIG_TTL_SECONDS: f64 = 60.0 * 60.0 * 24.0 * 30.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub port: u16,
    /// `:memory:` keeps everything in RAM.
    pub db_path: String,
    /// `0` (or any non-positive value) stores configs without expiry.
    pub config_ttl_seconds: Option<f64>,
    pub short_link_ttl_seconds: Option<f64>,
}

impl Settings {
    /// Reads settings through `var`, so tests need not touch the real environment.
    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> Result<Settings, String> {
        let get = |name: &str| var(name).filter(|v| !v.is_empty());
        // The Node runtime's parseNumber(): Number(value), ignoring non-finite results.
        let number = |name: &str| get(name).map(|raw| string_to_number(&raw)).filter(|n| n.is_finite());
        let port = match get("PORT") {
            None => DEFAULT_PORT,
            Some(raw) => raw.trim().parse::<u16>().map_err(|e| format!("invalid PORT {raw:?}: {e}"))?,
        };
        Ok(Settings {
            port,
            db_path: get("DB_PATH").unwrap_or_else(|| DEFAULT_DB_PATH.into()),
            config_ttl_seconds: Some(number("CONFIG_TTL_SECONDS").unwrap_or(DEFAULT_CONFIG_TTL_SECONDS)),
            short_link_ttl_seconds: number("SHORT_LINK_TTL_SECONDS").filter(|n| *n != 0.0),
        })
    }

    pub fn from_env() -> Result<Settings, String> {
        Settings::from_vars(|name| std::env::var(name).ok())
    }
}
