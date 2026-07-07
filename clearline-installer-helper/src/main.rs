use std::env;
#[cfg(windows)]
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};

#[cfg(windows)]
const HARDWARE_ID: &str = r"Root\ClearLineVirtualAudio";
#[cfg(windows)]
const INSTANCE_ID: &str = r"ROOT\CLEARLINEVIRTUALAUDIO\0000";
#[cfg(windows)]
const DEVICE_NAME: &str = "ClearLine Virtual Microphone";
const INF_NAME: &str = "ClearLineVirtualAudio.inf";
const VB_CABLE_INF_NAME: &str = "vbMmeCable64_win10.inf";
#[cfg(windows)]
const VB_CABLE_HARDWARE_ID: &str = "VBAudioVACWDM";
#[cfg(windows)]
const VB_CABLE_DEVICE_NAME: &str = "VB-Audio Virtual Cable";

#[derive(Debug, Clone, PartialEq, Eq)]
enum CommandLine {
    InstallDriver {
        package: PathBuf,
    },
    UninstallDriver,
    InstallVbCable {
        package: PathBuf,
    },
    UninstallVbCable,
    SaveDefaultAudio {
        path: PathBuf,
    },
    RestoreDefaultAudio {
        path: PathBuf,
    },
    SetDefaultVbCableMic,
    VerifyInstall {
        app: PathBuf,
        require_device: bool,
        require_vb_cable: bool,
    },
    VerifyVbCable,
}

fn main() -> Result<()> {
    let command = parse_args(env::args().skip(1))?;
    match command {
        CommandLine::InstallDriver { package } => install_driver(&package),
        CommandLine::UninstallDriver => uninstall_driver(),
        CommandLine::InstallVbCable { package } => install_vb_cable(&package),
        CommandLine::UninstallVbCable => uninstall_vb_cable(),
        CommandLine::SaveDefaultAudio { path } => save_default_audio(&path),
        CommandLine::RestoreDefaultAudio { path } => restore_default_audio(&path),
        CommandLine::SetDefaultVbCableMic => set_default_vb_cable_mic(),
        CommandLine::VerifyInstall {
            app,
            require_device,
            require_vb_cable,
        } => verify_install(&app, require_device, require_vb_cable),
        CommandLine::VerifyVbCable => verify_vb_cable(),
    }
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<CommandLine> {
    let mut args = args.into_iter();
    let Some(command) = args.next() else {
        bail!(usage());
    };

    match command.as_str() {
        "install-driver" => {
            let mut package = None;
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--package" => {
                        package = Some(PathBuf::from(expect_value(&mut args, "--package")?))
                    }
                    _ => bail!("unknown install-driver argument: {arg}\n{}", usage()),
                }
            }
            Ok(CommandLine::InstallDriver {
                package: package
                    .ok_or_else(|| anyhow!("install-driver requires --package\n{}", usage()))?,
            })
        }
        "uninstall-driver" => {
            if let Some(extra) = args.next() {
                bail!("unknown uninstall-driver argument: {extra}\n{}", usage());
            }
            Ok(CommandLine::UninstallDriver)
        }
        "install-vbcable" => {
            let mut package = None;
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--package" => {
                        package = Some(PathBuf::from(expect_value(&mut args, "--package")?))
                    }
                    _ => bail!("unknown install-vbcable argument: {arg}\n{}", usage()),
                }
            }
            Ok(CommandLine::InstallVbCable {
                package: package
                    .ok_or_else(|| anyhow!("install-vbcable requires --package\n{}", usage()))?,
            })
        }
        "uninstall-vbcable" => {
            if let Some(extra) = args.next() {
                bail!("unknown uninstall-vbcable argument: {extra}\n{}", usage());
            }
            Ok(CommandLine::UninstallVbCable)
        }
        "save-default-audio" => {
            let mut path = None;
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--path" => path = Some(PathBuf::from(expect_value(&mut args, "--path")?)),
                    _ => bail!("unknown save-default-audio argument: {arg}\n{}", usage()),
                }
            }
            Ok(CommandLine::SaveDefaultAudio {
                path: path
                    .ok_or_else(|| anyhow!("save-default-audio requires --path\n{}", usage()))?,
            })
        }
        "restore-default-audio" => {
            let mut path = None;
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--path" => path = Some(PathBuf::from(expect_value(&mut args, "--path")?)),
                    _ => bail!("unknown restore-default-audio argument: {arg}\n{}", usage()),
                }
            }
            Ok(CommandLine::RestoreDefaultAudio {
                path: path
                    .ok_or_else(|| anyhow!("restore-default-audio requires --path\n{}", usage()))?,
            })
        }
        "set-default-vbcable-mic" => {
            if let Some(extra) = args.next() {
                bail!(
                    "unknown set-default-vbcable-mic argument: {extra}\n{}",
                    usage()
                );
            }
            Ok(CommandLine::SetDefaultVbCableMic)
        }
        "verify-install" => {
            let mut app = None;
            let mut require_device = false;
            let mut require_vb_cable = false;
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--app" => app = Some(PathBuf::from(expect_value(&mut args, "--app")?)),
                    "--require-device" => require_device = true,
                    "--require-vb-cable" => require_vb_cable = true,
                    _ => bail!("unknown verify-install argument: {arg}\n{}", usage()),
                }
            }
            Ok(CommandLine::VerifyInstall {
                app: app.ok_or_else(|| anyhow!("verify-install requires --app\n{}", usage()))?,
                require_device,
                require_vb_cable,
            })
        }
        "verify-vb-cable" => {
            if let Some(extra) = args.next() {
                bail!("unknown verify-vb-cable argument: {extra}\n{}", usage());
            }
            Ok(CommandLine::VerifyVbCable)
        }
        _ => bail!("unknown command: {command}\n{}", usage()),
    }
}

