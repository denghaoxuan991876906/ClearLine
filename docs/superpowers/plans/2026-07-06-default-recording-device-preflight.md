# Default Recording Device Preflight Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a low-risk Windows sound settings launcher and UI prompt so users can manually set the virtual audio line as the Windows default recording device.

**Architecture:** Keep this in `clearline-app` because this step is UI/desktop integration, not audio pipeline logic. Add a focused `windows_settings` module that exposes the sound settings URI and a platform-gated launcher. Add small UI label helpers and a button inside the output device card; clicking it updates the existing status message.

**Tech Stack:** Rust 2021, existing `eframe/egui`, `std::process::Command` on Windows, existing app test style.

---

## File Map

- Create `clearline-app/src/windows_settings.rs`
  - Owns Windows Settings URI constants and platform-gated launch function.
- Modify `clearline-app/src/main.rs`
  - Add `mod windows_settings;`.
  - Add default recording device prompt under the output device selector.
  - Add label/status helper functions with unit tests.
- Modify `README.md`
  - Document the new sound settings button and manual default recording device step.
- Build artifacts after final verification
  - Rebuild Windows release exe and copy to `dist/ClearLine.exe`.

---

### Task 1: Add Windows sound settings launcher module

**Files:**
- Create: `clearline-app/src/windows_settings.rs`

- [ ] **Step 1: Write failing module tests**

Create `clearline-app/src/windows_settings.rs` with only these tests and minimal imports:

```rust
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
}
```

In `clearline-app/src/main.rs`, add near the top:

```rust
mod windows_settings;
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test -p clearline-app sound_settings_uri_targets_windows_sound_settings
```

Expected: compile failure because `sound_settings_uri` is not defined.

- [ ] **Step 3: Implement minimal launcher module**

Replace the top of `clearline-app/src/windows_settings.rs` with:

```rust
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
```

Keep the tests below this code.

- [ ] **Step 4: Run tests and verify pass**

Run:

```bash
cargo test -p clearline-app sound_settings_uri_targets_windows_sound_settings
cargo test -p clearline-app open_sound_settings_reports_unsupported_platform
```

Expected on WSL/Linux: both tests pass.

- [ ] **Step 5: Commit**

Run:

```bash
git add clearline-app/src/main.rs clearline-app/src/windows_settings.rs
git commit -m "feat: add Windows sound settings launcher"
```

---

### Task 2: Add output device UI prompt and button

**Files:**
- Modify: `clearline-app/src/main.rs`

- [ ] **Step 1: Write failing UI label/status tests**

Add these tests inside `clearline-app/src/main.rs` test module:

```rust
#[test]
fn default_recording_device_prompt_labels_are_chinese() {
    assert_eq!(
        default_recording_device_title(),
        "Windows 默认录音设备"
    );
    assert!(default_recording_device_message().contains("默认录音设备"));
    assert!(default_recording_device_message().contains("虚拟音频线"));
    assert_eq!(open_sound_settings_button_label(), "打开声音设置");
}

#[test]
fn sound_settings_status_messages_are_chinese() {
    assert_eq!(sound_settings_opened_status(), "已打开 Windows 声音设置");
    assert_eq!(
        sound_settings_open_failed_status("仅 Windows 可用"),
        "打开 Windows 声音设置失败：仅 Windows 可用"
    );
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test -p clearline-app default_recording_device_prompt_labels_are_chinese
cargo test -p clearline-app sound_settings_status_messages_are_chinese
```

Expected: compile failure because helper functions are missing.

- [ ] **Step 3: Implement UI helper functions**

Add these helper functions near other label helpers such as `output_device_label`:

```rust
fn default_recording_device_title() -> &'static str {
    "Windows 默认录音设备"
}

fn default_recording_device_message() -> &'static str {
    "如需让 Discord、微信、QQ、浏览器会议等应用使用降噪后的声音，请在 Windows 中把虚拟音频线的录音端设为默认录音设备。"
}

fn open_sound_settings_button_label() -> &'static str {
    "打开声音设置"
}

fn sound_settings_opened_status() -> &'static str {
    "已打开 Windows 声音设置"
}

fn sound_settings_open_failed_status(error: impl std::fmt::Display) -> String {
    format!("打开 Windows 声音设置失败：{error}")
}
```

