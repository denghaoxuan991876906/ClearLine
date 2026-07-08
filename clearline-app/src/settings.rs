use std::{fs, io, path::PathBuf};

use directories::BaseDirs;
use serde::{Deserialize, Serialize};

pub const SETTINGS_VERSION: u32 = 2;
pub const MODE_LOW_LATENCY: &str = "low_latency";
pub const MODE_HIGH_QUALITY: &str = "high_quality";
pub const STRENGTH_GENTLE: &str = "gentle";
pub const STRENGTH_BALANCED: &str = "balanced";
pub const STRENGTH_STRONG: &str = "strong";
pub const OUTPUT_TARGET_VB_CABLE: &str = "vb_cable";
#[cfg(test)]
pub const OUTPUT_TARGET_VIRTUAL_MIC: &str = "clearline_virtual_microphone";
#[cfg(test)]
pub const OUTPUT_TARGET_AUDIO_DEVICE: &str = "audio_device";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedSettings {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub input_device_id: Option<String>,
    #[serde(default)]
    pub input_device_name: Option<String>,
    #[serde(default)]
    pub output_device_id: Option<String>,
    #[serde(default)]
    pub output_device_name: Option<String>,
    #[serde(default = "default_output_target")]
    pub output_target: String,
    #[serde(default = "default_suppressor_mode")]
    pub suppressor_mode: String,
    #[serde(default = "default_suppression_strength")]
    pub suppression_strength: String,
    #[serde(default)]
    pub wind_noise_reduction_enabled: bool,
    #[serde(default = "default_echo_cancellation_enabled")]
    pub echo_cancellation_enabled: bool,
    #[serde(default = "default_noise_suppression_enabled")]
    pub noise_suppression_enabled: bool,
    #[serde(default = "default_microphone_boost_enabled")]
    pub microphone_boost_enabled: bool,
    #[serde(default)]
    pub start_on_login_enabled: bool,
    #[serde(default)]
    pub deepfilter_model_dir: String,
}

impl Default for PersistedSettings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            input_device_id: None,
            input_device_name: None,
            output_device_id: None,
            output_device_name: None,
            output_target: OUTPUT_TARGET_VB_CABLE.to_owned(),
            suppressor_mode: MODE_HIGH_QUALITY.to_owned(),
            suppression_strength: STRENGTH_BALANCED.to_owned(),
            wind_noise_reduction_enabled: false,
            echo_cancellation_enabled: true,
            noise_suppression_enabled: true,
            microphone_boost_enabled: true,
            start_on_login_enabled: false,
            deepfilter_model_dir: String::new(),
        }
    }
}

fn default_version() -> u32 {
    SETTINGS_VERSION
}

fn default_suppressor_mode() -> String {
    MODE_HIGH_QUALITY.to_owned()
}

fn default_suppression_strength() -> String {
    STRENGTH_BALANCED.to_owned()
}

fn default_output_target() -> String {
    OUTPUT_TARGET_VB_CABLE.to_owned()
}

fn default_echo_cancellation_enabled() -> bool {
    true
}

fn default_noise_suppression_enabled() -> bool {
    true
}

fn default_microphone_boost_enabled() -> bool {
    true
}

pub fn suppressor_mode_to_setting(mode: clearline_core::SuppressorMode) -> &'static str {
    match mode {
        clearline_core::SuppressorMode::Bypass => MODE_HIGH_QUALITY,
        clearline_core::SuppressorMode::LowLatency => MODE_HIGH_QUALITY,
        clearline_core::SuppressorMode::HighQuality => MODE_HIGH_QUALITY,
    }
}

pub fn suppressor_mode_from_setting(value: &str) -> clearline_core::SuppressorMode {
    match value {
        MODE_HIGH_QUALITY => clearline_core::SuppressorMode::HighQuality,
        MODE_LOW_LATENCY => clearline_core::SuppressorMode::HighQuality,
        _ => clearline_core::SuppressorMode::HighQuality,
    }
}

