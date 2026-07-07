#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![cfg_attr(not(windows), allow(dead_code, unused_imports))]

use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use anyhow::{anyhow, bail, Result};

#[cfg_attr(not(windows), allow(dead_code))]
mod payload {
    include!(concat!(env!("OUT_DIR"), "/payload_manifest.rs"));
}

const APP_NAME: &str = "ClearLine";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const SETUP_EXE_NAME: &str = "ClearLineSetup.exe";
const UNINSTALL_EXE_NAME: &str = "ClearLineUninstall.exe";
const HELPER_RELATIVE_PATH: &str = "installer/clearline-installer-helper.exe";
const VB_CABLE_PAYLOAD_DIR: &str = "virtual-audio/vb-cable";
const VB_CABLE_ZIP_NAME: &str = "VBCABLE_Driver_Pack45.zip";
const VB_CABLE_EXTRACTED_DIR_NAME: &str = "VBCABLE_Driver_Pack45";
const VB_CABLE_RENDER_DEVICE: &str = "CABLE Input / CABLE In 16 Ch";
const VB_CABLE_CAPTURE_DEVICE: &str = "CABLE Output";
const VB_CABLE_HARDWARE_ID: &str = "VBAudioVACWDM";
const VB_CABLE_VERIFY_RETRY_COUNT: u32 = 30;
const VB_CABLE_VERIFY_RETRY_DELAY: Duration = Duration::from_secs(1);
const UNINSTALL_KEY: &str = r"HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall\ClearLine";
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, PartialEq, Eq)]
struct InstallOptions {
    target_dir: PathBuf,
    start_on_login: bool,
}

#[cfg(windows)]
struct SetupLog {
    path: PathBuf,
    file: File,
}

#[cfg(windows)]
impl SetupLog {
    fn create() -> Result<Self> {
        let path = setup_log_path()?;
        let parent = path
            .parent()
            .ok_or_else(|| anyhow!("invalid setup log path: {}", path.display()))?;
        fs::create_dir_all(parent)?;
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let mut log = Self { path, file };
        log.line(format!("{APP_NAME} setup log started"));
        Ok(log)
    }

    fn line(&mut self, message: impl AsRef<str>) {
        let _ = writeln!(self.file, "{}", message.as_ref());
        let _ = self.file.flush();
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SetupCommand {
    Install {
        target_dir: Option<PathBuf>,
        quiet: bool,
        start_on_login: Option<bool>,
    },
    Uninstall {
        target_dir: Option<PathBuf>,
        quiet: bool,
        remove_vb_cable: Option<bool>,
    },
    CleanupInstallDir {
        target_dir: PathBuf,
        quiet: bool,
    },
    Help,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("ClearLine setup failed: {error:#}");
        #[cfg(windows)]
        show_message(
            "ClearLine 安装器",
            &format!("ClearLine 安装器失败：\n{error:#}"),
        );
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let current_exe = env::current_exe().unwrap_or_else(|_| PathBuf::from(SETUP_EXE_NAME));
    let command = parse_args_for_exe(&current_exe, env::args().skip(1))?;
    match command {
        SetupCommand::Install {
            target_dir,
            quiet,
            start_on_login,
        } => install(target_dir, quiet, start_on_login),
        SetupCommand::Uninstall {
            target_dir,
            quiet,
            remove_vb_cable,
        } => uninstall(target_dir, quiet, remove_vb_cable),
        SetupCommand::CleanupInstallDir { target_dir, quiet } => {
            cleanup_install_dir(target_dir, quiet)
        }
        SetupCommand::Help => {
            println!("{}", usage());
            Ok(())
        }
    }
}

fn parse_args_for_exe(exe: &Path, args: impl IntoIterator<Item = String>) -> Result<SetupCommand> {
    let mut mode = default_mode_for_exe(exe);
    let mut target_dir = None;
    let mut cleanup_dir = None;
    let mut quiet = false;
    let mut remove_vb_cable = None;
    let mut start_on_login = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--install" => mode = "install",
            "--uninstall" => mode = "uninstall",
            "--cleanup-install-dir" => {
                mode = "cleanup";
                cleanup_dir = Some(PathBuf::from(expect_value(
                    &mut args,
                    "--cleanup-install-dir",
                )?));
            }
            "--target" => target_dir = Some(PathBuf::from(expect_value(&mut args, "--target")?)),
            "--quiet" => quiet = true,
            "--start-on-login" => start_on_login = Some(true),
            "--no-start-on-login" => start_on_login = Some(false),
            "--remove-vb-cable" => remove_vb_cable = Some(true),
            "--keep-vb-cable" => remove_vb_cable = Some(false),
            "--help" | "-h" | "/?" => return Ok(SetupCommand::Help),
            unknown => bail!("unknown argument: {unknown}\n{}", usage()),
        }
    }

    match mode {
        "install" => Ok(SetupCommand::Install {
            target_dir,
            quiet,
            start_on_login,
        }),
        "uninstall" => Ok(SetupCommand::Uninstall {
            target_dir,
            quiet,
            remove_vb_cable,
        }),
        "cleanup" => Ok(SetupCommand::CleanupInstallDir {
            target_dir: cleanup_dir
                .ok_or_else(|| anyhow!("--cleanup-install-dir requires a directory"))?,
            quiet,
        }),
        _ => unreachable!(),
    }
}

fn expect_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String> {
    args.next()
        .filter(|value| !value.starts_with("--"))
        .ok_or_else(|| anyhow!("{flag} requires a value"))
}

fn usage() -> &'static str {
    "usage:\n  ClearLineSetup.exe [--install] [--target <dir>] [--quiet] [--start-on-login|--no-start-on-login]\n  ClearLineSetup.exe --uninstall [--target <dir>] [--quiet] [--remove-vb-cable|--keep-vb-cable]\n  ClearLineUninstall.exe\n\n双击 ClearLineSetup.exe 会显示安装界面、选择安装路径和开机自启动。"
}

fn default_mode_for_exe(exe: &Path) -> &'static str {
    exe.file_stem()
        .and_then(|name| name.to_str())
        .map(|name| name.to_ascii_lowercase().contains("uninstall"))
        .unwrap_or(false)
        .then_some("uninstall")
        .unwrap_or("install")
}

fn needs_interactive_install_dir(target_dir: Option<&Path>, quiet: bool) -> bool {
    !quiet && target_dir.is_none()
}

#[cfg(not(windows))]
fn install(
    _target_dir: Option<PathBuf>,
    _quiet: bool,
    _start_on_login: Option<bool>,
) -> Result<()> {
    bail!("ClearLineSetup.exe is only supported on Windows")
}

#[cfg(not(windows))]
fn uninstall(
    _target_dir: Option<PathBuf>,
    _quiet: bool,
    _remove_vb_cable: Option<bool>,
) -> Result<()> {
    bail!("ClearLineSetup.exe is only supported on Windows")
}

#[cfg(not(windows))]
fn cleanup_install_dir(_target_dir: PathBuf, _quiet: bool) -> Result<()> {
    bail!("ClearLineSetup.exe is only supported on Windows")
}

