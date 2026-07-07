# User Settings Persistence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist ClearLine's local user choices to `%APPDATA%\ClearLine\settings.json` and restore them on the next launch.

**Architecture:** Add an app-owned `settings` module in `clearline-app` that serializes/deserializes a small JSON settings model with `serde`. `ClearLineApp` loads settings before/around device refresh, applies non-device fields immediately, resolves saved devices by ID then name, and saves only after user-initiated changes. The UI layout remains unchanged and uses the existing status message for save/load/fallback feedback.

**Tech Stack:** Rust 2021, `serde`, `serde_json`, `directories`, existing `eframe/egui`, existing `clearline-core` device and suppressor types.

---

## File Map

- Modify `Cargo.toml`
  - Add workspace dependencies: `directories`, `serde`, `serde_json`.
- Modify `clearline-app/Cargo.toml`
  - Add app dependencies for settings persistence.
- Create `clearline-app/src/settings.rs`
  - Define `PersistedSettings`, `SettingsStore`, parse/format helpers, and JSON load/save tests.
- Modify `clearline-app/src/main.rs`
  - Add `mod settings;` and integrate `SettingsStore`.
  - Add pending loaded settings state.
  - Apply settings during startup refresh.
  - Save settings when the user changes persistent fields.
  - Add tests for device resolution and snapshot generation.
- Modify `README.md`
  - Document local settings file and persisted fields.
- Build artifacts after final verification
  - Rebuild Windows release exe and copy to `dist/ClearLine.exe`.

---

### Task 1: Add dependencies and settings module skeleton

**Files:**
- Modify: `Cargo.toml`
- Modify: `clearline-app/Cargo.toml`
- Create: `clearline-app/src/settings.rs`
- Modify: `clearline-app/src/main.rs`

- [ ] **Step 1: Write failing settings tests**

Create `clearline-app/src/settings.rs` with only this initial test module and minimal imports:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_settings_defaults_are_safe() {
        let settings = PersistedSettings::default();

        assert_eq!(settings.version, 1);
        assert_eq!(settings.suppressor_mode, "low_latency");
        assert_eq!(settings.suppression_strength, "balanced");
        assert!(!settings.wind_noise_reduction_enabled);
        assert!(settings.deepfilter_model_dir.is_empty());
    }

    #[test]
    fn persisted_settings_round_trips_as_json() {
        let settings = PersistedSettings {
            version: 1,
            input_device_id: Some("input-id".to_owned()),
            input_device_name: Some("MCHOSE V9 Turbo+".to_owned()),
            output_device_id: Some("output-id".to_owned()),
            output_device_name: Some("VB-CABLE Input".to_owned()),
            suppressor_mode: "high_quality".to_owned(),
            suppression_strength: "strong".to_owned(),
            wind_noise_reduction_enabled: true,
            deepfilter_model_dir: r"E:\Dev\模型onnx".to_owned(),
        };

        let json = serde_json::to_string_pretty(&settings).unwrap();
        let restored: PersistedSettings = serde_json::from_str(&json).unwrap();

        assert_eq!(restored, settings);
    }
}
```

In `clearline-app/src/main.rs`, add the module declaration near the top:

```rust
mod settings;
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test -p clearline-app persisted_settings_
```

Expected: compile failure because `serde`, `serde_json`, and `PersistedSettings` are not defined.

- [ ] **Step 3: Add dependencies**

In root `Cargo.toml`, add workspace dependencies:

```toml
directories = "6.0.0"
serde = { version = "1.0.228", features = ["derive"] }
serde_json = "1.0.145"
```

In `clearline-app/Cargo.toml`, add:

```toml
directories.workspace = true
serde.workspace = true
serde_json.workspace = true
```

- [ ] **Step 4: Implement `PersistedSettings`**

Replace the top of `clearline-app/src/settings.rs` with:

```rust
use serde::{Deserialize, Serialize};