fn expect_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String> {
    args.next()
        .filter(|value| !value.starts_with("--"))
        .ok_or_else(|| anyhow!("{flag} requires a value"))
}

fn usage() -> &'static str {
    "usage:\n  clearline-installer-helper install-driver --package <driver-package-dir>\n  clearline-installer-helper uninstall-driver\n  clearline-installer-helper install-vbcable --package <official-vb-cable-dir>\n  clearline-installer-helper uninstall-vbcable\n  clearline-installer-helper save-default-audio --path <snapshot-file>\n  clearline-installer-helper restore-default-audio --path <snapshot-file>\n  clearline-installer-helper set-default-vbcable-mic\n  clearline-installer-helper verify-install --app <install-dir> [--require-device] [--require-vb-cable]\n  clearline-installer-helper verify-vb-cable"
}

fn require_file(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    if !path.is_file() {
        bail!("required file missing: {}", path.display());
    }
    Ok(())
}

fn validate_driver_package(package: &Path) -> Result<PathBuf> {
    if !package.is_dir() {
        bail!("driver package directory missing: {}", package.display());
    }
    for name in [
        INF_NAME,
        "TabletAudioSample.sys",
        "KeywordDetectorContosoAdapter.dll",
        "clearline.cat",
    ] {
        require_file(package.join(name))?;
    }
    Ok(package.join(INF_NAME))
}

fn validate_vb_cable_package(package: &Path) -> Result<PathBuf> {
    if !package.is_dir() {
        bail!("VB-CABLE package directory missing: {}", package.display());
    }
    for name in [
        "readme.txt",
        VB_CABLE_INF_NAME,
        "vbaudio_cable64_win10.sys",
        "vbaudio_cable64_win10.cat",
    ] {
        require_file(package.join(name))?;
    }
    Ok(package.join(VB_CABLE_INF_NAME))
}

fn verify_payload(app: &Path) -> Result<()> {
    require_file(app.join("ClearLine.exe"))?;
    for name in ["enc.onnx", "erb_dec.onnx", "df_dec.onnx", "config.ini"] {
        require_file(app.join("models").join("deepfilternet").join(name))?;
    }
    require_file(
        app.join("virtual-audio")
            .join("vb-cable")
            .join("VBCABLE_Driver_Pack45.zip"),
    )?;
    Ok(())
}

#[cfg(windows)]
fn install_driver(package: &Path) -> Result<()> {
    let inf = validate_driver_package(package)?
        .canonicalize()
        .map_err(|err| anyhow!("canonicalize {}: {err}", package.join(INF_NAME).display()))?;
    let inf_text = inf.to_string_lossy().into_owned();

    println!("Installing {DEVICE_NAME} from {}", inf.display());

    let _ = run_status(
        "pnputil",
        [
            "/remove-device",
            "/deviceid",
            HARDWARE_ID,
            "/subtree",
            "/force",
        ],
    );
    delete_existing_driver_packages()?;

    run_checked_allowing_exit_codes(
        "pnputil",
        ["/add-driver", inf_text.as_str(), "/install"],
        &[0, 259],
    )?;

    register_root_device(HARDWARE_ID, DEVICE_NAME)?;
    bind_driver(HARDWARE_ID, &inf)?;
    run_checked("pnputil", ["/scan-devices"])?;

    println!("Installed {DEVICE_NAME}.");
    Ok(())
}

#[cfg(not(windows))]
fn install_driver(package: &Path) -> Result<()> {
    validate_driver_package(package)?;
    bail!("install-driver is only supported on Windows")
}

#[cfg(windows)]
fn uninstall_driver() -> Result<()> {
    println!("Uninstalling {DEVICE_NAME}.");
    let _ = run_status("taskkill", ["/IM", "ClearLine.exe", "/F"]);
    let _ = run_status(
        "pnputil",
        [
            "/remove-device",
            "/deviceid",
            HARDWARE_ID,
            "/subtree",
            "/force",
        ],
    );
    delete_existing_driver_packages()?;
    println!("Uninstalled {DEVICE_NAME} driver package when present.");
    Ok(())
}

#[cfg(windows)]
fn install_vb_cable(package: &Path) -> Result<()> {
    let inf = validate_vb_cable_package(package)?
        .canonicalize()
        .map_err(|err| {
            anyhow!(
                "canonicalize {}: {err}",
                package.join(VB_CABLE_INF_NAME).display()
            )
        })?;
    let inf_text = inf.to_string_lossy().into_owned();

    if verify_vb_cable().is_ok() {
        println!("VB-CABLE endpoints already exist; skipping VB-CABLE devnode creation.");
        return Ok(());
    }

    let existing_roots = vb_cable_root_instances()?;
    if !existing_roots.is_empty() && !has_exactly_one_vb_cable_root_instance(&existing_roots) {
        bail!(
            "multiple VB-CABLE root devnodes already exist: {}",
            format_instances(&existing_roots)
        );
    }

    println!(
        "Installing basic VB-Audio VB-CABLE from {} with hardware id {VB_CABLE_HARDWARE_ID}",
        inf.display()
    );
    run_checked_allowing_exit_codes(
        "pnputil",
        ["/add-driver", inf_text.as_str(), "/install"],
        &[0, 259],
    )?;
    if existing_roots.is_empty() {
        println!("No existing VB-CABLE root devnode found; creating one root-enumerated device.");
        register_root_device(VB_CABLE_HARDWARE_ID, VB_CABLE_DEVICE_NAME)?;
    } else {
        println!(
            "Reusing existing VB-CABLE root devnode {}; not creating another one.",
            existing_roots[0]
        );
    }
    bind_driver(VB_CABLE_HARDWARE_ID, &inf)?;
    run_checked("pnputil", ["/scan-devices"])?;

    let installed_roots = vb_cable_root_instances()?;
    if !installed_roots.is_empty() && !has_exactly_one_vb_cable_root_instance(&installed_roots) {
        bail!(
            "VB-CABLE install resulted in multiple root devnodes: {}",
            format_instances(&installed_roots)
        );
    }
    println!("Installed basic VB-Audio VB-CABLE devnode.");
    Ok(())
}

