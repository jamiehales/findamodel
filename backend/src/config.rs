//! Process settings, read from environment variables.
//!
//! Each setting accepts a `FINDAMODEL_*` variable and, for compatibility with existing
//! deployments, the equivalent ASP.NET-style variable (e.g. `Models__DirectoryPath`).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMode {
    Server,
    Desktop,
}

/// Defaults applied when the app config row is first created.
#[derive(Debug, Clone, Default)]
pub struct AppConfigDefaults {
    pub default_raft_height_mm: Option<f32>,
    pub generate_previews_enabled: Option<bool>,
    pub minimum_preview_generation_version: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub mode: RuntimeMode,
    pub listen: SocketAddr,
    /// Required on every /api request in desktop mode.
    pub desktop_token: Option<String>,
    pub disable_cors: bool,
    /// "Development" enables permissive CORS for the Vite dev server.
    pub environment: String,
    pub data_path: PathBuf,
    /// Directory holding the built frontend; served with an index.html fallback.
    pub web_root: Option<PathBuf>,
    /// Models root configured by the environment (overrides the setup wizard value).
    pub models_path: Option<String>,
    pub index_schedule: String,
    pub index_on_startup: bool,
    pub file_scan_threads: usize,
    pub directory_batch_size: usize,
    pub preview_use_gpu: bool,
    /// Most disk space generated model VOXLs may take up in the cache (0 = no cache).
    pub voxl_cache_max_bytes: u64,
    pub app_config_defaults: AppConfigDefaults,
}

pub const DEFAULT_FILE_SCAN_THREADS: usize = 10;
pub const DEFAULT_INDEX_SCHEDULE: &str = "0 3 * * *";
pub const DEFAULT_VOXL_CACHE_SIZE_MB: u64 = 1024;
const BYTES_PER_MB: u64 = 1024 * 1024;

fn env(names: &[&str]) -> Option<String> {
    names.iter().find_map(|n| std::env::var(n).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty()))
}

fn env_bool(names: &[&str]) -> Option<bool> {
    env(names).and_then(|v| match v.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    })
}

fn env_parse<T: std::str::FromStr>(names: &[&str]) -> Option<T> {
    env(names).and_then(|v| v.parse().ok())
}

/// Uses the configured thread count when positive, otherwise the default.
pub fn resolve_file_scan_threads(configured: Option<i64>) -> usize {
    match configured {
        Some(n) if n > 0 => n as usize,
        _ => DEFAULT_FILE_SCAN_THREADS,
    }
}

/// Parses a listen address from `host:port` or a URL such as `http://+:80`.
pub fn parse_listen_address(value: &str) -> Result<SocketAddr> {
    let first = value.split(';').next().unwrap_or(value).trim();
    let without_scheme = first.split_once("://").map_or(first, |(_, rest)| rest);
    let authority = without_scheme.split('/').next().unwrap_or(without_scheme);
    let (host, port) = authority.rsplit_once(':').with_context(|| format!("listen address '{value}' has no port"))?;
    let port: u16 = port.parse().with_context(|| format!("invalid port in '{value}'"))?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let ip: IpAddr = match host {
        "+" | "*" | "" | "0.0.0.0" => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        "localhost" => IpAddr::V4(Ipv4Addr::LOCALHOST),
        other => other.parse().with_context(|| format!("invalid host in '{value}'"))?,
    };
    Ok(SocketAddr::new(ip, port))
}

impl Settings {
    pub fn from_env() -> Result<Self> {
        let desktop = env(&["FINDAMODEL_MODE"]).is_some_and(|m| m.eq_ignore_ascii_case("desktop"));
        let desktop_token = env(&["FINDAMODEL_DESKTOP_SESSION_TOKEN"]);

        let listen = if desktop {
            let url = env(&["FINDAMODEL_URL"]).context("FINDAMODEL_URL is required in desktop mode.")?;
            let addr =
                parse_listen_address(&url).context("FINDAMODEL_URL must be a valid absolute URL in desktop mode.")?;
            if !addr.ip().is_loopback() {
                bail!("FINDAMODEL_URL must bind to localhost in desktop mode.");
            }
            if desktop_token.is_none() {
                bail!("FINDAMODEL_DESKTOP_SESSION_TOKEN is required in desktop mode.");
            }
            addr
        } else {
            match env(&["FINDAMODEL_LISTEN", "FINDAMODEL_URL", "ASPNETCORE_URLS"]) {
                Some(v) => parse_listen_address(&v)?,
                None => SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 5000),
            }
        };

        let data_path = env(&["FINDAMODEL_DATA_PATH", "Configuration__DataPath"]).unwrap_or_else(|| "data".to_string());
        let data_path = std::path::absolute(&data_path).unwrap_or_else(|_| PathBuf::from(&data_path));