pub fn suppression_strength_to_setting(
    strength: clearline_core::SuppressionStrength,
) -> &'static str {
    match strength {
        clearline_core::SuppressionStrength::Gentle => STRENGTH_GENTLE,
        clearline_core::SuppressionStrength::Balanced => STRENGTH_BALANCED,
        clearline_core::SuppressionStrength::Strong => STRENGTH_STRONG,
    }
}

pub fn suppression_strength_from_setting(value: &str) -> clearline_core::SuppressionStrength {
    match value {
        STRENGTH_GENTLE => clearline_core::SuppressionStrength::Gentle,
        STRENGTH_STRONG => clearline_core::SuppressionStrength::Strong,
        STRENGTH_BALANCED => clearline_core::SuppressionStrength::Balanced,
        _ => clearline_core::SuppressionStrength::Balanced,
    }
}

#[derive(Debug)]
pub enum SettingsError {
    Io(io::Error),
    Json(serde_json::Error),
    ConfigDirUnavailable,
}

impl std::fmt::Display for SettingsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "settings io error: {error}"),
            Self::Json(error) => write!(formatter, "failed to parse settings: {error}"),
            Self::ConfigDirUnavailable => formatter.write_str("settings directory is unavailable"),
        }
    }
}

impl std::error::Error for SettingsError {}