#[cfg(windows)]
fn uninstall_vb_cable() -> Result<()> {
    println!("Uninstalling basic VB-Audio VB-CABLE.");
    for instance in vb_cable_root_instances()? {
        println!("Removing VB-CABLE root device {instance}");
        let _ = run_status(
            "pnputil",
            ["/remove-device", instance.as_str(), "/subtree", "/force"],
        );
    }
    delete_vb_cable_driver_packages()?;
    run_checked("pnputil", ["/scan-devices"])?;
    println!("Uninstalled basic VB-Audio VB-CABLE when present.");
    Ok(())
}

#[cfg(windows)]
fn save_default_audio(path: &Path) -> Result<()> {
    let snapshot = audio_defaults::capture_snapshot()?;
    snapshot.save(path)?;
    println!("Saved default audio device snapshot: {}", path.display());
    Ok(())
}

#[cfg(windows)]
fn restore_default_audio(path: &Path) -> Result<()> {
    let snapshot = audio_defaults::DefaultAudioSnapshot::load(path)?;
    audio_defaults::restore_snapshot(&snapshot)?;
    println!(
        "Restored default audio devices from snapshot: {}",
        path.display()
    );
    Ok(())
}

#[cfg(windows)]
fn set_default_vb_cable_mic() -> Result<()> {
    audio_defaults::set_default_capture_endpoint_by_name("CABLE Output")?;
    println!("Set CABLE Output as default recording and communications device.");
    Ok(())
}

#[cfg(not(windows))]
fn install_vb_cable(package: &Path) -> Result<()> {
    validate_vb_cable_package(package)?;
    bail!("install-vbcable is only supported on Windows")
}

#[cfg(not(windows))]
fn uninstall_vb_cable() -> Result<()> {
    bail!("uninstall-vbcable is only supported on Windows")
}

#[cfg(not(windows))]
fn save_default_audio(_path: &Path) -> Result<()> {
    bail!("save-default-audio is only supported on Windows")
}

#[cfg(not(windows))]
fn restore_default_audio(_path: &Path) -> Result<()> {
    bail!("restore-default-audio is only supported on Windows")
}

#[cfg(not(windows))]
fn set_default_vb_cable_mic() -> Result<()> {
    bail!("set-default-vbcable-mic is only supported on Windows")
}

#[cfg(not(windows))]
fn uninstall_driver() -> Result<()> {
    bail!("uninstall-driver is only supported on Windows")
}

#[cfg(windows)]
fn verify_install(app: &Path, require_device: bool, require_vb_cable: bool) -> Result<()> {
    verify_payload(app)?;
    if require_device {
        run_checked("pnputil", ["/enum-devices", "/instanceid", INSTANCE_ID])?;
    }
    if require_vb_cable {
        verify_vb_cable()?;
    }
    println!("ClearLine installation verified: {}", app.display());
    Ok(())
}

#[cfg(not(windows))]
fn verify_install(app: &Path, require_device: bool, require_vb_cable: bool) -> Result<()> {
    verify_payload(app)?;
    if require_device {
        bail!("device verification is only supported on Windows")
    }
    if require_vb_cable {
        bail!("VB-CABLE endpoint verification is only supported on Windows")
    }
    println!("ClearLine installation payload verified: {}", app.display());
    Ok(())
}

#[cfg(windows)]
fn verify_vb_cable() -> Result<()> {
    use cpal::traits::HostTrait;

    let root_instances = vb_cable_root_instances()?;
    if root_instances.is_empty() {
        println!("VB-CABLE root devnodes: none found by pnputil");
    } else {
        println!(
            "VB-CABLE root devnodes: {}",
            format_instances(&root_instances)
        );
    }
    if !root_instances.is_empty() && !has_exactly_one_vb_cable_root_instance(&root_instances) {
        bail!(
            "multiple VB-CABLE root devnodes found: {}",
            format_instances(&root_instances)
        );
    }

    let host = cpal::default_host();
    let render_names = endpoint_names(host.output_devices()?);
    let capture_names = endpoint_names(host.input_devices()?);

    println!("Render endpoints: {}", render_names.join(" | "));
    println!("Capture endpoints: {}", capture_names.join(" | "));

    if has_vb_cable_pair(&render_names, &capture_names) {
        println!("VB-CABLE endpoints verified: CABLE Input / CABLE Output");
        return Ok(());
    }

    bail!("VB-CABLE endpoints not found: expected render 'CABLE Input' / 'CABLE In 16 Ch' and capture 'CABLE Output'")
}

#[cfg(not(windows))]
fn verify_vb_cable() -> Result<()> {
    bail!("VB-CABLE endpoint verification is only supported on Windows")
}

#[cfg(windows)]
fn endpoint_names<D>(devices: D) -> Vec<String>
where
    D: IntoIterator,
    D::Item: cpal::traits::DeviceTrait,
{
    devices
        .into_iter()
        .map(|device| device.to_string())
        .collect()
}

#[cfg(any(windows, test))]
fn has_vb_cable_pair(render_names: &[impl AsRef<str>], capture_names: &[impl AsRef<str>]) -> bool {
    render_names
        .iter()
        .any(|name| is_basic_vb_cable_endpoint(name.as_ref(), "input"))
        && capture_names
            .iter()
            .any(|name| is_basic_vb_cable_endpoint(name.as_ref(), "output"))
}