pub const SETTINGS_VERSION: u32 = 1;
pub const MODE_LOW_LATENCY: &str = "low_latency";
pub const MODE_HIGH_QUALITY: &str = "high_quality";
pub const STRENGTH_GENTLE: &str = "gentle";
pub const STRENGTH_BALANCED: &str = "balanced";
pub const STRENGTH_STRONG: &str = "strong";

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
    #[serde(default = "default_suppressor_mode")]
    pub suppressor_mode: String,
    #[serde(default = "default_suppression_strength")]
    pub suppression_strength: String,
    #[serde(default)]
    pub wind_noise_reduction_enabled: bool,
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
            suppressor_mode: MODE_LOW_LATENCY.to_owned(),
            suppression_strength: STRENGTH_BALANCED.to_owned(),
            wind_noise_reduction_enabled: false,
            deepfilter_model_dir: String::new(),
        }
    }
}

fn default_version() -> u32 {
    SETTINGS_VERSION
}

fn default_suppressor_mode() -> String {
    MODE_LOW_LATENCY.to_owned()
}

fn default_suppression_strength() -> String {
    STRENGTH_BALANCED.to_owned()
}
```

Keep the tests from Step 1 below this code.

- [ ] **Step 5: Run tests and verify pass**

Run:

```bash
cargo test -p clearline-app persisted_settings_
```

Expected: 2 tests pass.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock clearline-app/Cargo.toml clearline-app/src/settings.rs clearline-app/src/main.rs
git commit -m "feat: add persisted settings model"
```

---

### Task 2: Implement settings file load/save store

**Files:**
- Modify: `clearline-app/src/settings.rs`

- [ ] **Step 1: Write failing store tests**

Add these tests to `clearline-app/src/settings.rs` inside the existing test module:

```rust
#[test]
fn settings_store_loads_none_when_file_is_missing() {
    let path = unique_temp_settings_path("missing");
    let store = SettingsStore::from_path(path.clone());

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
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test -p clearline-app settings_store_
```

Expected: compile failure because `SettingsStore` is not defined.

- [ ] **Step 3: Implement `SettingsStore` and error type**

Add these imports at the top of `clearline-app/src/settings.rs`:

```rust
use std::{fs, io, path::PathBuf};

use directories::BaseDirs;
```

Add this code after `PersistedSettings` defaults:

```rust
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

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub fn load(&self) -> Result<Option<PersistedSettings>, SettingsError> {
        match fs::read_to_string(&self.path) {
            Ok(text) => serde_json::from_str(&text).map(Some).map_err(SettingsError::from),
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
```

- [ ] **Step 4: Run tests and verify pass**

Run:

```bash
cargo test -p clearline-app settings_store_
```

Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add clearline-app/src/settings.rs
git commit -m "feat: add settings file store"
```

---

### Task 3: Add conversion helpers and device restore functions

**Files:**
- Modify: `clearline-app/src/settings.rs`
- Modify: `clearline-app/src/main.rs`

- [ ] **Step 1: Write failing conversion tests in settings module**

Add to `clearline-app/src/settings.rs` tests:

```rust
#[test]
fn settings_parse_modes_and_strengths_with_defaults() {
    assert_eq!(suppressor_mode_to_setting(clearline_core::SuppressorMode::LowLatency), MODE_LOW_LATENCY);
    assert_eq!(suppressor_mode_to_setting(clearline_core::SuppressorMode::HighQuality), MODE_HIGH_QUALITY);
    assert_eq!(suppressor_mode_from_setting(MODE_HIGH_QUALITY), clearline_core::SuppressorMode::HighQuality);
    assert_eq!(suppressor_mode_from_setting("unknown"), clearline_core::SuppressorMode::LowLatency);

    assert_eq!(suppression_strength_to_setting(clearline_core::SuppressionStrength::Gentle), STRENGTH_GENTLE);
    assert_eq!(suppression_strength_to_setting(clearline_core::SuppressionStrength::Balanced), STRENGTH_BALANCED);
    assert_eq!(suppression_strength_to_setting(clearline_core::SuppressionStrength::Strong), STRENGTH_STRONG);
    assert_eq!(suppression_strength_from_setting(STRENGTH_STRONG), clearline_core::SuppressionStrength::Strong);
    assert_eq!(suppression_strength_from_setting("unknown"), clearline_core::SuppressionStrength::Balanced);
}
```

- [ ] **Step 2: Write failing device restore tests in app module**

Add to `clearline-app/src/main.rs` tests:

```rust
#[test]
fn restore_input_device_prefers_id_then_name_then_default() {
    let devices = vec![
        AudioInputDevice::new("default-id", "Default Mic", true),
        AudioInputDevice::new("saved-id", "Saved Mic", false),
        AudioInputDevice::new("name-id", "Name Mic", false),
    ];

    assert_eq!(
        resolve_input_device_from_settings(Some("saved-id"), Some("Name Mic"), &devices)
            .unwrap()
            .as_str(),
        "saved-id"
    );
    assert_eq!(
        resolve_input_device_from_settings(Some("missing-id"), Some("Name Mic"), &devices)
            .unwrap()
            .as_str(),
        "name-id"
    );
    assert_eq!(
        resolve_input_device_from_settings(Some("missing-id"), Some("Missing Mic"), &devices)
            .unwrap()
            .as_str(),
        "default-id"
    );
}

