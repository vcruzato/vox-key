use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub ptt_key: String,
    pub model: String,
    pub language: String,
    pub input_device: String,
    pub max_seconds: u32,
    pub hold_to_start_ms: u64,
    pub warn_seconds: u32,
    pub model_path: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ptt_key: "RightAlt".to_string(),
            model: "small".to_string(),
            language: "pt".to_string(),
            input_device: "default".to_string(),
            max_seconds: 60,
            hold_to_start_ms: 1000,
            warn_seconds: 30,
            model_path: None,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Config> {
        if !path.exists() {
            return Ok(Config::default());
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let cfg: Config =
            toml::from_str(&text).with_context(|| format!("parsing config {}", path.display()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<()> {
        if self.max_seconds == 0 {
            anyhow::bail!("max_seconds must be > 0");
        }
        Ok(())
    }

    /// Path to the GGML model file for `self.model`.
    pub fn resolved_model_path(&self) -> PathBuf {
        if let Some(p) = &self.model_path {
            return PathBuf::from(p);
        }
        cache_base()
            .join("voxkey")
            .join(format!("ggml-{}-q5_1.bin", self.model))
    }
}

fn cache_base() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

pub fn default_config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("voxkey").join("config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sensible() {
        let c = Config::default();
        assert_eq!(c.ptt_key, "RightAlt");
        assert_eq!(c.model, "small");
        assert_eq!(c.language, "pt");
        assert_eq!(c.input_device, "default");
        assert_eq!(c.max_seconds, 60);
    }

    #[test]
    fn partial_toml_overrides_only_given_fields() {
        let c: Config = toml::from_str("ptt_key = \"CapsLock\"\nmodel = \"base\"\n").unwrap();
        assert_eq!(c.ptt_key, "CapsLock");
        assert_eq!(c.model, "base");
        assert_eq!(c.language, "pt"); // default preserved
    }

    #[test]
    fn missing_file_yields_defaults() {
        let c = Config::load(Path::new("/nonexistent/voxkey.toml")).unwrap();
        assert_eq!(c.ptt_key, "RightAlt");
    }

    #[test]
    fn zero_max_seconds_is_rejected() {
        let err = toml::from_str::<Config>("max_seconds = 0\n")
            .map_err(|e| e.to_string())
            .and_then(|c| c.validate().map_err(|e| e.to_string()));
        assert!(err.is_err());
    }

    #[test]
    fn default_model_path_matches_model() {
        let c = Config::default();
        assert!(c.resolved_model_path().ends_with("ggml-small-q5_1.bin"));
    }

    #[test]
    fn defaults_include_latch_and_warn() {
        let c = Config::default();
        assert_eq!(c.hold_to_start_ms, 1000);
        assert_eq!(c.warn_seconds, 30);
    }

    #[test]
    fn latch_fields_override_from_toml() {
        let c: Config = toml::from_str("hold_to_start_ms = 1500\nwarn_seconds = 10\n").unwrap();
        assert_eq!(c.hold_to_start_ms, 1500);
        assert_eq!(c.warn_seconds, 10);
    }
}
