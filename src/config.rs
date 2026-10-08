use serde::Deserialize;
use std::path::PathBuf;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub reduced_motion: bool,
    pub dito: Dito,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Dito {
    pub command: String,
    pub root: Option<PathBuf>,
    pub adapter: Option<PathBuf>,
    pub timeout_seconds: u64,
}
impl Default for Dito {
    fn default() -> Self {
        Self {
            command: "dito".into(),
            root: None,
            adapter: None,
            timeout_seconds: 120,
        }
    }
}
impl Config {
    pub fn load(path: Option<&str>) -> anyhow::Result<Self> {
        let default = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
            })
            .join("liqinir/config.toml");
        let path = path.map(PathBuf::from).unwrap_or(default);
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(toml::from_str(&text)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }
}