impl From<io::Error> for SettingsError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for SettingsError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[derive(Debug, Clone)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new() -> Result<Self, SettingsError> {
        let dirs = BaseDirs::new().ok_or(SettingsError::ConfigDirUnavailable)?;
        Ok(Self::from_path(
            dirs.config_dir().join("ClearLine").join("settings.json"),
        ))
    }

    pub fn from_path(path: PathBuf) -> Self {
        Self { path }
    }

    #[cfg(test)]
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub fn load(&self) -> Result<Option<PersistedSettings>, SettingsError> {
        match fs::read_to_string(&self.path) {
            Ok(text) => serde_json::from_str(&text)
                .map(Some)
                .map_err(SettingsError::from),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(SettingsError::Io(error)),
        }
    }

    pub fn save(&self, settings: &PersistedSettings) -> Result<(), SettingsError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(settings)?;
        fs::write(&self.path, text)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_settings_defaults_are_safe() {
        let settings = PersistedSettings::default();

        assert_eq!(settings.version, 2);
        assert_eq!(settings.output_target, OUTPUT_TARGET_VB_CABLE);
        assert_eq!(settings.suppressor_mode, "high_quality");
        assert_eq!(settings.suppression_strength, "balanced");
        assert!(!settings.wind_noise_reduction_enabled);
        assert!(settings.echo_cancellation_enabled);
        assert!(settings.noise_suppression_enabled);
        assert!(settings.microphone_boost_enabled);
        assert!(!settings.start_on_login_enabled);
        assert!(settings.deepfilter_model_dir.is_empty());
    }

    #[test]
    fn settings_defaults_to_high_quality_and_aec() {
        let settings = PersistedSettings::default();

        assert_eq!(settings.suppressor_mode, MODE_HIGH_QUALITY);
        assert!(settings.echo_cancellation_enabled);
        assert_eq!(default_suppressor_mode(), MODE_HIGH_QUALITY);
        assert_eq!(
            suppressor_mode_from_setting(MODE_LOW_LATENCY),
            clearline_core::SuppressorMode::HighQuality
        );
        assert_eq!(
            suppressor_mode_from_setting("unknown"),
            clearline_core::SuppressorMode::HighQuality
        );
    }

    #[test]
    fn missing_settings_fields_default_to_high_quality_and_aec() {
        let settings: PersistedSettings = serde_json::from_str("{}").unwrap();

        assert_eq!(settings.suppressor_mode, MODE_HIGH_QUALITY);
        assert!(settings.echo_cancellation_enabled);
        assert!(settings.noise_suppression_enabled);
        assert!(settings.microphone_boost_enabled);
        assert!(!settings.start_on_login_enabled);
    }

    #[test]
    fn persisted_settings_round_trips_as_json() {
        let settings = PersistedSettings {
            version: 1,
            input_device_id: Some("input-id".to_owned()),
            input_device_name: Some("MCHOSE V9 Turbo+".to_owned()),
            output_device_id: Some("output-id".to_owned()),
            output_device_name: Some("VB-CABLE Input".to_owned()),
            output_target: OUTPUT_TARGET_AUDIO_DEVICE.to_owned(),
            suppressor_mode: "high_quality".to_owned(),
            suppression_strength: "strong".to_owned(),
            wind_noise_reduction_enabled: true,
            echo_cancellation_enabled: true,
            noise_suppression_enabled: false,
            microphone_boost_enabled: false,
            start_on_login_enabled: true,
            deepfilter_model_dir: r"E:\Dev\模型onnx".to_owned(),
        };

        let json = serde_json::to_string_pretty(&settings).unwrap();
        let restored: PersistedSettings = serde_json::from_str(&json).unwrap();

        assert_eq!(restored, settings);
        assert!(!restored.microphone_boost_enabled);
    }

    #[test]
    fn settings_parse_modes_and_strengths_with_defaults() {
        assert_eq!(
            suppressor_mode_to_setting(clearline_core::SuppressorMode::LowLatency),
            MODE_HIGH_QUALITY
        );
        assert_eq!(
            suppressor_mode_to_setting(clearline_core::SuppressorMode::HighQuality),
            MODE_HIGH_QUALITY
        );
        assert_eq!(
            suppressor_mode_from_setting(MODE_HIGH_QUALITY),
            clearline_core::SuppressorMode::HighQuality
        );
        assert_eq!(
            suppressor_mode_from_setting("unknown"),
            clearline_core::SuppressorMode::HighQuality
        );

        assert_eq!(
            suppression_strength_to_setting(clearline_core::SuppressionStrength::Gentle),
            STRENGTH_GENTLE
        );
        assert_eq!(
            suppression_strength_to_setting(clearline_core::SuppressionStrength::Balanced),
            STRENGTH_BALANCED
        );
        assert_eq!(
            suppression_strength_to_setting(clearline_core::SuppressionStrength::Strong),
            STRENGTH_STRONG
        );
        assert_eq!(
            suppression_strength_from_setting(STRENGTH_STRONG),
            clearline_core::SuppressionStrength::Strong
        );
        assert_eq!(
            suppression_strength_from_setting("unknown"),
            clearline_core::SuppressionStrength::Balanced
        );
    }

    #[test]
    fn settings_store_loads_none_when_file_is_missing() {
        let path = unique_temp_settings_path("missing");
        let store = SettingsStore::from_path(path.clone());

        assert_eq!(store.path(), path.as_path());
        assert_eq!(store.load().unwrap(), None);
        assert!(!path.exists());
    }

    #[test]
    fn settings_store_saves_and_loads_json() {
        let path = unique_temp_settings_path("round-trip");
        let store = SettingsStore::from_path(path.clone());
        let settings = PersistedSettings {
            deepfilter_model_dir: r"E:\Dev\模型onnx".to_owned(),
            suppressor_mode: MODE_HIGH_QUALITY.to_owned(),
            ..PersistedSettings::default()
        };

        store.save(&settings).unwrap();
        let loaded = store.load().unwrap().unwrap();

        assert_eq!(loaded, settings);
        let text = std::fs::read_to_string(path).unwrap();
        assert!(text.contains("deepfilter_model_dir"));
    }

    #[test]
    fn settings_store_reports_invalid_json() {
        let path = unique_temp_settings_path("invalid-json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "not json").unwrap();
        let store = SettingsStore::from_path(path);

        let error = store.load().unwrap_err();

        assert!(error.to_string().contains("failed to parse settings"));
    }

    fn unique_temp_settings_path(prefix: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "clearline-settings-{prefix}-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        path
    }
}