#[cfg(windows)]
fn install(target_dir: Option<PathBuf>, quiet: bool, start_on_login: Option<bool>) -> Result<()> {
    let Some(options) = resolve_install_options(target_dir, quiet, start_on_login)? else {
        return Ok(());
    };
    let mut args = elevation_args(&["--install"], Some(options.target_dir.as_path()), quiet);
    append_start_on_login_choice_args(&mut args, Some(options.start_on_login));
    ensure_elevated_args(args)?;
    let mut log = SetupLog::create()?;
    let result = install_with_log(
        Some(options.target_dir),
        quiet,
        options.start_on_login,
        &mut log,
    );
    if let Err(error) = result {
        log.line(format!("ERROR: {error:#}"));
        return Err(anyhow!("{error:#}\n日志文件: {}", log.path().display()));
    }
    Ok(())
}

#[cfg(windows)]
fn install_with_log(
    target_dir: Option<PathBuf>,
    quiet: bool,
    start_on_login: bool,
    log: &mut SetupLog,
) -> Result<()> {
    ensure_payload_available()?;

    let target_dir = target_dir.unwrap_or_else(default_install_dir);
    log.line(format!("Installing {APP_NAME} to {}", target_dir.display()));
    fs::create_dir_all(&target_dir)?;
    log.line("Writing embedded payload files");
    write_payload(&target_dir)?;
    log.line("Copying setup executable for uninstall");
    copy_self_for_uninstall(&target_dir)?;
    log.line("Creating Start Menu entries");
    create_start_menu_entries(&target_dir)?;
    log.line("Writing uninstall registry entries");
    write_uninstall_registry(&target_dir, log)?;
    let default_audio_snapshot = save_default_audio_before_vb_cable_install(&target_dir, log);
    ensure_vb_cable_available(&target_dir, quiet, log)?;
    restore_default_audio_after_vb_cable_install(
        &target_dir,
        default_audio_snapshot.as_deref(),
        log,
    );
    log.line(format!(
        "Configuring per-user startup: start_on_login={start_on_login}"
    ));
    set_installed_app_startup(&target_dir, start_on_login, log)?;
    log.line("Verifying installed ClearLine payload and VB-CABLE endpoints");
    run_helper_logged(
        &target_dir,
        &["verify-install", "--app", ".", "--require-vb-cable"],
        log,
    )?;

    log.line(format!(
        "Installed {APP_NAME} with VB-CABLE backend: render={VB_CABLE_RENDER_DEVICE}, capture={VB_CABLE_CAPTURE_DEVICE}."
    ));
    if !quiet {
        show_message(
            "ClearLine 安装完成",
            &format!(
                "ClearLine 已安装完成。\n\nClearLine uses VB-Audio VB-CABLE as the virtual audio device.\nVB-CABLE source: https://www.vb-cable.com / https://vb-audio.com/Cable/\nVB-CABLE is donationware and users may support/license it through VB-Audio.\n\nClearLine 会输出到 {VB_CABLE_RENDER_DEVICE}，请在 Discord、微信、QQ、浏览器会议或游戏语音中选择 {VB_CABLE_CAPTURE_DEVICE} 作为麦克风。\n\n日志文件：{}",
                log.path().display()
            ),
        );
    }
    Ok(())
}

#[cfg(windows)]
fn resolve_install_options(
    target_dir: Option<PathBuf>,
    quiet: bool,
    start_on_login: Option<bool>,
) -> Result<Option<InstallOptions>> {
    if needs_interactive_install_dir(target_dir.as_deref(), quiet)
        || (!quiet && start_on_login.is_none())
    {
        let default_dir = default_install_dir();
        let wizard_dir = target_dir.as_deref().unwrap_or(default_dir.as_path());
        return show_msi_style_install_wizard(wizard_dir, start_on_login.unwrap_or(false));
    }

    Ok(Some(InstallOptions {
        target_dir: target_dir.unwrap_or_else(default_install_dir),
        start_on_login: start_on_login.unwrap_or(false),
    }))
}

#[cfg(windows)]
fn uninstall(
    target_dir: Option<PathBuf>,
    quiet: bool,
    remove_vb_cable: Option<bool>,
) -> Result<()> {
    let mut args = elevation_args(&["--uninstall"], target_dir.as_deref(), quiet);
    append_uninstall_choice_args(&mut args, remove_vb_cable);
    ensure_elevated_args(args)?;
    let mut log = SetupLog::create()?;
    let result = uninstall_with_log(target_dir, quiet, remove_vb_cable, &mut log);
    if let Err(error) = result {
        log.line(format!("ERROR: {error:#}"));
        return Err(anyhow!("{error:#}\n日志文件: {}", log.path().display()));
    }
    Ok(())
}

#[cfg(windows)]
fn uninstall_with_log(
    target_dir: Option<PathBuf>,
    quiet: bool,
    remove_vb_cable: Option<bool>,
    log: &mut SetupLog,
) -> Result<()> {
    let target_dir = target_dir.unwrap_or_else(default_install_dir);
    log.line(format!(
        "Uninstalling {APP_NAME} from {}",
        target_dir.display()
    ));

    let remove_vb_cable = resolve_remove_vb_cable_choice(remove_vb_cable, quiet, log)?;
    if remove_vb_cable {
        log.line("User selected VB-CABLE removal during uninstall.");
        let result = run_helper_logged(&target_dir, &["uninstall-vbcable"], log);
        if let Err(error) = result {
            log.line(format!("WARNING: VB-CABLE uninstall failed: {error:#}"));
        }
    } else {
        log.line("Leaving VB-CABLE installed because it is a shared virtual audio component.");
    }
    let _ = remove_start_menu_entries();
    let _ = delete_uninstall_registry(log);
    schedule_or_remove_install_dir(&target_dir, log)?;

    log.line(format!("Uninstalled {APP_NAME}."));
    if !quiet {
        show_message(
            "ClearLine 卸载完成",
            &format!(
                "ClearLine 已卸载。\n\nVB-CABLE 状态：{}\n\n若安装目录仍短暂存在，系统会在卸载进程退出后自动清理。\n\n日志文件：{}",
                if remove_vb_cable { "已请求卸载" } else { "已保留" },
                log.path().display()
            ),
        );
    }
    Ok(())
}

#[cfg(windows)]
fn ensure_elevated_args(args: Vec<String>) -> Result<()> {
    if is_user_admin() {
        return Ok(());
    }

    let current_exe = env::current_exe()?;
    let exit_code = elevate_with_runas(&current_exe, &args)?;
    std::process::exit(exit_code as i32);
}

#[cfg(windows)]
fn elevation_args(base_args: &[&str], target_dir: Option<&Path>, quiet: bool) -> Vec<String> {
    let mut args = base_args
        .iter()
        .map(|arg| (*arg).to_string())
        .collect::<Vec<_>>();
    if let Some(target_dir) = target_dir {
        args.push("--target".to_string());
        args.push(target_dir.display().to_string());
    }
    if quiet {
        args.push("--quiet".to_string());
    }
    args
}

