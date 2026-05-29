use std::time::Duration;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::{ConfigError, Result};
use crate::validation::validate_non_empty;

const MIN_SCHEDULE_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    pub listen: String,
    pub public_url: Option<String>,
    pub bootstrap: bool,
    pub schedule: ScheduleConfig,
    pub analytics_script: AnalyticsScriptConfig,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:3000".to_owned(),
            public_url: None,
            bootstrap: true,
            schedule: ScheduleConfig::default(),
            analytics_script: AnalyticsScriptConfig::default(),
        }
    }
}

impl ServerConfig {
    pub(crate) fn validate(&self) -> Result<()> {
        validate_non_empty("server.listen", &self.listen)?;
        validate_public_url(self.public_url.as_deref())?;
        self.schedule.validate()?;
        self.analytics_script.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AnalyticsScriptConfig {
    pub enabled: bool,
    pub src: String,
    pub attributes: IndexMap<String, ScriptAttributeValue>,
}

impl Default for AnalyticsScriptConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            src: "https://rybbit.thekoppe.com/api/script.js".to_owned(),
            attributes: IndexMap::new(),
        }
    }
}

impl AnalyticsScriptConfig {
    fn validate(&self) -> Result<()> {
        validate_http_url("server.analytics_script.src", &self.src)?;

        for name in self.attributes.keys() {
            validate_script_attribute_name(name)?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ScriptAttributeValue {
    Bool(bool),
    String(String),
}

fn validate_public_url(value: Option<&str>) -> Result<()> {
    let Some(value) = value else {
        return Ok(());
    };

    let url = Url::parse(value).map_err(|error| {
        ConfigError::Validation(format!(
            "server.public_url must be an absolute URL: {error}"
        ))
    })?;

    if !matches!(url.scheme(), "http" | "https") {
        return Err(ConfigError::Validation(
            "server.public_url must use http or https".to_owned(),
        ));
    }

    if url.host_str().is_none() {
        return Err(ConfigError::Validation(
            "server.public_url must include a host".to_owned(),
        ));
    }

    if url.path() != "/" {
        return Err(ConfigError::Validation(
            "server.public_url must not include a path".to_owned(),
        ));
    }

    if url.query().is_some() || url.fragment().is_some() {
        return Err(ConfigError::Validation(
            "server.public_url must not include a query or fragment".to_owned(),
        ));
    }

    Ok(())
}

fn validate_http_url(name: &str, value: &str) -> Result<()> {
    let url = Url::parse(value).map_err(|error| {
        ConfigError::Validation(format!("{name} must be an absolute URL: {error}"))
    })?;

    if !matches!(url.scheme(), "http" | "https") {
        return Err(ConfigError::Validation(format!(
            "{name} must use http or https"
        )));
    }

    if url.host_str().is_none() {
        return Err(ConfigError::Validation(format!(
            "{name} must include a host"
        )));
    }

    Ok(())
}

fn validate_script_attribute_name(name: &str) -> Result<()> {
    if name.eq_ignore_ascii_case("src") {
        return Err(ConfigError::Validation(
            "server.analytics_script.attributes must not contain src".to_owned(),
        ));
    }

    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'.'))
    {
        return Err(ConfigError::Validation(format!(
            "server.analytics_script.attributes contains invalid attribute name {name:?}"
        )));
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScheduleConfig {
    pub enabled: bool,
    pub interval: String,
}

impl Default for ScheduleConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval: "24h".to_owned(),
        }
    }
}

impl ScheduleConfig {
    pub fn parse_interval(&self) -> std::result::Result<Duration, ConfigError> {
        parse_duration(&self.interval).map_err(|message| {
            ConfigError::Validation(format!("server.schedule.interval: {message}"))
        })
    }

    fn validate(&self) -> Result<()> {
        self.parse_interval()?;
        Ok(())
    }
}

pub(crate) fn parse_duration(s: &str) -> std::result::Result<Duration, String> {
    let s = s.trim();

    if s.is_empty() {
        return Err("interval must not be empty".to_owned());
    }

    let split_pos = s
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(s.len());

    let (number, unit) = s.split_at(split_pos);

    if number.is_empty() {
        return Err(format!("invalid number in {s:?}"));
    }

    let value: f64 = number
        .parse()
        .map_err(|_| format!("invalid number in {s:?}"))?;

    if !value.is_finite() || value <= 0.0 {
        return Err("interval must be positive".to_owned());
    }

    let seconds = match unit.trim() {
        "s" | "sec" | "secs" => value,
        "m" | "min" | "mins" => value * 60.0,
        "h" | "hr" | "hrs" | "hour" | "hours" => value * 3600.0,
        "d" | "day" | "days" => value * 86400.0,
        other => return Err(format!("unknown time unit {other:?}; use s, m, h, or d")),
    };

    if !seconds.is_finite() {
        return Err("interval is out of range".to_owned());
    }

    let duration = std::time::Duration::try_from_secs_f64(seconds)
        .map_err(|_| "interval is out of range".to_owned())?;

    if duration < MIN_SCHEDULE_INTERVAL {
        return Err(format!(
            "interval must be at least {}s",
            MIN_SCHEDULE_INTERVAL.as_secs()
        ));
    }

    Ok(duration)
}
