# Startup Runtime UX Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make ClearLine clearer before startup and prevent runtime edits to settings that do not affect the already-running audio pipeline.

**Architecture:** Keep changes in `clearline-app/src/main.rs` because this is UI state and status messaging only. Add small pure helper functions for button labels, running-lock text, and DeepFilterNet fallback warning so behavior is unit-testable. Gate runtime-sensitive egui controls with `ui.add_enabled_ui(!is_running, ...)` or `ui.add_enabled(!is_running, ...)`.

**Tech Stack:** Rust 2021, existing `eframe/egui`, existing app unit-test style, Windows release build through `/mnt/c/Users/DHX/.cargo/bin/cargo.exe`.

---

## File Map

- Modify `clearline-app/src/main.rs`
  - Top control label helper will include `can_start`.
  - Add DeepFilterNet startup warning helper.
  - Disable input/output device selectors, refresh buttons, mode buttons, model directory input, and clear button while running.
  - Add unit tests for labels, warnings, and source-level lock regressions.
- Modify `README.md`
  - Document that runtime-sensitive controls are locked while running.
- Build artifacts after final verification
  - Rebuild Windows release exe and copy to `dist/ClearLine.exe`.

---

### Task 1: Clarify top control label and DeepFilterNet fallback warning

**Files:**
- Modify: `clearline-app/src/main.rs`

- [ ] **Step 1: Write failing tests**

Add these tests inside the existing `#[cfg(test)] mod tests` in `clearline-app/src/main.rs`:

```rust
#[test]
fn top_control_label_explains_not_ready_state() {
    assert_eq!(top_control_label(true, true), "停止降噪");
    assert_eq!(top_control_label(false, true), "开始降噪");
    assert_eq!(top_control_label(false, false), "选择设备后开始");
}

#[test]
fn deepfilter_startup_warning_only_reports_invalid_non_empty_high_quality_path() {
    assert_eq!(deepfilter_startup_warning(SuppressorMode::LowLatency, "missing"), None);
    assert_eq!(deepfilter_startup_warning(SuppressorMode::HighQuality, ""), None);
    assert_eq!(deepfilter_startup_warning(SuppressorMode::HighQuality, "   "), None);

    let warning = deepfilter_startup_warning(SuppressorMode::HighQuality, "missing-model-dir")
        .expect("invalid non-empty model path should produce a warning");

    assert!(warning.contains("DeepFilterNet 模型目录无效"));
    assert!(warning.contains("内置高质量后端"));
}
```

Update the existing `top_control_labels_are_chinese_and_tab_independent` test to call the new signature:

```rust
#[test]
fn top_control_labels_are_chinese_and_tab_independent() {
    assert_eq!(top_control_label(false, true), "开始降噪");
    assert_eq!(top_control_label(true, true), "停止降噪");
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test -p clearline-app top_control_label_explains_not_ready_state
cargo test -p clearline-app deepfilter_startup_warning_only_reports_invalid_non_empty_high_quality_path
```

Expected: compile failure because `top_control_label` still takes one argument and `deepfilter_startup_warning` does not exist.

- [ ] **Step 3: Implement label and warning helpers**

Replace `top_control_label` with:

```rust
fn top_control_label(is_running: bool, can_start: bool) -> &'static str {
    if is_running {
        "停止降噪"
    } else if can_start {
        "开始降噪"
    } else {
        "选择设备后开始"
    }
}
```

Update `top_control_switch` button construction to call:

```rust
RichText::new(top_control_label(is_running, can_start))
```

Add this helper near `deepfilter_model_bundle_for_pipeline`:

```rust
fn deepfilter_startup_warning(mode: SuppressorMode, path: &str) -> Option<String> {
    if mode != SuppressorMode::HighQuality || path.trim().is_empty() {
        return None;
    }

    match deepfilter_model_status(path) {
        DeepFilterModelUiStatus::Valid | DeepFilterModelUiStatus::NotConfigured => None,
        DeepFilterModelUiStatus::MissingAsset(filename) => Some(format!(
            "DeepFilterNet 模型目录无效，已使用内置高质量后端：缺少模型文件 {filename}"
        )),
        DeepFilterModelUiStatus::Invalid(message) => Some(format!(
            "DeepFilterNet 模型目录无效，已使用内置高质量后端：{message}"
        )),
    }
}
```

In `start_pipeline`, before building the config, add:

```rust
let model_warning = deepfilter_startup_warning(mode, &self.deepfilter_model_dir);
```

Then in the `Ok(())` branch, replace the status message assignment with:

```rust
let running_status = format!("正在使用{selected_mode_label}输出到 {output_label}");
self.status_message = match model_warning {
    Some(warning) => format!("{warning}；{running_status}"),
    None => running_status,
};
```

- [ ] **Step 4: Run tests and verify pass**

Run:

```bash
cargo test -p clearline-app top_control_label_explains_not_ready_state
cargo test -p clearline-app deepfilter_startup_warning_only_reports_invalid_non_empty_high_quality_path
cargo test -p clearline-app top_control_labels_are_chinese_and_tab_independent
```

Expected: targeted tests pass.

- [ ] **Step 5: Commit**

Run:

```bash
git add clearline-app/src/main.rs
git commit -m "feat: clarify startup readiness status"
```

---

### Task 2: Lock runtime-sensitive UI controls while running

**Files:**
- Modify: `clearline-app/src/main.rs`

- [ ] **Step 1: Write failing tests**

Add these tests inside `clearline-app/src/main.rs` test module:

```rust
#[test]
fn source_omits_removed_runtime_hint_copy() {
    let source = include_str!("main.rs");
    let removed_hint = String::from_utf8(vec![
        232, 191, 144, 232, 161, 140, 228, 184, 173, 233, 156, 128, 229, 129, 156, 230,
        173, 162, 229, 144, 142, 228, 191, 174, 230, 148, 185,
    ])
    .expect("valid UTF-8 test fixture");

    assert!(!source.contains(&removed_hint));
}

#[test]
fn runtime_sensitive_controls_are_disabled_in_source() {
    let source = include_str!("main.rs");

    assert!(source.contains("ui.add_enabled_ui(!is_running, |ui|"));
    assert!(source.contains("ui.add_enabled(!is_running, button)"));
    assert!(source.contains("TextEdit::singleline(&mut self.deepfilter_model_dir)"));
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test -p clearline-app source_omits_removed_runtime_hint_copy
cargo test -p clearline-app runtime_sensitive_controls_are_disabled_in_source
```

Expected: test failure if the removed reminder copy returns, or if source does not yet lock all controls.

- [ ] **Step 3: Keep runtime state implicit**

Do not add a separate running lock helper or extra inline reminder. The disabled controls and the top start/stop button are enough.

- [ ] **Step 4: Disable input device controls while running**

At the start of `device_card`, add:

```rust
let is_running = self.pipeline.state().is_running();
```

Wrap the input combo box with `ui.add_enabled_ui(!is_running, |ui| { ... });`:

```rust
ui.add_enabled_ui(!is_running, |ui| {
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
});
```

Change the refresh button from `ui.add(...)` to:

```rust
ui.add_enabled(!is_running, refresh_button)
```

- [ ] **Step 5: Disable output device controls while running**

At the start of `output_device_card`, add:

```rust
let is_running = self.pipeline.state().is_running();
```

Wrap the output combo box with `ui.add_enabled_ui(!is_running, |ui| { ... });` using the existing combo-box body.

Change the output refresh button from `ui.add(...)` to:

```rust
ui.add_enabled(!is_running, refresh_button)
```

After the selector row, add the same running lock hint label when `is_running` is true.

Keep `self.default_recording_device_prompt(ui);` enabled because opening Windows settings does not alter the current ClearLine audio pipeline.

- [ ] **Step 6: Disable mode buttons and model directory controls while running**

In `mode_card`, change the mode button click from:

```rust
if ui.add(button).clicked() {
```

to:

```rust
if ui.add_enabled(!is_running, button).clicked() {
```

In the DeepFilterNet model directory section, change:

```rust
let response = ui.add(
    egui::TextEdit::singleline(&mut self.deepfilter_model_dir)
        .hint_text(r"C:\Models\DeepFilterNet")
        .desired_width(edit_width),
);
```

to:

```rust
let response = ui.add_enabled(
    !is_running,
    egui::TextEdit::singleline(&mut self.deepfilter_model_dir)
        .hint_text(r"C:\Models\DeepFilterNet")
        .desired_width(edit_width),
);
```

Change the clear button from `ui.add(...)` to:

```rust
ui.add_enabled(!is_running, clear_button)
```

- [ ] **Step 7: Run tests and verify pass**

Run:

```bash
cargo test -p clearline-app source_omits_removed_runtime_hint_copy
cargo test -p clearline-app runtime_sensitive_controls_are_disabled_in_source
cargo test -p clearline-app
```

Expected: all app tests pass.

- [ ] **Step 8: Commit**

Run:

```bash
git add clearline-app/src/main.rs
git commit -m "feat: lock runtime sensitive controls"
```

---

### Task 3: Document, verify, and build Windows exe

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Update README**

Add this bullet to the current MVP status list near UI bullets:

```markdown
- 运行中会锁定输入/输出设备、降噪模式和 DeepFilterNet 模型目录等影响当前音频链路的设置，避免 UI 修改与实际运行配置不一致。
```

Add this testing note near the manual usage/build section:

```markdown
运行中不展示额外锁定提示；“打开声音设置”仍可使用。
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
git commit -m "docs: document runtime locked controls"
```

If verification required code fixes directly related to this feature, include those files in the same commit.

- [ ] **Step 4: Run Windows verification and build exe**

Run:

```bash
'/mnt/c/Users/DHX/.cargo/bin/cargo.exe' check -p clearline-app
'/mnt/c/Users/DHX/.cargo/bin/cargo.exe' check -p clearline-app --no-default-features
'/mnt/c/Users/DHX/.cargo/bin/cargo.exe' build -p clearline-app --release
mkdir -p dist
cp target/release/clearline-app.exe dist/ClearLine.exe
cp target/release/clearline-app.exe dist/ClearLine-runtime-lock.exe
file dist/ClearLine.exe dist/ClearLine-runtime-lock.exe
strings -a dist/ClearLine.exe | grep -F "$(git rev-parse --short HEAD)" | head -n 5
```

Expected: Windows checks and release build exit 0; `file` reports PE32+ GUI x86-64 executables; `strings` shows the current git commit.

- [ ] **Step 5: Final status check**

Run:

```bash
git status --short
git log --oneline -10
```

Expected: no uncommitted source/doc changes. `dist/` and `*.exe` are ignored by git.

---

## Manual Testing Instructions After Implementation

1. Run `E:\Dev\ClearLine\dist\ClearLine.exe`.
2. Start ClearLine normally with selected input and output devices.
3. While running, confirm input device selector, output device selector, refresh buttons, low/high latency mode buttons, DeepFilterNet model directory input, and clear button are disabled.
4. While running, confirm strength and anti-wind controls remain disabled as before.
5. While running, confirm `打开声音设置` is still clickable.
6. Stop ClearLine and confirm the locked controls become editable again.
7. Select high-quality mode, enter a non-empty invalid model directory, start ClearLine, and confirm the status mentions `DeepFilterNet 模型目录无效，已使用内置高质量后端`.