#[cfg(windows)]
fn append_uninstall_choice_args(args: &mut Vec<String>, remove_vb_cable: Option<bool>) {
    match remove_vb_cable {
        Some(true) => args.push("--remove-vb-cable".to_owned()),
        Some(false) => args.push("--keep-vb-cable".to_owned()),
        None => {}
    }
}

#[cfg(windows)]
fn append_start_on_login_choice_args(args: &mut Vec<String>, start_on_login: Option<bool>) {
    match start_on_login {
        Some(true) => args.push("--start-on-login".to_owned()),
        Some(false) => args.push("--no-start-on-login".to_owned()),
        None => {}
    }
}

#[cfg(windows)]
#[allow(dead_code)]
fn resolve_start_on_login_choice(start_on_login: Option<bool>, quiet: bool) -> Result<bool> {
    if let Some(start_on_login) = start_on_login {
        return Ok(start_on_login);
    }
    if quiet {
        return Ok(false);
    }
    select_start_on_login_interactively()
}

#[cfg(windows)]
#[allow(dead_code)]
fn select_start_on_login_interactively() -> Result<bool> {
    show_msi_style_install_wizard(&default_install_dir(), false)?
        .map(|options| options.start_on_login)
        .ok_or_else(|| anyhow!("用户取消安装"))
}

#[cfg(windows)]
fn ensure_payload_available() -> Result<()> {
    if payload::PAYLOAD_FILE_COUNT == 0 {
        bail!("ClearLineSetup.exe does not contain embedded payload files")
    }
    println!(
        "Embedded payload: {} files, {} bytes",
        payload::PAYLOAD_FILE_COUNT,
        payload::PAYLOAD_TOTAL_BYTES
    );
    Ok(())
}

#[cfg(windows)]
fn write_payload(target_dir: &Path) -> Result<()> {
    for file in payload::PAYLOAD_FILES {
        let destination = join_payload_path(target_dir, file.relative_path)?;
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&destination, file.bytes)
            .map_err(|err| anyhow!("write {}: {err}", destination.display()))?;
    }
    Ok(())
}

#[cfg(windows)]
fn join_payload_path(root: &Path, relative_path: &str) -> Result<PathBuf> {
    let mut path = root.to_path_buf();
    for component in relative_path.split('/') {
        if component.is_empty() || component == "." || component == ".." || component.contains('\\')
        {
            bail!("invalid embedded payload path: {relative_path}");
        }
        path.push(component);
    }
    Ok(path)
}

#[cfg(windows)]
fn copy_self_for_uninstall(target_dir: &Path) -> Result<()> {
    let setup_dir = target_dir.join("installer");
    fs::create_dir_all(&setup_dir)?;
    let current_exe = env::current_exe()?;
    fs::copy(&current_exe, setup_dir.join(SETUP_EXE_NAME))?;
    fs::copy(current_exe, setup_dir.join(UNINSTALL_EXE_NAME))?;
    Ok(())
}

#[cfg(windows)]
fn save_default_audio_before_vb_cable_install(
    target_dir: &Path,
    log: &mut SetupLog,
) -> Option<PathBuf> {
    let snapshot = target_dir
        .join("installer")
        .join("default-audio-before-vbcable.txt");
    let snapshot_arg = snapshot.display().to_string();
    log.line(format!(
        "Saving default audio device snapshot before VB-CABLE install: {}",
        snapshot.display()
    ));
    match run_helper_logged(
        target_dir,
        &["save-default-audio", "--path", snapshot_arg.as_str()],
        log,
    ) {
        Ok(()) => Some(snapshot),
        Err(error) => {
            log.line(format!(
                "WARNING: Could not save default audio device snapshot: {error:#}"
            ));
            None
        }
    }
}

#[cfg(windows)]
fn restore_default_audio_after_vb_cable_install(
    target_dir: &Path,
    snapshot: Option<&Path>,
    log: &mut SetupLog,
) {
    let Some(snapshot) = snapshot else {
        log.line("Skipping default audio restore because no snapshot was saved.");
        return;
    };
    let snapshot_arg = snapshot.display().to_string();
    log.line(format!(
        "Restoring default audio devices after VB-CABLE install: {}",
        snapshot.display()
    ));
    if let Err(error) = run_helper_logged(
        target_dir,
        &["restore-default-audio", "--path", snapshot_arg.as_str()],
        log,
    ) {
        log.line(format!(
            "WARNING: Could not restore default audio devices after VB-CABLE install: {error:#}"
        ));
    }
}

#[cfg(windows)]
fn ensure_vb_cable_available(target_dir: &Path, _quiet: bool, log: &mut SetupLog) -> Result<()> {
    log_vb_cable_notice(log);
    log.line(format!(
        "VB-CABLE root-enumerated hardware id: {VB_CABLE_HARDWARE_ID}"
    ));
    log.line("Checking VB-CABLE endpoints before installation attempt");
    match run_helper_logged(target_dir, &["verify-vb-cable"], log) {
        Ok(()) => {
            log.line("VB-CABLE already detected; skipping VB-CABLE devnode creation.");
            return Ok(());
        }
        Err(error) => {
            log.line(format!("VB-CABLE not detected before setup: {error:#}"));
        }
    }

    let package_dir = extract_vb_cable_package(target_dir, log)?;
    let package_arg = package_dir.display().to_string();
    run_helper_logged(
        target_dir,
        &["install-vbcable", "--package", package_arg.as_str()],
        log,
    )?;

    wait_for_vb_cable_endpoints(target_dir, log)
}

#[cfg(windows)]
fn wait_for_vb_cable_endpoints(target_dir: &Path, log: &mut SetupLog) -> Result<()> {
    log.line(format!(
        "Waiting for VB-CABLE endpoints after driver installation, up to {VB_CABLE_VERIFY_RETRY_COUNT}s"
    ));

    let mut last_error = None;
    for attempt in 1..=VB_CABLE_VERIFY_RETRY_COUNT {
        log.line(format!(
            "Checking VB-CABLE endpoints after standard driver installation, attempt {attempt}/{VB_CABLE_VERIFY_RETRY_COUNT}"
        ));
        match run_helper_logged(target_dir, &["verify-vb-cable"], log) {
            Ok(()) => {
                log.line(format!(
                    "VB-CABLE endpoints detected on attempt {attempt}/{VB_CABLE_VERIFY_RETRY_COUNT}."
                ));
                return Ok(());
            }
            Err(error) => {
                log.line(format!(
                    "VB-CABLE endpoints not ready on attempt {attempt}/{VB_CABLE_VERIFY_RETRY_COUNT}: {error:#}"
                ));
                last_error = Some(error);
                if attempt < VB_CABLE_VERIFY_RETRY_COUNT {
                    std::thread::sleep(VB_CABLE_VERIFY_RETRY_DELAY);
                }
            }
        }
    }

    let error = last_error
        .map(|error| format!("{error:#}"))
        .unwrap_or_else(|| "no verification attempt was run".to_owned());
    Err(anyhow!(
        "VB-CABLE endpoints were still not detected after driver installation.\n\
         Expected render device '{VB_CABLE_RENDER_DEVICE}' and recording device '{VB_CABLE_CAPTURE_DEVICE}'.\n\
         Please reboot if Windows requested it, then run ClearLineSetup.exe again.\n\
         {error}"
    ))
}