#[cfg(any(windows, test))]
fn is_basic_vb_cable_endpoint(name: &str, direction: &str) -> bool {
    let normalized = name.to_ascii_lowercase();
    let expected = format!("cable {direction}");
    let expected_2024_render = direction == "input" && normalized.contains("cable in");
    (normalized.contains(&expected) || expected_2024_render)
        && !normalized.contains("cable-a")
        && !normalized.contains("cable-b")
        && !normalized.contains("cable-c")
        && !normalized.contains("cable-d")
}

#[cfg(windows)]
fn vb_cable_root_instances() -> Result<Vec<String>> {
    let output =
        run_output_allowing_exit_codes("pnputil", ["/enum-devices", "/class", "MEDIA"], &[0, 259])?;
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(parse_vb_cable_root_instances(&text))
}

#[cfg(any(windows, test))]
fn parse_vb_cable_root_instances(pnputil_output: &str) -> Vec<String> {
    let mut instances = Vec::new();
    for instance_id in pnputil_output
        .lines()
        .filter_map(parse_root_instance_from_line)
        .filter(|instance_id| is_vb_cable_root_instance(instance_id))
    {
        if !instances.iter().any(|existing| existing == instance_id) {
            instances.push(instance_id.to_owned());
        }
    }
    instances
}

#[cfg(any(windows, test))]
fn parse_root_instance_from_line(line: &str) -> Option<&str> {
    if let Some(instance_id) = parse_root_instance_token(line) {
        return Some(instance_id);
    }
    let trimmed = line.trim();
    let (label, value) = trimmed.split_once(':')?;
    let label = label.trim().to_ascii_lowercase();
    if label == "instance id" || label == "instanceid" {
        let value = value.trim();
        if !value.is_empty() {
            return Some(value);
        }
    }
    None
}

