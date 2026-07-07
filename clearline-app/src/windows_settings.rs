use std::{fmt, io};

pub const SOUND_SETTINGS_URI: &str = "ms-settings:sound";

pub fn sound_settings_uri() -> &'static str {
    SOUND_SETTINGS_URI
}

#[derive(Debug)]
pub enum WindowsSettingsError {
    Io(io::Error),
    #[cfg(not(windows))]
    UnsupportedPlatform,
}

impl fmt::Display for WindowsSettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "打开 Windows 设置失败：{error}"),
            #[cfg(not(windows))]
            Self::UnsupportedPlatform => formatter.write_str("仅 Windows 可用"),
        }
    }
}

impl std::error::Error for WindowsSettingsError {}

impl From<io::Error> for WindowsSettingsError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn open_sound_settings() -> Result<(), WindowsSettingsError> {
    open_settings_uri(sound_settings_uri())
}

pub fn set_start_on_login_enabled(enabled: bool) -> Result<(), WindowsSettingsError> {
    set_startup_run_entry(enabled)
}

pub fn is_start_on_login_enabled() -> bool {
    query_startup_run_entry()
}

#[cfg(windows)]
fn open_settings_uri(uri: &str) -> Result<(), WindowsSettingsError> {
    std::process::Command::new("cmd")
        .args(["/C", "start", "", uri])
        .spawn()?;
    Ok(())
}

#[cfg(not(windows))]
fn open_settings_uri(_uri: &str) -> Result<(), WindowsSettingsError> {
    Err(WindowsSettingsError::UnsupportedPlatform)
}

#[cfg(windows)]
fn set_startup_run_entry(enabled: bool) -> Result<(), WindowsSettingsError> {
    let exe = std::env::current_exe()?;
    let command = format!("\"{}\" --minimized", exe.display());
    let status = if enabled {
        std::process::Command::new("reg.exe")
            .args([
                "add",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                "ClearLine",
                "/t",
                "REG_SZ",
                "/d",
                &command,
                "/f",
            ])
            .status()?
    } else {
        std::process::Command::new("reg.exe")
            .args([
                "delete",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                "ClearLine",
                "/f",
            ])
            .status()?
    };

    if status.success() || !enabled {
        Ok(())
    } else {
        Err(io::Error::other(format!("reg.exe exit status {status}")).into())
    }
}

#[cfg(not(windows))]
fn set_startup_run_entry(_enabled: bool) -> Result<(), WindowsSettingsError> {
    Err(WindowsSettingsError::UnsupportedPlatform)
}

#[cfg(windows)]
fn query_startup_run_entry() -> bool {
    std::process::Command::new("reg.exe")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            "ClearLine",
        ])
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(not(windows))]
fn query_startup_run_entry() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sound_settings_uri_targets_windows_sound_settings() {
        assert_eq!(sound_settings_uri(), "ms-settings:sound");
    }

    #[cfg(not(windows))]
    #[test]
    fn open_sound_settings_reports_unsupported_platform() {
        let error = open_sound_settings().unwrap_err();

        assert!(error.to_string().contains("仅 Windows 可用"));
    }

    #[cfg(not(windows))]
    #[test]
    fn startup_setting_reports_unsupported_platform() {
        let error = set_start_on_login_enabled(true).unwrap_err();

        assert!(error.to_string().contains("仅 Windows 可用"));
        assert!(!is_start_on_login_enabled());
    }
}