#[cfg(windows)]
fn resolve_remove_vb_cable_choice(
    remove_vb_cable: Option<bool>,
    quiet: bool,
    log: &mut SetupLog,
) -> Result<bool> {
    if let Some(remove_vb_cable) = remove_vb_cable {
        return Ok(remove_vb_cable);
    }
    if quiet {
        log.line("Quiet uninstall did not specify VB-CABLE removal; keeping VB-CABLE.");
        return Ok(false);
    }
    ask_remove_vb_cable_on_uninstall()
}

#[cfg(windows)]
fn ask_remove_vb_cable_on_uninstall() -> Result<bool> {
    match show_yes_no_cancel(
        "ClearLine 卸载器",
        "是否同时卸载 VB-CABLE 虚拟音频驱动？\n\n选择“是”：卸载 ClearLine 和 VB-CABLE。\n选择“否”：只卸载 ClearLine，保留 VB-CABLE。\n选择“取消”：取消卸载。",
    ) {
        Some(value) => Ok(value),
        None => bail!("用户取消卸载"),
    }
}

#[cfg(windows)]
fn log_vb_cable_notice(log: &mut SetupLog) {
    log.line("ClearLine uses VB-Audio VB-CABLE as the virtual audio device.");
    log.line("VB-CABLE source: https://www.vb-cable.com / https://vb-audio.com/Cable/");
    log.line("VB-CABLE is donationware and users may support/license it through VB-Audio.");
}

#[cfg(windows)]
fn extract_vb_cable_package(target_dir: &Path, log: &mut SetupLog) -> Result<PathBuf> {
    let payload_dir = target_dir.join(VB_CABLE_PAYLOAD_DIR.replace('/', "\\"));
    let zip_path = payload_dir.join(VB_CABLE_ZIP_NAME);
    let extract_dir = payload_dir.join(VB_CABLE_EXTRACTED_DIR_NAME);
    log.line(format!(
        "Extracting official VB-CABLE package: {} -> {}",
        zip_path.display(),
        extract_dir.display()
    ));
    extract_zip_to_dir(&zip_path, &extract_dir)?;
    validate_extracted_vb_cable_package(&extract_dir)?;
    Ok(extract_dir)
}

#[cfg(windows)]
fn extract_zip_to_dir(zip_path: &Path, destination: &Path) -> Result<()> {
    if !zip_path.is_file() {
        bail!("official VB-CABLE zip missing: {}", zip_path.display());
    }
    if destination.exists() {
        fs::remove_dir_all(destination)?;
    }
    fs::create_dir_all(destination)?;

    let file = File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let Some(enclosed) = entry.enclosed_name() else {
            bail!("VB-CABLE zip contains unsafe path: {}", entry.name());
        };
        let output = destination.join(enclosed);
        if entry.is_dir() {
            fs::create_dir_all(&output)?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        fs::write(output, bytes)?;
    }
    Ok(())
}

#[cfg(windows)]
fn validate_extracted_vb_cable_package(package_dir: &Path) -> Result<()> {
    for name in [
        "readme.txt",
        "vbMmeCable64_win10.inf",
        "vbaudio_cable64_win10.sys",
        "vbaudio_cable64_win10.cat",
    ] {
        let path = package_dir.join(name);
        if !path.is_file() {
            bail!("extracted VB-CABLE package missing {}", path.display());
        }
    }
    Ok(())
}

#[cfg(windows)]
fn run_helper_logged(target_dir: &Path, args: &[&str], log: &mut SetupLog) -> Result<()> {
    let helper = target_dir.join(HELPER_RELATIVE_PATH.replace('/', "\\"));
    if !helper.is_file() {
        bail!("installer helper missing: {}", helper.display());
    }

    let mut command = Command::new(&helper);
    command.current_dir(target_dir);
    let mut expanded_args = Vec::new();
    for arg in args {
        let expanded = match *arg {
            "driver/package" => target_dir
                .join("driver")
                .join("package")
                .display()
                .to_string(),
            "." => target_dir.display().to_string(),
            other => other.to_string(),
        };
        command.arg(&expanded);
        expanded_args.push(expanded);
    }
    log.line(format!(
        "Running helper: {} {}",
        helper.display(),
        expanded_args.join(" ")
    ));
    command.creation_flags(CREATE_NO_WINDOW);
    let output = command.output()?;
    log.line(format!("helper exit status: {}", output.status));
    if !output.stdout.is_empty() {
        log.line("helper stdout:");
        log.line(String::from_utf8_lossy(&output.stdout));
    }
    if !output.stderr.is_empty() {
        log.line("helper stderr:");
        log.line(String::from_utf8_lossy(&output.stderr));
    }
    if !output.status.success() {
        bail!("{} failed with status {}", helper.display(), output.status);
    }
    Ok(())
}

#[cfg(windows)]
fn write_uninstall_registry(target_dir: &Path, log: &mut SetupLog) -> Result<()> {
    let (uninstall_string, quiet_uninstall_string) = uninstall_registry_strings(target_dir);
    let estimated_size_kb = (payload::PAYLOAD_TOTAL_BYTES / 1024).max(1).to_string();

    reg_add("DisplayName", "REG_SZ", APP_NAME, log)?;
    reg_add("DisplayVersion", "REG_SZ", VERSION, log)?;
    reg_add("Publisher", "REG_SZ", "ClearLine", log)?;
    reg_add(
        "InstallLocation",
        "REG_SZ",
        &target_dir.display().to_string(),
        log,
    )?;
    reg_add(
        "DisplayIcon",
        "REG_SZ",
        &target_dir.join("ClearLine.exe").display().to_string(),
        log,
    )?;
    reg_add("UninstallString", "REG_SZ", &uninstall_string, log)?;
    reg_add(
        "QuietUninstallString",
        "REG_SZ",
        &quiet_uninstall_string,
        log,
    )?;
    reg_add("EstimatedSize", "REG_DWORD", &estimated_size_kb, log)?;
    reg_add("NoModify", "REG_DWORD", "1", log)?;
    reg_add("NoRepair", "REG_DWORD", "1", log)?;
    Ok(())
}

fn uninstall_registry_strings(target_dir: &Path) -> (String, String) {
    let uninstall_exe = target_dir.join("installer").join(UNINSTALL_EXE_NAME);
    (
        format!("\"{}\"", uninstall_exe.display()),
        format!("\"{}\" --quiet", uninstall_exe.display()),
    )
}