#[test]
fn restore_output_device_prefers_id_then_name_then_default() {
    let devices = vec![
        AudioOutputDevice::new("default-out", "Default Speaker", true),
        AudioOutputDevice::new("saved-out", "Saved Output", false),
        AudioOutputDevice::new("name-out", "Name Output", false),
    ];

    assert_eq!(
        resolve_output_device_from_settings(Some("saved-out"), Some("Name Output"), &devices)
            .unwrap()
            .as_str(),
        "saved-out"
    );
    assert_eq!(
        resolve_output_device_from_settings(Some("missing-out"), Some("Name Output"), &devices)
            .unwrap()
            .as_str(),
        "name-out"
    );
    assert_eq!(
        resolve_output_device_from_settings(Some("missing-out"), Some("Missing Output"), &devices)
            .unwrap()
            .as_str(),
        "default-out"
    );
}
```

- [ ] **Step 3: Run tests and verify failure**

Run:

```bash
cargo test -p clearline-app settings_parse_modes_and_strengths_with_defaults
cargo test -p clearline-app restore_input_device_prefers_id_then_name_then_default
cargo test -p clearline-app restore_output_device_prefers_id_then_name_then_default
```

Expected: compile failure because conversion and restore helpers are missing.

- [ ] **Step 4: Implement conversion helpers**

In `clearline-app/src/settings.rs`, add:

```rust
pub fn suppressor_mode_to_setting(mode: clearline_core::SuppressorMode) -> &'static str {
    match mode {
        clearline_core::SuppressorMode::Bypass => MODE_LOW_LATENCY,
        clearline_core::SuppressorMode::LowLatency => MODE_LOW_LATENCY,
        clearline_core::SuppressorMode::HighQuality => MODE_HIGH_QUALITY,
    }
}

pub fn suppressor_mode_from_setting(value: &str) -> clearline_core::SuppressorMode {
    match value {
        MODE_HIGH_QUALITY => clearline_core::SuppressorMode::HighQuality,
        MODE_LOW_LATENCY => clearline_core::SuppressorMode::LowLatency,
        _ => clearline_core::SuppressorMode::LowLatency,
    }
}

pub fn suppression_strength_to_setting(strength: clearline_core::SuppressionStrength) -> &'static str {
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
```

- [ ] **Step 5: Import settings helpers and implement restore helpers**

In `clearline-app/src/main.rs`, add this import near the existing `use clearline_core` block:

```rust
use settings::{
    PersistedSettings, SettingsStore, suppression_strength_from_setting,
    suppression_strength_to_setting, suppressor_mode_from_setting, suppressor_mode_to_setting,
};
```

Then add helper functions near `default_suppressor_mode()`:

```rust
fn resolve_input_device_from_settings(
    saved_id: Option<&str>,
    saved_name: Option<&str>,
    devices: &[AudioInputDevice],
) -> Option<DeviceId> {
    resolve_device_id_from_settings(
        saved_id,
        saved_name,
        devices.iter().map(|device| (device.id(), device.name(), device.is_default())),
    )
}

