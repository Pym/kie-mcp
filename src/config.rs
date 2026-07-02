use std::{env, path::PathBuf, time::Duration};

use crate::kie::KieError;

pub const DEFAULT_API_BASE: &str = "https://api.kie.ai";
pub const DEFAULT_UPLOAD_BASE: &str = "https://kieai.redpandaai.co";
pub const DEFAULT_OUTPUT_DIR: &str = "output/kie";
pub const DEFAULT_MAX_UPLOAD_BYTES: u64 = 512 * 1024 * 1024;
const DEFAULT_TIMEOUT_SECS: u64 = 900;
const DEFAULT_HTTP_TIMEOUT_SECS: u64 = 300;

#[derive(Debug, Clone)]
pub struct Config {
    pub api_key: Option<String>,
    pub api_base: String,
    pub upload_base: String,
    pub output_dir: PathBuf,
    pub timeout: Duration,
    pub http_timeout: Duration,
    pub max_upload_bytes: u64,
    pub input_roots: Vec<PathBuf>,
}

impl Config {
    pub fn from_env() -> Result<Self, KieError> {
        Ok(Self {
            api_key: env_string("KIE_API_KEY")?.filter(|value| !value.is_empty()),
            api_base: env_base_url("KIE_MCP_API_BASE", DEFAULT_API_BASE)?,
            upload_base: env_base_url("KIE_MCP_UPLOAD_BASE", DEFAULT_UPLOAD_BASE)?,
            output_dir: env_path("KIE_MCP_OUTPUT_DIR", DEFAULT_OUTPUT_DIR)?,
            timeout: Duration::from_secs(env_positive_u64(
                "KIE_MCP_TIMEOUT_SECS",
                DEFAULT_TIMEOUT_SECS,
            )?),
            http_timeout: Duration::from_secs(env_positive_u64(
                "KIE_MCP_HTTP_TIMEOUT_SECS",
                DEFAULT_HTTP_TIMEOUT_SECS,
            )?),
            max_upload_bytes: env_positive_u64(
                "KIE_MCP_MAX_UPLOAD_BYTES",
                DEFAULT_MAX_UPLOAD_BYTES,
            )?,
            input_roots: env::var_os("KIE_MCP_INPUT_ROOTS")
                .map(|value| {
                    env::split_paths(&value)
                        .filter(|path| !path.as_os_str().is_empty())
                        .collect()
                })
                .unwrap_or_default(),
        })
    }

    pub fn require_api_key(&self) -> Result<&str, KieError> {
        self.api_key.as_deref().ok_or(KieError::MissingApiKey)
    }
}

fn env_string(name: &'static str) -> Result<Option<String>, KieError> {
    match env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err(invalid_config(name, "must be valid Unicode")),
    }
}

fn env_base_url(name: &'static str, default: &str) -> Result<String, KieError> {
    let value = env_string(name)?.unwrap_or_else(|| default.to_string());
    normalize_base_url(name, &value)
}

fn env_path(name: &'static str, default: &str) -> Result<PathBuf, KieError> {
    match env::var_os(name) {
        Some(value) if value.as_os_str().is_empty() => {
            Err(invalid_config(name, "must not be empty"))
        }
        Some(value) => Ok(PathBuf::from(value)),
        None => Ok(PathBuf::from(default)),
    }
}

fn normalize_base_url(name: &'static str, value: &str) -> Result<String, KieError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(invalid_config(name, "must not be empty"));
    }
    let parsed = url::Url::parse(value).map_err(|err| {
        invalid_config(name, format!("must be a valid http or https URL ({err})"))
    })?;
    match parsed.scheme() {
        "http" | "https" => {}
        scheme => {
            return Err(invalid_config(
                name,
                format!("must use http or https, got {scheme}"),
            ));
        }
    }
    if parsed.host_str().is_none() {
        return Err(invalid_config(name, "must include a host"));
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(invalid_config(
            name,
            "must not include a query string or fragment",
        ));
    }
    Ok(value.trim_end_matches('/').to_string())
}

fn env_positive_u64(name: &'static str, default: u64) -> Result<u64, KieError> {
    let Some(value) = env_string(name)? else {
        return Ok(default);
    };
    parse_positive_u64(name, &value)
}

fn parse_positive_u64(name: &'static str, value: &str) -> Result<u64, KieError> {
    let value = value.trim();
    let parsed = value
        .parse::<u64>()
        .map_err(|_| invalid_config(name, "must be a positive integer"))?;
    if parsed == 0 {
        return Err(invalid_config(name, "must be greater than 0"));
    }
    Ok(parsed)
}

fn invalid_config(name: &'static str, message: impl Into<String>) -> KieError {
    KieError::InvalidConfig {
        name,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_base_url, parse_positive_u64};

    #[test]
    fn base_urls_must_be_http_or_https() {
        assert_eq!(
            normalize_base_url("KIE_MCP_API_BASE", "https://api.kie.ai/").unwrap(),
            "https://api.kie.ai"
        );

        let err = normalize_base_url("KIE_MCP_API_BASE", "file:///tmp/api").unwrap_err();
        assert!(err.to_string().contains("http or https"));

        let err = normalize_base_url("KIE_MCP_API_BASE", "not a url").unwrap_err();
        assert!(err.to_string().contains("valid http or https URL"));
    }

    #[test]
    fn numeric_config_values_must_be_positive_integers() {
        assert_eq!(
            parse_positive_u64("KIE_MCP_TIMEOUT_SECS", "30").unwrap(),
            30
        );

        let err = parse_positive_u64("KIE_MCP_TIMEOUT_SECS", "0").unwrap_err();
        assert!(err.to_string().contains("greater than 0"));

        let err = parse_positive_u64("KIE_MCP_TIMEOUT_SECS", "oops").unwrap_err();
        assert!(err.to_string().contains("positive integer"));
    }
}