#[cfg(windows)]
fn set_installed_app_startup(target_dir: &Path, enabled: bool, log: &mut SetupLog) -> Result<()> {
    let app_command = format!(
        "\"{}\" --minimized",
        target_dir.join("ClearLine.exe").display()
    );
    if enabled {
        log.line(format!(
            "Enabling ClearLine startup for current user: {app_command}"
        ));
        return run_checked(
            "reg.exe",
            &[
                "add",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                "ClearLine",
                "/t",
                "REG_SZ",
                "/d",
                &app_command,
                "/f",
            ],
            log,
        );
    }

    log.line("Disabling ClearLine startup for current user if present.");
    let mut command = Command::new("reg.exe");
    command.args([
        "delete",
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
        "/v",
        "ClearLine",
        "/f",
    ]);
    command.creation_flags(CREATE_NO_WINDOW);
    let output = command.output()?;
    log.line(format!("startup delete exit status: {}", output.status));
    if !output.stdout.is_empty() {
        log.line("startup delete stdout:");
        log.line(String::from_utf8_lossy(&output.stdout));
    }
    if !output.stderr.is_empty() {
        log.line("startup delete stderr:");
        log.line(String::from_utf8_lossy(&output.stderr));
    }
    Ok(())
}

#[cfg(windows)]
fn reg_add(name: &str, value_type: &str, value: &str, log: &mut SetupLog) -> Result<()> {
    run_checked(
        "reg.exe",
        &[
            "add",
            UNINSTALL_KEY,
            "/v",
            name,
            "/t",
            value_type,
            "/d",
            value,
            "/f",
        ],
        log,
    )
}

#[cfg(windows)]
fn delete_uninstall_registry(log: &mut SetupLog) -> Result<()> {
    run_checked("reg.exe", &["delete", UNINSTALL_KEY, "/f"], log)
}

#[cfg(windows)]
fn create_start_menu_entries(target_dir: &Path) -> Result<()> {
    let start_menu_dir =
        PathBuf::from(env::var_os("ProgramData").ok_or_else(|| anyhow!("ProgramData is not set"))?)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("ClearLine");
    fs::create_dir_all(&start_menu_dir)?;
    let app = target_dir.join("ClearLine.exe");
    create_url_shortcut(&start_menu_dir.join("ClearLine.url"), &app, &app)?;
    let uninstaller = target_dir.join("installer").join(UNINSTALL_EXE_NAME);
    create_url_shortcut(
        &start_menu_dir.join("卸载 ClearLine.url"),
        &uninstaller,
        &uninstaller,
    )?;
    Ok(())
}

#[cfg(windows)]
fn create_url_shortcut(shortcut: &Path, target: &Path, icon: &Path) -> Result<()> {
    let url = format!(
        "[InternetShortcut]\r\nURL=file:///{}\r\nIconFile={}\r\nIconIndex=0\r\n",
        target
            .display()
            .to_string()
            .replace('\\', "/")
            .replace(' ', "%20"),
        icon.display()
    );
    fs::write(shortcut, url)?;
    Ok(())
}

#[cfg(windows)]
fn remove_start_menu_entries() -> Result<()> {
    let start_menu_dir =
        PathBuf::from(env::var_os("ProgramData").ok_or_else(|| anyhow!("ProgramData is not set"))?)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("ClearLine");
    if start_menu_dir.exists() {
        fs::remove_dir_all(start_menu_dir)?;
    }
    Ok(())
}

#[cfg(windows)]
fn schedule_or_remove_install_dir(target_dir: &Path, log: &mut SetupLog) -> Result<()> {
    let current_exe = env::current_exe().unwrap_or_default();
    let running_from_target = current_exe.starts_with(target_dir);
    if running_from_target {
        spawn_cleanup_copy(target_dir, log)?;
        return Ok(());
    }

    remove_install_dir_now(target_dir, log)
}

#[cfg(windows)]
fn spawn_cleanup_copy(target_dir: &Path, log: &mut SetupLog) -> Result<()> {
    let cleanup_exe = cleanup_copy_path()?;
    if let Some(parent) = cleanup_exe.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(env::current_exe()?, &cleanup_exe)?;
    log.line(format!("Copied cleanup setup to {}", cleanup_exe.display()));

    let mut cleanup = Command::new(&cleanup_exe);
    cleanup.current_dir(env::temp_dir());
    cleanup.arg("--cleanup-install-dir");
    cleanup.arg(target_dir);
    cleanup.arg("--quiet");
    cleanup.creation_flags(CREATE_NO_WINDOW);
    cleanup.spawn()?;
    log.line(format!(
        "Spawned cleanup setup from {}",
        cleanup_exe.display()
    ));
    Ok(())
}

#[cfg(windows)]
fn cleanup_copy_path() -> Result<PathBuf> {
    let program_data =
        env::var_os("ProgramData").ok_or_else(|| anyhow!("ProgramData is not set"))?;
    Ok(PathBuf::from(program_data)
        .join("ClearLine")
        .join("cleanup")
        .join("ClearLineSetup-cleanup.exe"))
}

#[cfg(windows)]
fn cleanup_install_dir(target_dir: PathBuf, quiet: bool) -> Result<()> {
    let mut elevation_args = vec![
        "--cleanup-install-dir".to_string(),
        target_dir.display().to_string(),
    ];
    if quiet {
        elevation_args.push("--quiet".to_string());
    }
    ensure_elevated_args(elevation_args)?;
    let mut log = SetupLog::create()?;
    let result = cleanup_install_dir_with_retry(&target_dir, &mut log);
    if let Err(error) = result {
        log.line(format!("ERROR: {error:#}"));
        return Err(anyhow!("{error:#}\n日志文件: {}", log.path().display()));
    }
    schedule_cleanup_self_delete(&mut log)?;
    Ok(())
}

