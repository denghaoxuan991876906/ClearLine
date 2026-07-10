use std::{
    fs::{self, File},
    io::{Read, Write},
    path::PathBuf,
    process::Command,
    sync::mpsc,
    thread,
};

use anyhow::{anyhow, bail, Context, Result};
use directories::BaseDirs;
use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const UPDATE_MANIFEST_URL: &str =
    "https://github.com/denghaoxuan991876906/ClearLine/releases/latest/download/update.json";

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateManifest {
    pub version: String,
    pub installer_url: String,
    pub sha256: String,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug)]
pub enum UpdateEvent {
    Checked(Option<UpdateManifest>),
    Downloaded {
        manifest: UpdateManifest,
        installer: PathBuf,
    },
    Failed(String),
}

pub fn check(current_version: &str, sender: mpsc::Sender<UpdateEvent>) {
    let current_version = current_version.to_owned();
    thread::spawn(move || {
        let result = fetch_manifest().and_then(|manifest| {
            if is_newer_version(&manifest.version, &current_version)? {
                Ok(Some(manifest))
            } else {
                Ok(None)
            }
        });
        let _ = sender.send(match result {
            Ok(manifest) => UpdateEvent::Checked(manifest),
            Err(error) => UpdateEvent::Failed(error.to_string()),
        });
    });
}

pub fn download(manifest: UpdateManifest, sender: mpsc::Sender<UpdateEvent>) {
    thread::spawn(move || {
        let result = download_installer(&manifest);
        let _ = sender.send(match result {
            Ok(installer) => UpdateEvent::Downloaded {
                manifest,
                installer,
            },
            Err(error) => UpdateEvent::Failed(error.to_string()),
        });
    });
}

pub fn launch_installer(path: &std::path::Path) -> Result<()> {
    Command::new(path)
        .spawn()
        .with_context(|| format!("无法启动更新安装包：{}", path.display()))?;
    Ok(())
}

fn fetch_manifest() -> Result<UpdateManifest> {
    let mut response = ureq::get(UPDATE_MANIFEST_URL)
        .call()
        .context("无法连接 GitHub Releases")?;
    let bytes = response
        .body_mut()
        .read_to_vec()
        .context("无法读取更新清单")?;
    serde_json::from_slice(&bytes).context("更新清单格式无效")
}

fn download_installer(manifest: &UpdateManifest) -> Result<PathBuf> {
    let dirs = BaseDirs::new().ok_or_else(|| anyhow!("无法确定本地更新目录"))?;
    let update_dir = dirs
        .data_local_dir()
        .join("ClearLine")
        .join("updates")
        .join(&manifest.version);
    fs::create_dir_all(&update_dir)?;
    let temporary = update_dir.join("ClearLineSetup.exe.download");
    let installer = update_dir.join("ClearLineSetup.exe");

    let mut response = ureq::get(&manifest.installer_url)
        .call()
        .context("下载安装包失败")?;
    let mut reader = response.body_mut().as_reader();
    let mut file = File::create(&temporary)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])?;
        hasher.update(&buffer[..read]);
    }
    file.flush()?;

    let actual = format!("{:X}", hasher.finalize());
    if !actual.eq_ignore_ascii_case(manifest.sha256.trim()) {
        let _ = fs::remove_file(&temporary);
        bail!("安装包 SHA256 校验失败");
    }
    if installer.exists() {
        fs::remove_file(&installer)?;
    }
    fs::rename(temporary, &installer)?;
    Ok(installer)
}

fn is_newer_version(candidate: &str, current: &str) -> Result<bool> {
    Ok(parse_version(candidate)? > parse_version(current)?)
}

fn parse_version(value: &str) -> Result<(u32, u32, u32)> {
    let value = value.trim().trim_start_matches('v');
    let core = value.split_once('-').map_or(value, |(core, _)| core);
    let mut parts = core.split('.');
    let major = parts.next().unwrap_or("0").parse()?;
    let minor = parts.next().unwrap_or("0").parse()?;
    let patch = parts.next().unwrap_or("0").parse()?;
    if parts.next().is_some() {
        bail!("版本号无效：{value}");
    }
    Ok((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_release_versions() {
        assert!(is_newer_version("0.2.0", "0.1.9").unwrap());
        assert!(is_newer_version("v1.0.0", "0.9.9").unwrap());
        assert!(!is_newer_version("0.1.0", "0.1.0").unwrap());
        assert!(!is_newer_version("0.0.9", "0.1.0").unwrap());
    }
}