fn resolve_output_device_from_settings(
    saved_id: Option<&str>,
    saved_name: Option<&str>,
    devices: &[AudioOutputDevice],
) -> Option<DeviceId> {
    resolve_device_id_from_settings(
        saved_id,
        saved_name,
        devices.iter().map(|device| (device.id(), device.name(), device.is_default())),
    )
}

fn resolve_device_id_from_settings<'a>(
    saved_id: Option<&str>,
    saved_name: Option<&str>,
    devices: impl Iterator<Item = (&'a DeviceId, &'a str, bool)>,
) -> Option<DeviceId> {
    let devices = devices.collect::<Vec<_>>();
    if let Some(saved_id) = saved_id.filter(|value| !value.trim().is_empty()) {
        if let Some((id, _, _)) = devices.iter().find(|(id, _, _)| id.as_str() == saved_id) {
            return Some((*id).clone());
        }
    }

    if let Some(saved_name) = saved_name.filter(|value| !value.trim().is_empty()) {
        if let Some((id, _, _)) = devices.iter().find(|(_, name, _)| *name == saved_name) {
            return Some((*id).clone());
        }
    }

    devices
        .iter()
        .find(|(_, _, is_default)| *is_default)
        .or_else(|| devices.first())
        .map(|(id, _, _)| (*id).clone())
}
```

- [ ] **Step 6: Run tests and verify pass**

Run:

```bash
cargo test -p clearline-app settings_parse_modes_and_strengths_with_defaults
cargo test -p clearline-app restore_input_device_prefers_id_then_name_then_default
cargo test -p clearline-app restore_output_device_prefers_id_then_name_then_default
```

Expected: all targeted tests pass.

- [ ] **Step 7: Commit**

```bash
git add clearline-app/src/settings.rs clearline-app/src/main.rs
git commit -m "feat: add settings conversion and device restore"
```

---

### Task 4: Integrate settings into app startup and snapshots

**Files:**
- Modify: `clearline-app/src/main.rs`

- [ ] **Step 1: Write failing app snapshot/apply tests**

Add to `clearline-app/src/main.rs` tests:

```rust
#[test]
fn app_settings_snapshot_includes_current_choices() {
    let mut app = ClearLineApp::new_without_loading_settings_for_tests();
    app.devices = vec![AudioInputDevice::new("mic-id", "Saved Mic", true)];
    app.output_devices = vec![AudioOutputDevice::new("out-id", "Saved Output", true)];
    app.selected_device_id = Some(DeviceId::new("mic-id"));
    app.selected_output_device_id = Some(DeviceId::new("out-id"));
    app.suppressor_mode = SuppressorModeSelection(SuppressorMode::HighQuality);
    app.suppression_strength = clearline_core::SuppressionStrength::Strong;
    app.wind_noise_reduction_enabled = true;
    app.deepfilter_model_dir = r"E:\Dev\模型onnx".to_owned();

    let settings = app.persisted_settings_snapshot();

    assert_eq!(settings.input_device_id.as_deref(), Some("mic-id"));
    assert_eq!(settings.input_device_name.as_deref(), Some("Saved Mic"));
    assert_eq!(settings.output_device_id.as_deref(), Some("out-id"));
    assert_eq!(settings.output_device_name.as_deref(), Some("Saved Output"));
    assert_eq!(settings.suppressor_mode, settings::MODE_HIGH_QUALITY);
    assert_eq!(settings.suppression_strength, settings::STRENGTH_STRONG);
    assert!(settings.wind_noise_reduction_enabled);
    assert_eq!(settings.deepfilter_model_dir, r"E:\Dev\模型onnx");
}