#[cfg(windows)]
fn cleanup_install_dir_with_retry(target_dir: &Path, log: &mut SetupLog) -> Result<()> {
    const CLEANUP_ATTEMPTS: usize = 30;
    log.line(format!(
        "Cleaning install directory with retry: {}",
        target_dir.display()
    ));
    for attempt in 1..=CLEANUP_ATTEMPTS {
        log.line(format!("Cleanup attempt {attempt}/{CLEANUP_ATTEMPTS}"));
        if !target_dir.exists() {
            log.line("Install directory is already absent");
            return Ok(());
        }
        match fs::remove_dir_all(target_dir) {
            Ok(()) => {
                log.line("Install directory removed");
                return Ok(());
            }
            Err(error) => {
                log.line(format!(
                    "Install directory removal failed on attempt {attempt}: {error}"
                ));
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }
    bail!(
        "failed to remove install directory after {CLEANUP_ATTEMPTS} attempts: {}",
        target_dir.display()
    )
}

#[cfg(windows)]
fn schedule_cleanup_self_delete(log: &mut SetupLog) -> Result<()> {
    let current_exe = env::current_exe()?;
    let command = format!(
        "ping -n 2 127.0.0.1 >nul & del /f /q \"{}\"",
        current_exe.display()
    );
    log.line(format!("cleanup self-delete command: {command}"));
    let mut cleanup = Command::new("cmd.exe");
    cleanup.current_dir(env::temp_dir());
    cleanup.args(["/C", &command]);
    cleanup.creation_flags(CREATE_NO_WINDOW);
    cleanup.spawn()?;
    Ok(())
}

#[cfg(windows)]
fn remove_install_dir_now(target_dir: &Path, log: &mut SetupLog) -> Result<()> {
    if target_dir.exists() {
        log.line(format!(
            "Removing install directory now: {}",
            target_dir.display()
        ));
        fs::remove_dir_all(target_dir)?;
    }
    log.line(format!(
        "Install directory removed or already absent: {}",
        target_dir.display()
    ));
    Ok(())
}

#[cfg(windows)]
fn run_checked(program: &str, args: &[&str], log: &mut SetupLog) -> Result<()> {
    log.line(format!("Running command: {program} {}", args.join(" ")));
    let mut command = Command::new(program);
    command.args(args);
    command.creation_flags(CREATE_NO_WINDOW);
    let output = command.output()?;
    log.line(format!("command exit status: {}", output.status));
    if !output.stdout.is_empty() {
        log.line("command stdout:");
        log.line(String::from_utf8_lossy(&output.stdout));
    }
    if !output.stderr.is_empty() {
        log.line("command stderr:");
        log.line(String::from_utf8_lossy(&output.stderr));
    }
    if output.status.success() {
        return Ok(());
    }
    bail!(
        "{program} failed with status {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[cfg(windows)]
#[allow(dead_code)]
fn select_install_dir_interactively(default_dir: &Path) -> Result<Option<PathBuf>> {
    show_msi_style_install_wizard(default_dir, false)
        .map(|options| options.map(|options| options.target_dir))
}

#[cfg(windows)]
const WIZARD_ID_PATH_EDIT: i32 = 2001;
#[cfg(windows)]
const WIZARD_ID_BROWSE: i32 = 2002;
#[cfg(windows)]
const WIZARD_ID_STARTUP: i32 = 2003;
#[cfg(windows)]
const WIZARD_ID_INSTALL: i32 = 2004;
#[cfg(windows)]
const WIZARD_ID_CANCEL: i32 = 2005;

#[cfg(windows)]
struct InstallWizardState {
    result: Option<InstallOptions>,
    default_start_on_login: bool,
}

#[cfg(windows)]
fn show_msi_style_install_wizard(
    default_dir: &Path,
    default_start_on_login: bool,
) -> Result<Option<InstallOptions>> {
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{COLOR_WINDOW, HBRUSH};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, LoadCursorW,
        RegisterClassW, TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, GWLP_USERDATA,
        IDC_ARROW, MSG, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_NCCREATE, WNDCLASSW, WS_CAPTION,
        WS_SYSMENU, WS_VISIBLE,
    };

    unsafe extern "system" fn wizard_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_NCCREATE => {
                let createstruct =
                    lparam as *const windows_sys::Win32::UI::WindowsAndMessaging::CREATESTRUCTW;
                let state = (*createstruct).lpCreateParams as *mut InstallWizardState;
                windows_sys::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(
                    hwnd,
                    GWLP_USERDATA,
                    state as isize,
                );
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            windows_sys::Win32::UI::WindowsAndMessaging::WM_CREATE => {
                if let Some(state) = wizard_state(hwnd) {
                    create_msi_style_wizard_controls(hwnd, state.default_start_on_login);
                }
                0
            }
            WM_COMMAND => {
                let control_id = (wparam & 0xffff) as i32;
                match control_id {
                    WIZARD_ID_BROWSE => {
                        let current_dir = get_dlg_item_text(hwnd, WIZARD_ID_PATH_EDIT);
                        let current_dir = PathBuf::from(current_dir);
                        if let Ok(Some(path)) = browse_install_dir(&current_dir) {
                            set_dlg_item_text(
                                hwnd,
                                WIZARD_ID_PATH_EDIT,
                                &path.display().to_string(),
                            );
                        }
                        0
                    }
                    WIZARD_ID_INSTALL => {
                        if let Some(state) = wizard_state(hwnd) {
                            let target_dir =
                                PathBuf::from(get_dlg_item_text(hwnd, WIZARD_ID_PATH_EDIT));
                            let start_on_login = checkbox_checked(hwnd, WIZARD_ID_STARTUP);
                            state.result = Some(InstallOptions {
                                target_dir,
                                start_on_login,
                            });
                        }
                        windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);
                        0
                    }
                    WIZARD_ID_CANCEL => {
                        windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);
                        0
                    }
                    _ => DefWindowProcW(hwnd, message, wparam, lparam),
                }
            }
            WM_CLOSE => {
                windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);
                0
            }
            WM_DESTROY => {
                windows_sys::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    let class_name = to_wide("ClearLineMsiStyleInstallWizard");
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wizard_proc),
        hInstance: instance,
        hCursor: unsafe { LoadCursorW(std::ptr::null_mut(), IDC_ARROW) },
        hbrBackground: (COLOR_WINDOW as isize + 1) as HBRUSH,
        lpszClassName: class_name.as_ptr(),
        ..Default::default()
    };
    unsafe {
        RegisterClassW(&class);
    }

    let state = Box::new(InstallWizardState {
        result: None,
        default_start_on_login,
    });
    let state_ptr = Box::into_raw(state);
    let title = to_wide("ClearLine 安装向导");
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            680,
            430,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            state_ptr.cast(),
        )
    };
    if hwnd.is_null() {
        unsafe {
            drop(Box::from_raw(state_ptr));
        }
        bail!("CreateWindowExW failed while showing ClearLine installer wizard");
    }

    set_dlg_item_text(
        hwnd,
        WIZARD_ID_PATH_EDIT,
        &default_dir.display().to_string(),
    );

    unsafe {
        let mut message = MSG::default();
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        let state = Box::from_raw(state_ptr);
        Ok(state.result)
    }
}

#[cfg(windows)]
unsafe fn wizard_state(
    hwnd: windows_sys::Win32::Foundation::HWND,
) -> Option<&'static mut InstallWizardState> {
    let state = windows_sys::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
        hwnd,
        windows_sys::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
    ) as *mut InstallWizardState;
    state.as_mut()
}