        let web_root = env(&["FINDAMODEL_WEB_ROOT"]).map(PathBuf::from).or_else(|| {
            let default = PathBuf::from("wwwroot");
            default.is_dir().then_some(default)
        });

        Ok(Settings {
            mode: if desktop { RuntimeMode::Desktop } else { RuntimeMode::Server },
            listen,
            desktop_token: if desktop { desktop_token } else { None },
            disable_cors: env_bool(&["FINDAMODEL_DISABLE_CORS"]).unwrap_or(false),
            environment: env(&["FINDAMODEL_ENVIRONMENT", "ASPNETCORE_ENVIRONMENT"])
                .unwrap_or_else(|| "Production".to_string()),
            data_path,
            web_root,
            models_path: env(&["FINDAMODEL_MODELS_PATH", "Models__DirectoryPath"]),
            index_schedule: env(&["FINDAMODEL_INDEX_SCHEDULE", "Models__IndexSchedule"])
                .unwrap_or_else(|| DEFAULT_INDEX_SCHEDULE.to_string()),
            index_on_startup: env_bool(&["FINDAMODEL_INDEX_ON_STARTUP", "Indexing__IndexOnStartup"]).unwrap_or(false),
            file_scan_threads: resolve_file_scan_threads(env_parse(&[
                "FINDAMODEL_FILE_SCAN_THREADS",
                "Indexing__FileScanThreads",
            ])),
            directory_batch_size: env_parse(&["FINDAMODEL_DIRECTORY_BATCH_SIZE", "Indexing__DirectoryBatchSize"])
                .filter(|n: &usize| *n > 0)
                .unwrap_or(1000),
            preview_use_gpu: env_bool(&["FINDAMODEL_PREVIEW_USE_GPU", "Preview__UseGpu"]).unwrap_or(true),
            voxl_cache_max_bytes: env_parse::<u64>(&["FINDAMODEL_VOXL_CACHE_SIZE_MB"])
                .unwrap_or(DEFAULT_VOXL_CACHE_SIZE_MB)
                .saturating_mul(BYTES_PER_MB),
            app_config_defaults: AppConfigDefaults {
                default_raft_height_mm: env_parse(&["AppConfig__DefaultRaftHeightMm"]),
                generate_previews_enabled: env_bool(&["AppConfig__GeneratePreviewsEnabled"]),
                minimum_preview_generation_version: env_parse(&["AppConfig__MinimumPreviewGenerationVersion"]),
            },
        })
    }

    /// Settings for an embedded or test instance rooted at `data_path`.
    pub fn local(data_path: PathBuf, models_path: Option<String>) -> Self {
        Settings {
            mode: RuntimeMode::Server,
            listen: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            desktop_token: None,
            disable_cors: true,
            environment: "Production".to_string(),
            data_path,
            web_root: None,
            models_path,
            index_schedule: DEFAULT_INDEX_SCHEDULE.to_string(),
            index_on_startup: false,
            file_scan_threads: DEFAULT_FILE_SCAN_THREADS,
            directory_batch_size: 1000,
            preview_use_gpu: false,
            voxl_cache_max_bytes: DEFAULT_VOXL_CACHE_SIZE_MB * BYTES_PER_MB,
            app_config_defaults: AppConfigDefaults::default(),
        }
    }

    pub fn is_development(&self) -> bool {
        self.environment.eq_ignore_ascii_case("development")
    }

    pub fn database_path(&self) -> PathBuf {
        self.data_path.join("findamodel.db")
    }

    pub fn renders_path(&self) -> PathBuf {
        self.data_path.join("cache").join("renders")
    }

    pub fn voxl_cache_path(&self) -> PathBuf {
        self.data_path.join("cache").join("voxl")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_listen_addresses() {
        assert_eq!(parse_listen_address("http://+:80").unwrap(), "0.0.0.0:80".parse().unwrap());
        assert_eq!(
            parse_listen_address("http://localhost:5000;https://localhost:5001").unwrap(),
            "127.0.0.1:5000".parse().unwrap()
        );
        assert_eq!(parse_listen_address("127.0.0.1:41234").unwrap(), "127.0.0.1:41234".parse().unwrap());
        assert_eq!(parse_listen_address("http://[::1]:8080").unwrap(), "[::1]:8080".parse().unwrap());
        assert!(parse_listen_address("http://localhost").is_err());
    }

    #[test]
    fn file_scan_threads_use_configured_value_or_default() {
        assert_eq!(resolve_file_scan_threads(None), 10);
        assert_eq!(resolve_file_scan_threads(Some(0)), 10);
        assert_eq!(resolve_file_scan_threads(Some(-3)), 10);
        assert_eq!(resolve_file_scan_threads(Some(12)), 12);
    }
}