- [ ] **Step 4: Add UI prompt and click handler**

In `output_device_card`, after the device selector row, add:

```rust
ui.add_space(10.0);
ui.separator();
ui.add_space(10.0);
self.default_recording_device_prompt(ui);
```

Add this method to `impl ClearLineApp` near `output_device_card`:

```rust
fn default_recording_device_prompt(&mut self, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(
                RichText::new(default_recording_device_title())
                    .size(14.0)
                    .strong()
                    .color(ios_text()),
            );
            ui.add_space(2.0);
            ui.label(
                RichText::new(default_recording_device_message())
                    .size(13.0)
                    .color(ios_secondary_text()),
            );
        });

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(
                    egui::Button::new(RichText::new(open_sound_settings_button_label()).color(ios_blue()))
                        .fill(ios_control_fill())
                        .stroke(Stroke::NONE)
                        .corner_radius(12)
                        .min_size(Vec2::new(112.0, 30.0)),
                )
                .clicked()
            {
                match windows_settings::open_sound_settings() {
                    Ok(()) => self.status_message = sound_settings_opened_status().to_owned(),
                    Err(error) => self.status_message = sound_settings_open_failed_status(error),
                }
            }
        });
    });
}
```

- [ ] **Step 5: Run tests and verify pass**

Run:

```bash
cargo test -p clearline-app default_recording_device_prompt_labels_are_chinese
cargo test -p clearline-app sound_settings_status_messages_are_chinese
cargo test -p clearline-app
```

Expected: all app tests pass.

- [ ] **Step 6: Commit**

Run:

```bash
git add clearline-app/src/main.rs
git commit -m "feat: add default recording device prompt"
```

---

### Task 3: Document, verify, and build Windows exe

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Update README**

Add this bullet to the current MVP status list near the output-device/UI bullets:

```markdown
- 输出设备区域提供“打开声音设置”按钮，方便用户把虚拟音频线的录音端手动设为 Windows 默认录音设备；当前不自动修改系统默认设备。
```

Add this note near the development/build section:

```markdown
手动设置默认录音设备：在 ClearLine 的输出设备区域点击“打开声音设置”，然后在 Windows 中把虚拟音频线的录音端设为默认录音设备。
```

- [ ] **Step 2: Run full WSL verification**

Run:

```bash
cargo fmt
cargo fmt --check
cargo check
cargo test -p clearline-app
cargo test -p clearline-core
```

Expected: all commands exit 0.

- [ ] **Step 3: Commit docs/fixes**

Run:

```bash
git add README.md
git commit -m "docs: document default recording device prompt"
```

If verification required code fixes directly related to this feature, include those fixed files in the same commit.

- [ ] **Step 4: Run Windows verification and build exe**

Run:

```bash
'/mnt/c/Users/DHX/.cargo/bin/cargo.exe' check -p clearline-app
'/mnt/c/Users/DHX/.cargo/bin/cargo.exe' check -p clearline-app --no-default-features
'/mnt/c/Users/DHX/.cargo/bin/cargo.exe' build -p clearline-app --release
mkdir -p dist
cp target/release/clearline-app.exe dist/ClearLine.exe
cp target/release/clearline-app.exe dist/ClearLine-default-recording.exe
file dist/ClearLine.exe dist/ClearLine-default-recording.exe
strings -a dist/ClearLine.exe | grep -F "$(git rev-parse --short HEAD)" | head -n 5
```

Expected: Windows checks and release build exit 0; `file` reports PE32+ GUI x86-64 executables; `strings` shows the current git commit.

- [ ] **Step 5: Final status check**

Run:

```bash
git status --short
git log --oneline -8
```

Expected: no uncommitted source/doc changes. `dist/` and `*.exe` are ignored by git.

---

## Manual Testing Instructions After Implementation

1. Run `E:\Dev\ClearLine\dist\ClearLine.exe`.
2. Open the `设备` tab.
3. In the output device card, confirm the `Windows 默认录音设备` prompt is visible.
4. Confirm the button text is `打开声音设置`.
5. Click `打开声音设置`.
6. Confirm Windows opens the sound settings page.
7. Return to ClearLine and confirm the status bar says `已打开 Windows 声音设置`.
8. In Windows sound settings, manually set the virtual audio line recording endpoint as the default recording device.