#[cfg(windows)]
fn create_msi_style_wizard_controls(
    hwnd: windows_sys::Win32::Foundation::HWND,
    default_start_on_login: bool,
) {
    use windows_sys::Win32::Foundation::{HWND, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{GetStockObject, DEFAULT_GUI_FONT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, SendMessageW, BM_SETCHECK, BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON,
        ES_AUTOHSCROLL, WM_SETFONT, WS_BORDER, WS_CHILD, WS_TABSTOP, WS_VISIBLE,
    };

    unsafe fn add_control(
        hwnd: HWND,
        class_name: &str,
        text: &str,
        style: u32,
        id: i32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> HWND {
        let class_name = to_wide(class_name);
        let text = to_wide(text);
        let control = CreateWindowExW(
            0,
            class_name.as_ptr(),
            text.as_ptr(),
            style,
            x,
            y,
            width,
            height,
            hwnd,
            id as usize as _,
            std::ptr::null_mut(),
            std::ptr::null(),
        );
        let font = GetStockObject(DEFAULT_GUI_FONT) as WPARAM;
        SendMessageW(control, WM_SETFONT, font, 1);
        control
    }

    let static_style = WS_CHILD | WS_VISIBLE;
    let button_style = WS_CHILD | WS_VISIBLE | WS_TABSTOP;

    unsafe {
        add_control(
            hwnd,
            "STATIC",
            "ClearLine 安装向导",
            static_style,
            0,
            24,
            18,
            600,
            28,
        );
        add_control(
            hwnd,
            "STATIC",
            "此向导将安装 ClearLine、DeepFilterNet 模型和 VB-CABLE 虚拟音频组件。",
            static_style,
            0,
            24,
            52,
            610,
            28,
        );
        add_control(
            hwnd,
            "STATIC",
            "目标文件夹",
            static_style,
            0,
            42,
            112,
            200,
            22,
        );
        add_control(
            hwnd,
            "EDIT",
            "",
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
            WIZARD_ID_PATH_EDIT,
            42,
            138,
            470,
            26,
        );
        add_control(
            hwnd,
            "BUTTON",
            "浏览...",
            button_style,
            WIZARD_ID_BROWSE,
            524,
            137,
            92,
            28,
        );
        let checkbox = add_control(
            hwnd,
            "BUTTON",
            "开机自启动（登录后最小化到系统托盘）",
            button_style | BS_AUTOCHECKBOX as u32,
            WIZARD_ID_STARTUP,
            42,
            188,
            420,
            26,
        );
        if default_start_on_login {
            SendMessageW(checkbox, BM_SETCHECK, 1, 0);
        }
        add_control(
            hwnd,
            "STATIC",
            "单击“安装”开始安装。安装驱动时 Windows 会显示 UAC 确认。",
            static_style,
            0,
            42,
            238,
            560,
            24,
        );
        add_control(
            hwnd,
            "BUTTON",
            "安装",
            button_style | BS_DEFPUSHBUTTON as u32,
            WIZARD_ID_INSTALL,
            420,
            338,
            92,
            30,
        );
        add_control(
            hwnd,
            "BUTTON",
            "取消",
            button_style,
            WIZARD_ID_CANCEL,
            524,
            338,
            92,
            30,
        );
    }
}

#[cfg(windows)]
fn get_dlg_item_text(hwnd: windows_sys::Win32::Foundation::HWND, id: i32) -> String {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetDlgItemTextW;

    let mut buffer = [0u16; 1024];
    let len = unsafe { GetDlgItemTextW(hwnd, id, buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..len as usize])
}

#[cfg(windows)]
fn set_dlg_item_text(hwnd: windows_sys::Win32::Foundation::HWND, id: i32, text: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetDlgItem, SetWindowTextW};

    let control = unsafe { GetDlgItem(hwnd, id) };
    if !control.is_null() {
        let text = to_wide(text);
        unsafe {
            SetWindowTextW(control, text.as_ptr());
        }
    }
}

#[cfg(windows)]
fn checkbox_checked(hwnd: windows_sys::Win32::Foundation::HWND, id: i32) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetDlgItem, SendMessageW, BM_GETCHECK};

    let control = unsafe { GetDlgItem(hwnd, id) };
    !control.is_null() && unsafe { SendMessageW(control, BM_GETCHECK, 0, 0) } == 1
}

#[cfg(windows)]
fn browse_install_dir(default_dir: &Path) -> Result<Option<PathBuf>> {
    use windows_sys::Win32::Foundation::{RPC_E_CHANGED_MODE, S_FALSE, S_OK};
    use windows_sys::Win32::System::Com::{
        CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED,
    };
    use windows_sys::Win32::UI::Shell::{
        SHBrowseForFolderW, SHGetPathFromIDListW, BIF_EDITBOX, BIF_NEWDIALOGSTYLE,
        BIF_RETURNONLYFSDIRS, BROWSEINFOW,
    };

    let hr = unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
    let should_uninitialize = hr == S_OK || hr == S_FALSE;
    if hr != S_OK && hr != S_FALSE && hr != RPC_E_CHANGED_MODE {
        bail!("CoInitializeEx failed: 0x{:08x}", hr as u32);
    }

    let title = to_wide(&format!(
        "选择安装路径。默认路径：{}",
        default_dir.display()
    ));
    let mut display_name = [0u16; 260];
    let browse_info = BROWSEINFOW {
        hwndOwner: std::ptr::null_mut(),
        pidlRoot: std::ptr::null_mut(),
        pszDisplayName: display_name.as_mut_ptr(),
        lpszTitle: title.as_ptr(),
        ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE | BIF_EDITBOX,
        lpfn: None,
        lParam: 0,
        iImage: 0,
    };

    let selected = unsafe { SHBrowseForFolderW(&browse_info) };
    if selected.is_null() {
        if should_uninitialize {
            unsafe {
                CoUninitialize();
            }
        }
        return Ok(None);
    }

    let mut path = [0u16; 260];
    let got_path = unsafe { SHGetPathFromIDListW(selected, path.as_mut_ptr()) != 0 };
    unsafe {
        CoTaskMemFree(selected.cast());
        if should_uninitialize {
            CoUninitialize();
        }
    }
    if !got_path {
        bail!("SHGetPathFromIDListW failed while selecting install directory");
    }
    Ok(Some(PathBuf::from(wide_buffer_to_string(&path))))
}

#[cfg(windows)]
fn wide_buffer_to_string(buffer: &[u16]) -> String {
    let len = buffer
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..len])
}

#[cfg(windows)]
fn default_install_dir() -> PathBuf {
    PathBuf::from(env::var_os("ProgramFiles").unwrap_or_else(|| "C:\\Program Files".into()))
        .join("ClearLine")
}

#[cfg(windows)]
fn setup_log_path() -> Result<PathBuf> {
    let program_data =
        env::var_os("ProgramData").ok_or_else(|| anyhow!("ProgramData is not set"))?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| anyhow!("system time before unix epoch: {err}"))?
        .as_secs();
    Ok(PathBuf::from(program_data)
        .join("ClearLine")
        .join("logs")
        .join(format!("ClearLineSetup-{timestamp}.log")))
}

#[cfg(windows)]
fn is_user_admin() -> bool {
    unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() != 0 }
}