#[cfg(any(windows, test))]
fn parse_root_instance_token(line: &str) -> Option<&str> {
    let uppercase = line.to_ascii_uppercase();
    let start = uppercase.find(r"ROOT\")?;
    let rest = &line[start..];
    let end = rest
        .find(|ch: char| ch.is_whitespace())
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

#[cfg(any(windows, test))]
fn is_vb_cable_root_instance(instance_id: &str) -> bool {
    let normalized = instance_id.to_ascii_uppercase();
    normalized.starts_with(r"ROOT\")
        && normalized.contains("VB-AUDIO")
        && normalized.contains("VIRTUAL")
        && normalized.contains("CABLE")
}

#[cfg(any(windows, test))]
fn has_exactly_one_vb_cable_root_instance(instances: &[String]) -> bool {
    instances.len() == 1
}

#[cfg(windows)]
fn format_instances(instances: &[String]) -> String {
    if instances.is_empty() {
        "(none)".to_owned()
    } else {
        instances.join(", ")
    }
}

#[cfg(windows)]
fn delete_existing_driver_packages() -> Result<()> {
    let windir = env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let inf_dir = windir.join("INF");
    if !inf_dir.is_dir() {
        return Ok(());
    }

    for entry in
        std::fs::read_dir(&inf_dir).map_err(|err| anyhow!("read {}: {err}", inf_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !file_name.to_ascii_lowercase().starts_with("oem") || !file_name.ends_with(".inf") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if text.contains(HARDWARE_ID)
            || text.contains(DEVICE_NAME)
            || text.contains("ClearLineVirtualAudio")
        {
            let _ = run_status(
                "pnputil",
                ["/delete-driver", file_name, "/uninstall", "/force"],
            );
        }
    }
    Ok(())
}

#[cfg(windows)]
fn delete_vb_cable_driver_packages() -> Result<()> {
    let windir = env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let inf_dir = windir.join("INF");
    if !inf_dir.is_dir() {
        return Ok(());
    }

    for entry in
        std::fs::read_dir(&inf_dir).map_err(|err| anyhow!("read {}: {err}", inf_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !file_name.to_ascii_lowercase().starts_with("oem") || !file_name.ends_with(".inf") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let lower = text.to_ascii_lowercase();
        let is_basic_vb_cable = lower.contains("vb-audio virtual cable")
            || lower.contains("vbaudiovacwdm")
            || lower.contains("vbmmecable64_win10")
            || lower.contains("vbaudio_cable64_win10");
        let is_ab_cd = lower.contains("cable-a")
            || lower.contains("cable-b")
            || lower.contains("cable-c")
            || lower.contains("cable-d")
            || lower.contains("vbaudiocablea")
            || lower.contains("vbaudiocableb")
            || lower.contains("vbaudiocablec")
            || lower.contains("vbaudiocabled");
        if is_basic_vb_cable && !is_ab_cd {
            let _ = run_status(
                "pnputil",
                ["/delete-driver", file_name, "/uninstall", "/force"],
            );
        }
    }
    Ok(())
}

#[cfg(windows)]
fn run_checked<I, S>(program: &str, args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    run_checked_allowing_exit_codes(program, args, &[0])
}

#[cfg(windows)]
fn run_checked_allowing_exit_codes<I, S>(program: &str, args: I, allowed: &[i32]) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = run_output_allowing_exit_codes(program, args, allowed)?;
    print_process_output(program, &output);
    Ok(())
}

#[cfg(windows)]
fn run_output_allowing_exit_codes<I, S>(
    program: &str,
    args: I,
    allowed: &[i32],
) -> Result<std::process::Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .map_err(|err| anyhow!("start {program}: {err}"))?;
    let code = output.status.code().unwrap_or(-1);
    if allowed.contains(&code) {
        return Ok(output);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    bail!("{program} failed with exit code {code}\nstdout:\n{stdout}\nstderr:\n{stderr}")
}

#[cfg(windows)]
fn run_status<I, S>(program: &str, args: I) -> Result<i32>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .map_err(|err| anyhow!("start {program}: {err}"))?;
    print_process_output(program, &output);
    Ok(output.status.code().unwrap_or(-1))
}

#[cfg(windows)]
fn print_process_output(program: &str, output: &std::process::Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stdout.trim().is_empty() {
        println!("[{program} stdout]\n{stdout}");
    }
    if !stderr.trim().is_empty() {
        eprintln!("[{program} stderr]\n{stderr}");
    }
}

#[cfg(windows)]
fn register_root_device(hardware_id: &str, description: &str) -> Result<()> {
    use std::mem::{size_of, zeroed};
    use windows_sys::core::GUID;
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
        SetupDiCallClassInstaller, SetupDiCreateDeviceInfoList, SetupDiCreateDeviceInfoW,
        SetupDiDestroyDeviceInfoList, SetupDiSetDeviceRegistryPropertyW, DICD_GENERATE_ID,
        DIF_REGISTERDEVICE, SPDRP_HARDWAREID, SP_DEVINFO_DATA,
    };
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_DEVINST_ALREADY_EXISTS};

    const MEDIA_CLASS_GUID: GUID = GUID {
        data1: 0x4d36e96c,
        data2: 0xe325,
        data3: 0x11ce,
        data4: [0xbf, 0xc1, 0x08, 0x00, 0x2b, 0xe1, 0x03, 0x18],
    };

    unsafe {
        let info_set = SetupDiCreateDeviceInfoList(&MEDIA_CLASS_GUID, std::ptr::null_mut());
        if info_set == -1isize {
            return Err(last_os_error("SetupDiCreateDeviceInfoList"));
        }

        let mut data: SP_DEVINFO_DATA = zeroed();
        data.cbSize = size_of::<SP_DEVINFO_DATA>() as u32;
        let device_name = to_wide(description);
        let created = SetupDiCreateDeviceInfoW(
            info_set,
            device_name.as_ptr(),
            &MEDIA_CLASS_GUID,
            device_name.as_ptr(),
            std::ptr::null_mut(),
            DICD_GENERATE_ID,
            &mut data,
        );
        if created == 0 && GetLastError() != ERROR_DEVINST_ALREADY_EXISTS {
            let err = last_os_error("SetupDiCreateDeviceInfoW");
            SetupDiDestroyDeviceInfoList(info_set);
            return Err(err);
        }

        let hardware_multi_sz = to_multi_sz([hardware_id]);
        let set_hw_id = SetupDiSetDeviceRegistryPropertyW(
            info_set,
            &mut data,
            SPDRP_HARDWAREID,
            hardware_multi_sz.as_ptr() as *const u8,
            (hardware_multi_sz.len() * 2) as u32,
        );
        if set_hw_id == 0 {
            let err = last_os_error("SetupDiSetDeviceRegistryPropertyW");
            SetupDiDestroyDeviceInfoList(info_set);
            return Err(err);
        }

        let registered = SetupDiCallClassInstaller(DIF_REGISTERDEVICE, info_set, &data);
        if registered == 0 && GetLastError() != ERROR_DEVINST_ALREADY_EXISTS {
            let err = last_os_error("SetupDiCallClassInstaller");
            SetupDiDestroyDeviceInfoList(info_set);
            return Err(err);
        }

        SetupDiDestroyDeviceInfoList(info_set);
    }
    Ok(())
}

#[cfg(windows)]
fn bind_driver(hardware_id: &str, inf: &Path) -> Result<()> {
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
        UpdateDriverForPlugAndPlayDevicesW, INSTALLFLAG_FORCE, INSTALLFLAG_NONINTERACTIVE,
    };

    let hardware_id = to_wide(hardware_id);
    let inf = to_wide_path(inf);
    let mut reboot_required = 0;
    let result = unsafe {
        UpdateDriverForPlugAndPlayDevicesW(
            std::ptr::null_mut(),
            hardware_id.as_ptr(),
            inf.as_ptr(),
            INSTALLFLAG_FORCE | INSTALLFLAG_NONINTERACTIVE,
            &mut reboot_required,
        )
    };
    if result == 0 {
        return Err(last_os_error("UpdateDriverForPlugAndPlayDevicesW"));
    }
    if reboot_required != 0 {
        println!("Driver install requested a reboot.");
    }
    Ok(())
}

#[cfg(windows)]
fn last_os_error(function: &str) -> anyhow::Error {
    let err = std::io::Error::last_os_error();
    anyhow!("{function} failed: {err}")
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

#[cfg(windows)]
fn to_multi_sz<'a>(values: impl IntoIterator<Item = &'a str>) -> Vec<u16> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    let mut result = Vec::new();
    for value in values {
        result.extend(OsStr::new(value).encode_wide());
        result.push(0);
    }
    result.push(0);
    result
}

#[cfg(windows)]
mod audio_defaults {
    use super::*;
    use std::ffi::c_void;

    use windows::core::{Interface, GUID, HRESULT, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{PROPERTYKEY, S_OK};
    use windows::Win32::Media::Audio::{
        eCapture, eCommunications, eConsole, eMultimedia, eRender, EDataFlow, ERole, IMMDevice,
        IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::StructuredStorage::{
        PropVariantClear, PropVariantToStringAlloc,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
        COINIT_APARTMENTTHREADED, STGM_READ,
    };

    const CLSID_POLICY_CONFIG_CLIENT: GUID =
        GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);
    const PKEY_DEVICE_FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
        pid: 14,
    };

    windows::core::imp::define_interface!(
        IPolicyConfig,
        IPolicyConfig_Vtbl,
        0xf8679f50_850a_41cf_9c72_430f290290c8
    );
    windows::core::imp::interface_hierarchy!(IPolicyConfig, windows::core::IUnknown);