#[test]
fn app_applies_loaded_settings_to_fields_and_devices() {
    let mut app = ClearLineApp::new_without_loading_settings_for_tests();
    app.devices = vec![
        AudioInputDevice::new("default-mic", "Default Mic", true),
        AudioInputDevice::new("saved-mic", "Saved Mic", false),
    ];
    app.output_devices = vec![
        AudioOutputDevice::new("default-out", "Default Output", true),
        AudioOutputDevice::new("saved-out", "Saved Output", false),
    ];
    app.pending_settings = Some(PersistedSettings {
        input_device_id: Some("saved-mic".to_owned()),
        input_device_name: Some("Saved Mic".to_owned()),
        output_device_id: Some("saved-out".to_owned()),
        output_device_name: Some("Saved Output".to_owned()),
        suppressor_mode: settings::MODE_HIGH_QUALITY.to_owned(),
        suppression_strength: settings::STRENGTH_GENTLE.to_owned(),
        wind_noise_reduction_enabled: true,
        deepfilter_model_dir: r"E:\Dev\模型onnx".to_owned(),
        ..PersistedSettings::default()
    });

    app.apply_pending_settings_after_refresh();

    assert_eq!(app.selected_device_id.as_ref().unwrap().as_str(), "saved-mic");
    assert_eq!(app.selected_output_device_id.as_ref().unwrap().as_str(), "saved-out");
    assert_eq!(app.suppressor_mode.value(), SuppressorMode::HighQuality);
    assert_eq!(app.suppression_strength, clearline_core::SuppressionStrength::Gentle);
    assert!(app.wind_noise_reduction_enabled);
    assert_eq!(app.deepfilter_model_dir, r"E:\Dev\模型onnx");
    assert!(app.pending_settings.is_none());
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test -p clearline-app app_settings_snapshot_includes_current_choices
cargo test -p clearline-app app_applies_loaded_settings_to_fields_and_devices
```

Expected: compile failure because `pending_settings`, `new_without_loading_settings_for_tests`, and snapshot/apply methods do not exist.

- [ ] **Step 3: Extend `ClearLineApp` fields and constructors**

In `ClearLineApp`, add fields:

```rust
settings_store: Option<SettingsStore>,
pending_settings: Option<PersistedSettings>,
settings_loaded: bool,
```

Replace `ClearLineApp::new()` with:

```rust
fn new() -> Self {
    let (settings_store, pending_settings, status_message, settings_loaded) = match SettingsStore::new() {
        Ok(store) => match store.load() {
            Ok(settings) => {
                let loaded = settings.is_some();
                let status = if loaded {
                    "已加载本地设置".to_owned()
                } else {
                    "正在初始化设备列表".to_owned()
                };
                (Some(store), settings, status, loaded)
            }
            Err(error) => (
                Some(store),
                None,
                format!("设置文件无效，已使用默认设置：{error}"),
                false,
            ),
        },
        Err(error) => (
            None,
            None,
            format!("设置目录不可用，已使用默认设置：{error}"),
            false,
        ),
    };

    let mut app = Self::new_with_settings_for_tests(settings_store, pending_settings, status_message, settings_loaded);
    app.refresh_devices();
    app
}

fn new_with_settings_for_tests(
    settings_store: Option<SettingsStore>,
    pending_settings: Option<PersistedSettings>,
    status_message: String,
    settings_loaded: bool,
) -> Self {
    Self {
        enumerator: CpalDeviceEnumerator,
        devices: Vec::new(),
        output_devices: Vec::new(),
        selected_device_id: None,
        selected_output_device_id: None,
        suppressor_mode: SuppressorModeSelection(default_suppressor_mode()),
        suppression_strength: SuppressionStrength::default(),
        wind_noise_reduction_enabled: false,
        deepfilter_model_dir: String::new(),
        selected_tab: default_app_tab(),
        pipeline: AudioPipeline::new(),
        input_level: 0.0,
        status_message,
        settings_store,
        pending_settings,
        settings_loaded,
    }
}

#[cfg(test)]
fn new_without_loading_settings_for_tests() -> Self {
    Self::new_with_settings_for_tests(None, None, "正在初始化设备列表".to_owned(), false)
}
```

- [ ] **Step 4: Implement settings apply and snapshot methods**

Add to `impl ClearLineApp`:

```rust
fn apply_pending_settings_after_refresh(&mut self) {
    let Some(settings) = self.pending_settings.take() else {
        return;
    };

    self.suppressor_mode = SuppressorModeSelection(suppressor_mode_from_setting(&settings.suppressor_mode));
    self.suppression_strength = suppression_strength_from_setting(&settings.suppression_strength);
    self.wind_noise_reduction_enabled = settings.wind_noise_reduction_enabled;
    self.deepfilter_model_dir = settings.deepfilter_model_dir.clone();

    self.selected_device_id = resolve_input_device_from_settings(
        settings.input_device_id.as_deref(),
        settings.input_device_name.as_deref(),
        &self.devices,
    );
    self.selected_output_device_id = resolve_output_device_from_settings(
        settings.output_device_id.as_deref(),
        settings.output_device_name.as_deref(),
        &self.output_devices,
    );

    let input_fell_back = settings.input_device_id.as_deref().is_some_and(|saved| {
        self.selected_device_id.as_ref().map(DeviceId::as_str) != Some(saved)
    });
    let output_fell_back = settings.output_device_id.as_deref().is_some_and(|saved| {
        self.selected_output_device_id.as_ref().map(DeviceId::as_str) != Some(saved)
    });

    if input_fell_back || output_fell_back {
        self.status_message = "已加载本地设置，部分设备不可用，已使用可用设备".to_owned();
    } else if self.settings_loaded {
        self.status_message = "已加载本地设置".to_owned();
    }
}

fn persisted_settings_snapshot(&self) -> PersistedSettings {
    let input = self.selected_device();
    let output = self.selected_output_device();
    PersistedSettings {
        version: settings::SETTINGS_VERSION,
        input_device_id: self.selected_device_id.as_ref().map(|id| id.as_str().to_owned()),
        input_device_name: input.map(|device| device.name().to_owned()),
        output_device_id: self
            .selected_output_device_id
            .as_ref()
            .map(|id| id.as_str().to_owned()),
        output_device_name: output.map(|device| device.name().to_owned()),
        suppressor_mode: suppressor_mode_to_setting(self.suppressor_mode.value()).to_owned(),
        suppression_strength: suppression_strength_to_setting(self.suppression_strength).to_owned(),
        wind_noise_reduction_enabled: self.wind_noise_reduction_enabled,
        deepfilter_model_dir: self.deepfilter_model_dir.clone(),
    }
}
```

- [ ] **Step 5: Call apply from refresh flow**

In `refresh_devices`, after `self.ensure_selected_output_device();`, insert:

```rust
self.apply_pending_settings_after_refresh();
```

Then only set the generic device-count `status_message` if it was not just set by loaded settings. Replace the current assignment with:

```rust
if self.devices.is_empty()
    || self.output_devices.is_empty()
    || self.status_message == "正在初始化设备列表"
{
    self.status_message = match (self.devices.is_empty(), self.output_devices.is_empty()) {
        (true, true) => "未找到输入和输出设备。请检查 Windows 音频设备。".to_owned(),
        (true, false) => "未找到输入设备。请检查 Windows 麦克风权限和录音设备。".to_owned(),
        (false, true) => "未找到输出设备。请检查 Windows 播放设备。".to_owned(),
        (false, false) => format!(
            "已找到 {} 个输入设备、{} 个输出设备",
            self.devices.len(),
            self.output_devices.len()
        ),
    };
}
```

- [ ] **Step 6: Run tests and verify pass**

Run:

```bash
cargo test -p clearline-app app_settings_snapshot_includes_current_choices
cargo test -p clearline-app app_applies_loaded_settings_to_fields_and_devices
```

Expected: both tests pass.

- [ ] **Step 7: Commit**

```bash
git add clearline-app/src/main.rs
git commit -m "feat: restore persisted app settings"
```

---

### Task 5: Save settings on user changes

**Files:**
- Modify: `clearline-app/src/main.rs`

- [ ] **Step 1: Write failing save helper tests**

Add to `clearline-app/src/main.rs` tests:

```rust
#[test]
fn save_settings_writes_current_snapshot() {
    let path = unique_temp_settings_path("app-save");
    let store = SettingsStore::from_path(path.clone());
    let mut app = ClearLineApp::new_with_settings_for_tests(
        Some(store),
        None,
        "正在初始化设备列表".to_owned(),
        false,
    );
    app.devices = vec![AudioInputDevice::new("mic-id", "Saved Mic", true)];
    app.output_devices = vec![AudioOutputDevice::new("out-id", "Saved Output", true)];
    app.selected_device_id = Some(DeviceId::new("mic-id"));
    app.selected_output_device_id = Some(DeviceId::new("out-id"));
    app.deepfilter_model_dir = r"E:\Dev\模型onnx".to_owned();

    app.save_settings_after_user_change();

    let loaded = SettingsStore::from_path(path).load().unwrap().unwrap();
    assert_eq!(loaded.input_device_id.as_deref(), Some("mic-id"));
    assert_eq!(loaded.output_device_id.as_deref(), Some("out-id"));
    assert_eq!(loaded.deepfilter_model_dir, r"E:\Dev\模型onnx");
}

fn unique_temp_settings_path(prefix: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "clearline-app-settings-{prefix}-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    path
}
```

- [ ] **Step 2: Run test and verify failure**

Run:

```bash
cargo test -p clearline-app save_settings_writes_current_snapshot
```

Expected: compile failure because `save_settings_after_user_change` is not defined.

- [ ] **Step 3: Implement save helper**

Add to `impl ClearLineApp`:

```rust
fn save_settings_after_user_change(&mut self) {
    let Some(store) = self.settings_store.clone() else {
        return;
    };
    let settings = self.persisted_settings_snapshot();
    match store.save(&settings) {
        Ok(()) => {
            if !self.pipeline.state().is_running() {
                self.status_message = "设置已保存".to_owned();
            }
        }
        Err(error) => {
            self.status_message = format!("设置保存失败：{error}");
        }
    }
}
```

- [ ] **Step 4: Call save helper from UI interactions**

In `device_card`, avoid borrowing `self.devices` while saving by cloning the device list before the combo box loop:

```rust
let devices = self.devices.clone();
egui::ComboBox::from_id_salt("input-device-selector")
    .width(ui.available_width() - 92.0)
    .selected_text(self.selected_device_label())
    .show_ui(ui, |ui| {
        for device in devices {
            let is_selected = self.selected_device_id.as_ref() == Some(device.id());
            if ui.selectable_label(is_selected, device_label(&device)).clicked() {
                self.selected_device_id = Some(device.id().clone());
                self.save_settings_after_user_change();
            }
        }
    });
```

In `output_device_card`, use the same clone-before-loop pattern:

```rust
let output_devices = self.output_devices.clone();
egui::ComboBox::from_id_salt("output-device-selector")
    .width(ui.available_width() - 92.0)
    .selected_text(self.selected_output_device_label())
    .show_ui(ui, |ui| {
        for device in output_devices {
            let is_selected = self.selected_output_device_id.as_ref() == Some(device.id());
            if ui.selectable_label(is_selected, output_device_label(&device)).clicked() {
                self.selected_output_device_id = Some(device.id().clone());
                self.save_settings_after_user_change();
            }
        }
    });
```

In `mode_card`, after changing mode, strength, wind toggle, model path, and clear button, call save:

```rust
if ui.add(button).clicked() {
    self.suppressor_mode = SuppressorModeSelection(mode);
    self.save_settings_after_user_change();
}
```

```rust
if ui.add_enabled(!is_running, button).clicked() {
    self.suppression_strength = strength;
    self.save_settings_after_user_change();
}
```

```rust
if ui.add_enabled(!is_running, button).clicked() {
    self.wind_noise_reduction_enabled = !self.wind_noise_reduction_enabled;
    self.save_settings_after_user_change();
}
```

For the text edit, capture the response:

```rust
let response = ui.add(
    egui::TextEdit::singleline(&mut self.deepfilter_model_dir)
        .hint_text(r"C:\Models\DeepFilterNet")
        .desired_width(edit_width),
);
if response.changed() {
    self.save_settings_after_user_change();
}
```

For the clear button:

```rust
self.deepfilter_model_dir.clear();
self.save_settings_after_user_change();
```

- [ ] **Step 5: Run tests and verify pass**

Run:

```bash
cargo test -p clearline-app save_settings_writes_current_snapshot
cargo test -p clearline-app
```

Expected: targeted test passes; all app tests pass.

- [ ] **Step 6: Commit**

```bash
git add clearline-app/src/main.rs
git commit -m "feat: save settings on user changes"
```

---

### Task 6: Document settings and run full verification

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Update README**

Add this bullet to the current status list near the UI bullets:

```markdown
- 本地设置会保存到 `%APPDATA%\ClearLine\settings.json`：输入/输出设备、降噪模式、强度、抗风噪开关和 DeepFilterNet 模型目录会在下次启动时恢复。
```

Add this item to the testing or development section:

```markdown
- 设置文件：Windows 下位于 `%APPDATA%\ClearLine\settings.json`。删除该文件可恢复默认选择。
```

- [ ] **Step 2: Run full WSL verification**

Run:

```bash
cargo fmt
cargo fmt --check
cargo check
cargo test -p clearline-core
cargo test -p clearline-core --features rnnoise
cargo test -p clearline-core --features deepfilternet
cargo test -p clearline-core --features rnnoise,deepfilternet
cargo test -p clearline-app
CLEARLINE_DF_MODEL_DIR='/mnt/e/Dev/模型onnx' cargo test -p clearline-core --features deepfilternet high_quality_runs_downloaded_deepfilternet_model -- --ignored --nocapture
cargo check -p clearline-app --no-default-features
```

Expected: all commands exit 0.

- [ ] **Step 3: Commit docs/fixes**

```bash
git add README.md
git commit -m "docs: document local settings file"
```

If verification required code fixes, include the fixed files in the same commit only if they directly relate to settings persistence.

- [ ] **Step 4: Run Windows verification and build exe after final commit**

Run:

```bash
'/mnt/c/Users/DHX/.cargo/bin/cargo.exe' check -p clearline-app
'/mnt/c/Users/DHX/.cargo/bin/cargo.exe' check -p clearline-app --no-default-features
'/mnt/c/Users/DHX/.cargo/bin/cargo.exe' build -p clearline-app --release
mkdir -p dist
cp target/release/clearline-app.exe dist/ClearLine.exe
cp target/release/clearline-app.exe dist/ClearLine-settings.exe
file dist/ClearLine.exe dist/ClearLine-settings.exe
strings -a dist/ClearLine.exe | grep -F "$(git rev-parse --short HEAD)" | head -n 5
```

Expected: Windows checks and release build exit 0; `file` reports PE32+ GUI x86-64 executables; `strings` shows the current git commit.

- [ ] **Step 5: Final status check**

Run:

```bash
git status --short
git log --oneline -8
```

Expected: no uncommitted source/doc changes. `dist/` and `*.exe` are ignored by git, so mention built exe paths in the final response.

---

## Manual Testing Instructions After Implementation

1. Run `E:\Dev\ClearLine\dist\ClearLine.exe`.
2. Select the desired input microphone.
3. Select the desired output device.
4. Select `高质量降噪` or `低延迟降噪`.
5. Change strength, wind-noise option, and DeepFilterNet model directory.
6. Confirm status briefly shows `设置已保存` when not running.
7. Close ClearLine.
8. Reopen ClearLine.
9. Confirm the same choices are restored.
10. Confirm `%APPDATA%\ClearLine\settings.json` exists.
11. Delete `%APPDATA%\ClearLine\settings.json` and reopen ClearLine.
12. Confirm ClearLine falls back to default selections.
13. Optional device fallback test: disable/unplug the saved device, reopen, and confirm ClearLine selects an available/default device instead of crashing.