#[cfg(windows)]
fn elevate_with_runas(exe: &Path, args: &[String]) -> Result<u32> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, WAIT_FAILED, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, WaitForSingleObject, INFINITE,
    };
    use windows_sys::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let operation = to_wide("runas");
    let file = to_wide_path(exe);
    let params = to_wide(&join_command_line(args));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        hwnd: std::ptr::null_mut(),
        lpVerb: operation.as_ptr(),
        lpFile: file.as_ptr(),
        lpParameters: params.as_ptr(),
        lpDirectory: std::ptr::null(),
        nShow: SW_SHOWNORMAL,
        ..Default::default()
    };

    let launched = unsafe { ShellExecuteExW(&mut info) };
    if launched == 0 {
        bail!("ShellExecuteExW runas failed with Win32 error {}", unsafe {
            GetLastError()
        });
    }

    if info.hProcess.is_null() {
        return Ok(0);
    }

    let wait_result = unsafe { WaitForSingleObject(info.hProcess, INFINITE) };
    if wait_result == WAIT_FAILED {
        let error = unsafe { GetLastError() };
        unsafe {
            CloseHandle(info.hProcess);
        }
        bail!("WaitForSingleObject failed with Win32 error {error}");
    }
    if wait_result != WAIT_OBJECT_0 {
        unsafe {
            CloseHandle(info.hProcess);
        }
        bail!("unexpected wait result from elevated setup process: {wait_result}");
    }

    let mut exit_code = 0;
    let got_exit_code = unsafe { GetExitCodeProcess(info.hProcess, &mut exit_code) };
    unsafe {
        CloseHandle(info.hProcess);
    }
    if got_exit_code == 0 {
        bail!("GetExitCodeProcess failed with Win32 error {}", unsafe {
            GetLastError()
        });
    }
    Ok(exit_code)
}

#[cfg(windows)]
fn join_command_line(args: &[String]) -> String {
    args.iter()
        .map(|arg| quote_arg(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(windows)]
fn quote_arg(arg: &str) -> String {
    if !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }
    format!("\"{}\"", arg.replace('"', "\\\""))
}

#[cfg(windows)]
fn show_message(title: &str, message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};
    let title = to_wide(title);
    let message = to_wide(message);
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

#[cfg(windows)]
fn show_yes_no_cancel(title: &str, message: &str) -> Option<bool> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDCANCEL, IDNO, IDYES, MB_DEFBUTTON2, MB_ICONQUESTION, MB_YESNOCANCEL,
    };
    let title = to_wide(title);
    let message = to_wide(message);
    let result = unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_YESNOCANCEL | MB_ICONQUESTION | MB_DEFBUTTON2,
        )
    };
    match result {
        IDYES => Some(true),
        IDNO => Some(false),
        IDCANCEL => None,
        _ => None,
    }
}

#[cfg(windows)]
fn to_wide(value: &str) -> Vec<u16> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(windows)]
fn to_wide_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(values: &[&str]) -> Result<SetupCommand> {
        parse_args_for_exe(
            Path::new(SETUP_EXE_NAME),
            values.iter().map(|value| value.to_string()),
        )
    }

    #[test]
    fn defaults_to_install() {
        assert_eq!(
            parse(&[]).unwrap(),
            SetupCommand::Install {
                target_dir: None,
                quiet: false,
                start_on_login: None
            }
        );
    }

    #[test]
    fn uninstall_exe_defaults_to_uninstall_without_command_line() {
        assert_eq!(
            parse_args_for_exe(
                Path::new(r"C:\Program Files\ClearLine\installer\ClearLineUninstall.exe"),
                std::iter::empty::<String>()
            )
            .unwrap(),
            SetupCommand::Uninstall {
                target_dir: None,
                quiet: false,
                remove_vb_cable: None
            }
        );
    }

    #[test]
    fn interactive_install_prompts_for_target_before_elevation() {
        assert!(needs_interactive_install_dir(None, false));
        assert!(!needs_interactive_install_dir(None, true));
        assert!(!needs_interactive_install_dir(
            Some(Path::new(r"C:\Program Files\ClearLine")),
            false
        ));
    }

    #[test]
    fn usage_mentions_gui_uninstaller() {
        assert!(usage().contains("ClearLineUninstall.exe"));
        assert!(usage().contains("选择安装路径"));
        assert!(usage().contains("--start-on-login"));
        assert!(usage().contains("--no-start-on-login"));
    }

    #[test]
    fn install_startup_choice_is_supported_by_source() {
        let source = include_str!("main.rs");

        for marker in [
            "start_on_login",
            "--start-on-login",
            "--no-start-on-login",
            "select_start_on_login_interactively",
            "show_msi_style_install_wizard",
            "ClearLineMsiStyleInstallWizard",
            "CreateWindowExW",
            "目标文件夹",
            "开机自启动（登录后最小化到系统托盘）",
            "set_installed_app_startup",
            "--minimized",
        ] {
            assert!(source.contains(marker), "setup source missing {marker}");
        }
    }

    #[test]
    fn uninstall_registry_points_to_gui_uninstaller() {
        let (uninstall, quiet_uninstall) =
            uninstall_registry_strings(Path::new(r"C:\Program Files\ClearLine"));

        assert!(uninstall.contains("ClearLineUninstall.exe"));
        assert!(!uninstall.contains("--uninstall"));
        assert_eq!(
            quiet_uninstall,
            r#""C:\Program Files\ClearLine/installer/ClearLineUninstall.exe" --quiet"#
        );
    }

    #[test]
    fn parses_uninstall_quiet_with_target() {
        assert_eq!(
            parse(&["--uninstall", "--quiet", "--target", r"C:\ClearLine"]).unwrap(),
            SetupCommand::Uninstall {
                target_dir: Some(PathBuf::from(r"C:\ClearLine")),
                quiet: true,
                remove_vb_cable: None
            }
        );
    }

    #[test]
    fn parses_uninstall_vb_cable_choices() {
        assert_eq!(
            parse(&["--uninstall", "--remove-vb-cable"]).unwrap(),
            SetupCommand::Uninstall {
                target_dir: None,
                quiet: false,
                remove_vb_cable: Some(true)
            }
        );
        assert_eq!(
            parse(&["--uninstall", "--keep-vb-cable"]).unwrap(),
            SetupCommand::Uninstall {
                target_dir: None,
                quiet: false,
                remove_vb_cable: Some(false)
            }
        );
    }

    #[test]
    fn parses_install_startup_choices() {
        assert_eq!(
            parse(&["--install", "--start-on-login"]).unwrap(),
            SetupCommand::Install {
                target_dir: None,
                quiet: false,
                start_on_login: Some(true)
            }
        );
        assert_eq!(
            parse(&["--install", "--no-start-on-login"]).unwrap(),
            SetupCommand::Install {
                target_dir: None,
                quiet: false,
                start_on_login: Some(false)
            }
        );
    }

    #[cfg(windows)]
    #[test]
    fn elevation_args_preserve_target_and_quiet_mode() {
        assert_eq!(
            elevation_args(
                &["--install"],
                Some(Path::new(r"C:\Program Files\ClearLine")),
                true
            ),
            vec![
                "--install".to_string(),
                "--target".to_string(),
                r"C:\Program Files\ClearLine".to_string(),
                "--quiet".to_string()
            ]
        );
    }

    #[test]
    fn parses_cleanup_install_dir_quiet() {
        assert_eq!(
            parse(&[
                "--cleanup-install-dir",
                r"C:\Program Files\ClearLine",
                "--quiet"
            ])
            .unwrap(),
            SetupCommand::CleanupInstallDir {
                target_dir: PathBuf::from(r"C:\Program Files\ClearLine"),
                quiet: true
            }
        );
    }
}