    #[repr(C)]
    #[allow(non_snake_case)]
    pub struct IPolicyConfig_Vtbl {
        pub base__: windows::core::IUnknown_Vtbl,
        pub GetMixFormat:
            unsafe extern "system" fn(*mut c_void, PCWSTR, *mut *mut c_void) -> HRESULT,
        pub GetDeviceFormat:
            unsafe extern "system" fn(*mut c_void, PCWSTR, i32, *mut *mut c_void) -> HRESULT,
        pub ResetDeviceFormat: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT,
        pub SetDeviceFormat:
            unsafe extern "system" fn(*mut c_void, PCWSTR, *const c_void, *const c_void) -> HRESULT,
        pub GetProcessingPeriod:
            unsafe extern "system" fn(*mut c_void, PCWSTR, i32, *mut i64, *mut i64) -> HRESULT,
        pub SetProcessingPeriod:
            unsafe extern "system" fn(*mut c_void, PCWSTR, *const i64) -> HRESULT,
        pub GetShareMode: unsafe extern "system" fn(*mut c_void, PCWSTR, *mut c_void) -> HRESULT,
        pub SetShareMode: unsafe extern "system" fn(*mut c_void, PCWSTR, *const c_void) -> HRESULT,
        pub GetPropertyValue: unsafe extern "system" fn(
            *mut c_void,
            PCWSTR,
            *const PROPERTYKEY,
            *mut c_void,
        ) -> HRESULT,
        pub SetPropertyValue: unsafe extern "system" fn(
            *mut c_void,
            PCWSTR,
            *const PROPERTYKEY,
            *const c_void,
        ) -> HRESULT,
        pub SetDefaultEndpoint: unsafe extern "system" fn(*mut c_void, PCWSTR, ERole) -> HRESULT,
        pub SetEndpointVisibility: unsafe extern "system" fn(*mut c_void, PCWSTR, i32) -> HRESULT,
    }

    impl IPolicyConfig {
        unsafe fn set_default_endpoint(&self, device_id: &str, role: ERole) -> Result<()> {
            let wide = super::to_wide(device_id);
            let result = (Interface::vtable(self).SetDefaultEndpoint)(
                Interface::as_raw(self),
                PCWSTR(wide.as_ptr()),
                role,
            );
            if result == S_OK {
                Ok(())
            } else {
                Err(anyhow!(
                    "IPolicyConfig::SetDefaultEndpoint failed for role {:?}: 0x{:08x}",
                    role,
                    result.0 as u32
                ))
            }
        }
    }

    struct ComApartment;

    impl ComApartment {
        fn new() -> Result<Self> {
            unsafe {
                CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                    .ok()
                    .map_err(|err| anyhow!("CoInitializeEx failed: {err}"))?;
            }
            Ok(Self)
        }
    }

    impl Drop for ComApartment {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }

    #[derive(Debug, Clone, Default, PartialEq, Eq)]
    pub struct DefaultAudioSnapshot {
        render_console: Option<String>,
        render_multimedia: Option<String>,
        render_communications: Option<String>,
        capture_console: Option<String>,
        capture_multimedia: Option<String>,
        capture_communications: Option<String>,
    }

    impl DefaultAudioSnapshot {
        pub fn save(&self, path: &Path) -> Result<()> {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, self.to_text())
                .map_err(|err| anyhow!("write {}: {err}", path.display()))
        }

        pub fn load(path: &Path) -> Result<Self> {
            let text = fs::read_to_string(path)
                .map_err(|err| anyhow!("read {}: {err}", path.display()))?;
            Self::from_text(&text)
        }

        fn to_text(&self) -> String {
            [
                ("render_console", self.render_console.as_deref()),
                ("render_multimedia", self.render_multimedia.as_deref()),
                (
                    "render_communications",
                    self.render_communications.as_deref(),
                ),
                ("capture_console", self.capture_console.as_deref()),
                ("capture_multimedia", self.capture_multimedia.as_deref()),
                (
                    "capture_communications",
                    self.capture_communications.as_deref(),
                ),
            ]
            .into_iter()
            .map(|(key, value)| format!("{key}={}", value.unwrap_or("")))
            .collect::<Vec<_>>()
            .join("\n")
        }

        fn from_text(text: &str) -> Result<Self> {
            let value = |key: &str| -> Option<String> {
                text.lines().find_map(|line| {
                    let (line_key, line_value) = line.split_once('=')?;
                    (line_key == key && !line_value.is_empty()).then(|| line_value.to_owned())
                })
            };
            Ok(Self {
                render_console: value("render_console"),
                render_multimedia: value("render_multimedia"),
                render_communications: value("render_communications"),
                capture_console: value("capture_console"),
                capture_multimedia: value("capture_multimedia"),
                capture_communications: value("capture_communications"),
            })
        }
    }

    pub fn capture_snapshot() -> Result<DefaultAudioSnapshot> {
        let _com = ComApartment::new()?;
        let enumerator = device_enumerator()?;
        Ok(DefaultAudioSnapshot {
            render_console: default_endpoint_id(&enumerator, eRender, eConsole),
            render_multimedia: default_endpoint_id(&enumerator, eRender, eMultimedia),
            render_communications: default_endpoint_id(&enumerator, eRender, eCommunications),
            capture_console: default_endpoint_id(&enumerator, eCapture, eConsole),
            capture_multimedia: default_endpoint_id(&enumerator, eCapture, eMultimedia),
            capture_communications: default_endpoint_id(&enumerator, eCapture, eCommunications),
        })
    }

    pub fn restore_snapshot(snapshot: &DefaultAudioSnapshot) -> Result<()> {
        let _com = ComApartment::new()?;
        let policy = policy_config()?;
        restore_role(&policy, snapshot.render_console.as_deref(), eConsole)?;
        restore_role(&policy, snapshot.render_multimedia.as_deref(), eMultimedia)?;
        restore_role(
            &policy,
            snapshot.render_communications.as_deref(),
            eCommunications,
        )?;
        restore_role(&policy, snapshot.capture_console.as_deref(), eConsole)?;
        restore_role(&policy, snapshot.capture_multimedia.as_deref(), eMultimedia)?;
        restore_role(
            &policy,
            snapshot.capture_communications.as_deref(),
            eCommunications,
        )?;
        Ok(())
    }

    pub fn set_default_capture_endpoint_by_name(name_fragment: &str) -> Result<()> {
        let _com = ComApartment::new()?;
        let enumerator = device_enumerator()?;
        let endpoint = find_endpoint_by_name(&enumerator, eCapture, name_fragment)?;
        let policy = policy_config()?;
        unsafe {
            policy.set_default_endpoint(&endpoint.id, eConsole)?;
            policy.set_default_endpoint(&endpoint.id, eMultimedia)?;
            policy.set_default_endpoint(&endpoint.id, eCommunications)?;
        }
        Ok(())
    }

    fn restore_role(policy: &IPolicyConfig, device_id: Option<&str>, role: ERole) -> Result<()> {
        if let Some(device_id) = device_id {
            unsafe {
                policy.set_default_endpoint(device_id, role)?;
            }
        }
        Ok(())
    }

    fn device_enumerator() -> Result<IMMDeviceEnumerator> {
        unsafe {
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|err| anyhow!("CoCreateInstance(MMDeviceEnumerator) failed: {err}"))
        }
    }

    fn policy_config() -> Result<IPolicyConfig> {
        unsafe {
            CoCreateInstance(&CLSID_POLICY_CONFIG_CLIENT, None, CLSCTX_ALL)
                .map_err(|err| anyhow!("CoCreateInstance(PolicyConfigClient) failed: {err}"))
        }
    }

    fn default_endpoint_id(
        enumerator: &IMMDeviceEnumerator,
        dataflow: EDataFlow,
        role: ERole,
    ) -> Option<String> {
        unsafe {
            let device = enumerator.GetDefaultAudioEndpoint(dataflow, role).ok()?;
            device_id(&device).ok()
        }
    }

    struct EndpointInfo {
        id: String,
        name: String,
    }

    fn find_endpoint_by_name(
        enumerator: &IMMDeviceEnumerator,
        dataflow: EDataFlow,
        name_fragment: &str,
    ) -> Result<EndpointInfo> {
        let endpoints = active_endpoints(enumerator, dataflow)?;
        let normalized_fragment = name_fragment.to_ascii_lowercase();
        endpoints
            .into_iter()
            .find(|endpoint| {
                endpoint
                    .name
                    .to_ascii_lowercase()
                    .contains(&normalized_fragment)
            })
            .ok_or_else(|| anyhow!("audio endpoint containing '{name_fragment}' was not found"))
    }

    fn active_endpoints(
        enumerator: &IMMDeviceEnumerator,
        dataflow: EDataFlow,
    ) -> Result<Vec<EndpointInfo>> {
        let collection = unsafe {
            enumerator
                .EnumAudioEndpoints(dataflow, DEVICE_STATE_ACTIVE)
                .map_err(|err| anyhow!("EnumAudioEndpoints failed: {err}"))?
        };
        let count = unsafe {
            collection
                .GetCount()
                .map_err(|err| anyhow!("IMMDeviceCollection::GetCount failed: {err}"))?
        };
        let mut endpoints = Vec::new();
        for index in 0..count {
            let device = unsafe {
                collection
                    .Item(index)
                    .map_err(|err| anyhow!("IMMDeviceCollection::Item({index}) failed: {err}"))?
            };
            let id = device_id(&device)?;
            let name = friendly_name(&device).unwrap_or_else(|_| id.clone());
            endpoints.push(EndpointInfo { id, name });
        }
        Ok(endpoints)
    }

    fn device_id(device: &IMMDevice) -> Result<String> {
        let id = unsafe {
            device
                .GetId()
                .map_err(|err| anyhow!("IMMDevice::GetId failed: {err}"))?
        };
        let text = pwstr_to_string(id);
        unsafe {
            CoTaskMemFree(Some(id.0 as *const c_void));
        }
        Ok(text)
    }

    fn friendly_name(device: &IMMDevice) -> Result<String> {
        let store = unsafe {
            device
                .OpenPropertyStore(STGM_READ)
                .map_err(|err| anyhow!("OpenPropertyStore failed: {err}"))?
        };
        let mut value = unsafe {
            store.GetValue(&PKEY_DEVICE_FRIENDLY_NAME).map_err(|err| {
                anyhow!("IPropertyStore::GetValue(PKEY_Device_FriendlyName) failed: {err}")
            })?
        };
        let text_ptr = unsafe {
            PropVariantToStringAlloc(&value)
                .map_err(|err| anyhow!("PropVariantToStringAlloc failed: {err}"))?
        };
        let text = pwstr_to_string(text_ptr);
        unsafe {
            CoTaskMemFree(Some(text_ptr.0 as *const c_void));
            let _ = PropVariantClear(&mut value);
        }
        Ok(text)
    }

    fn pwstr_to_string(value: PWSTR) -> String {
        if value.is_null() {
            return String::new();
        }
        unsafe {
            let mut len = 0;
            while *value.0.add(len) != 0 {
                len += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(value.0, len))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn snapshot_round_trips_as_key_value_text() {
            let snapshot = DefaultAudioSnapshot {
                render_console: Some("render-console".to_owned()),
                capture_communications: Some("capture-communications".to_owned()),
                ..DefaultAudioSnapshot::default()
            };

            let parsed = DefaultAudioSnapshot::from_text(&snapshot.to_text()).unwrap();

            assert_eq!(parsed, snapshot);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(values: &[&str]) -> Result<CommandLine> {
        parse_args(values.iter().map(|value| value.to_string()))
    }

    #[test]
    fn parses_install_driver_package() {
        assert_eq!(
            parse(&[
                "install-driver",
                "--package",
                r"C:\ClearLine\driver\package"
            ])
            .unwrap(),
            CommandLine::InstallDriver {
                package: PathBuf::from(r"C:\ClearLine\driver\package"),
            }
        );
    }

    #[test]
    fn parses_uninstall_driver_without_arguments() {
        assert_eq!(
            parse(&["uninstall-driver"]).unwrap(),
            CommandLine::UninstallDriver
        );
    }

    #[test]
    fn parses_install_vbcable_package() {
        assert_eq!(
            parse(&["install-vbcable", "--package", r"C:\ClearLine\vb-cable"]).unwrap(),
            CommandLine::InstallVbCable {
                package: PathBuf::from(r"C:\ClearLine\vb-cable"),
            }
        );
    }

    #[test]
    fn parses_vb_cable_uninstall_and_default_audio_commands() {
        assert_eq!(
            parse(&["uninstall-vbcable"]).unwrap(),
            CommandLine::UninstallVbCable
        );
        assert_eq!(
            parse(&["save-default-audio", "--path", r"C:\ClearLine\audio.txt"]).unwrap(),
            CommandLine::SaveDefaultAudio {
                path: PathBuf::from(r"C:\ClearLine\audio.txt"),
            }
        );
        assert_eq!(
            parse(&["restore-default-audio", "--path", r"C:\ClearLine\audio.txt"]).unwrap(),
            CommandLine::RestoreDefaultAudio {
                path: PathBuf::from(r"C:\ClearLine\audio.txt"),
            }
        );
        assert_eq!(
            parse(&["set-default-vbcable-mic"]).unwrap(),
            CommandLine::SetDefaultVbCableMic
        );
    }

    #[test]
    fn parses_verify_install_with_required_device() {
        assert_eq!(
            parse(&[
                "verify-install",
                "--app",
                r"C:\Program Files\ClearLine",
                "--require-device"
            ])
            .unwrap(),
            CommandLine::VerifyInstall {
                app: PathBuf::from(r"C:\Program Files\ClearLine"),
                require_device: true,
                require_vb_cable: false,
            }
        );
    }

    #[test]
    fn rejects_missing_install_package() {
        let error = parse(&["install-driver"]).unwrap_err().to_string();
        assert!(error.contains("requires --package"));
    }

    #[test]
    fn parses_verify_vb_cable() {
        assert_eq!(
            parse(&["verify-vb-cable"]).unwrap(),
            CommandLine::VerifyVbCable
        );
    }

    #[test]
    fn vb_cable_endpoint_matching_requires_input_and_output() {
        assert!(has_vb_cable_pair(
            &["扬声器 (CABLE Input)", "Realtek Speakers"],
            &["麦克风 (CABLE Output)", "USB Mic"]
        ));
        assert!(has_vb_cable_pair(
            &[
                "CABLE In 16 Ch (VB-Audio Virtual Cable)",
                "Realtek Speakers"
            ],
            &["CABLE Output (VB-Audio Virtual Cable)", "USB Mic"]
        ));
        assert!(!has_vb_cable_pair(
            &["扬声器 (CABLE-A Input)"],
            &["麦克风 (CABLE-A Output)"]
        ));
        assert!(!has_vb_cable_pair(&["扬声器 (CABLE Input)"], &["USB Mic"]));
    }

    #[test]
    fn parses_only_root_vb_cable_instances_from_pnputil_output() {
        let output = r#"
Microsoft PnP Utility

Instance ID:                ROOT\VB-AUDIO_VIRTUAL_CABLE\0000
Device Description:         VB-Audio Virtual Cable
Class Name:                 MEDIA
Class GUID:                 {4d36e96c-e325-11ce-bfc1-08002be10318}
Manufacturer Name:          VB-Audio Software
Status:                     Started
Driver Name:                oem18.inf

Instance ID:                SWD\MMDEVAPI\{0.0.1.00000000}.{11111111-1111-1111-1111-111111111111}
Device Description:         CABLE Output (VB-Audio Virtual Cable)
Class Name:                 AudioEndpoint

Instance ID:                ROOT\VB-AUDIO_VIRTUAL_CABLE\0001
Device Description:         VB-Audio Virtual Cable
Class Name:                 MEDIA
"#;

        assert_eq!(
            parse_vb_cable_root_instances(output),
            vec![
                r"ROOT\VB-AUDIO_VIRTUAL_CABLE\0000".to_owned(),
                r"ROOT\VB-AUDIO_VIRTUAL_CABLE\0001".to_owned(),
            ]
        );
    }

    #[test]
    fn parses_root_vb_cable_instance_from_localized_pnputil_output() {
        let output = r#"
ʵ�� ID:                ROOT\VB-AUDIO_VIRTUAL_CABLE\0000
�豸����:         VB-Audio Virtual Cable
����:                 MEDIA
"#;

        assert_eq!(
            parse_vb_cable_root_instances(output),
            vec![r"ROOT\VB-AUDIO_VIRTUAL_CABLE\0000".to_owned()]
        );
    }

    #[test]
    fn basic_vb_cable_root_count_accepts_exactly_one_instance() {
        assert!(has_exactly_one_vb_cable_root_instance(&[
            r"ROOT\VB-AUDIO_VIRTUAL_CABLE\0000".to_owned()
        ]));
        assert!(!has_exactly_one_vb_cable_root_instance(&[]));
        assert!(!has_exactly_one_vb_cable_root_instance(&[
            r"ROOT\VB-AUDIO_VIRTUAL_CABLE\0000".to_owned(),
            r"ROOT\VB-AUDIO_VIRTUAL_CABLE\0001".to_owned(),
        ]));
    }
}
