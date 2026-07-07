# DeepFilterNet + AEC Default Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 ClearLine 的用户路径收敛为 DeepFilterNet + 回音消除，移除 RNNoise 的默认 App 入口和高质量模式回退。

**Architecture:** `clearline-app` 负责默认设置、UI 入口、模型可用性检查和用户提示；`clearline-core` 保留 RNNoise/低延迟代码作为 legacy/dev feature，但生产 `clearline-app` 不再默认启用。音频管线启动时必须拿到有效 DeepFilterNet bundle；模型缺失或加载失败时停止启动并显示明确错误，不再隐式落到 RNNoise。

**Tech Stack:** Rust workspace, `eframe/egui`, `clearline-core`, DeepFilterNet feature, AEC feature, `cargo test/check/fmt`。

---

### Task 1: App 默认值与设置迁移

**Files:**
- Modify: `clearline-app/src/settings.rs`
- Modify: `clearline-app/src/main.rs`

- [ ] **Step 1: Write failing tests**

Add/adjust tests so defaults are high quality and echo cancellation is on:

```rust
assert_eq!(PersistedSettings::default().suppressor_mode, MODE_HIGH_QUALITY);
assert!(PersistedSettings::default().echo_cancellation_enabled);
assert_eq!(suppressor_mode_from_setting(MODE_LOW_LATENCY), clearline_core::SuppressorMode::HighQuality);
assert_eq!(default_suppressor_mode(), SuppressorMode::HighQuality);
```

- [ ] **Step 2: Run tests to verify RED**

Run: `cargo test -p clearline-app settings_defaults_to_high_quality_and_aec -- --nocapture`

Expected: FAIL because current defaults are low latency and AEC off.

- [ ] **Step 3: Implement defaults**

Change `PersistedSettings::default`, `default_suppressor_mode`, `suppressor_mode_from_setting`, and `ClearLineApp::new_with_settings_for_tests` to use high quality + AEC enabled by default. Legacy saved `low_latency` migrates to high quality.

- [ ] **Step 4: Verify GREEN**

Run: `cargo test -p clearline-app settings_defaults_to_high_quality_and_aec -- --nocapture`

Expected: PASS.

### Task 2: Remove RNNoise from App user path

**Files:**
- Modify: `clearline-app/Cargo.toml`
- Modify: `clearline-app/src/main.rs`

- [ ] **Step 1: Write failing tests**

Assert user-selectable modes contain only high quality and App manifest does not enable RNNoise:

```rust
assert_eq!(user_selectable_suppressor_modes(), [SuppressorMode::HighQuality]);
assert!(!include_str!("../Cargo.toml").contains("features = [\"rnnoise\"]"));
```

- [ ] **Step 2: Run tests to verify RED**

Run:

```bash
cargo test -p clearline-app user_selectable_modes_only_show_high_quality -- --nocapture
cargo test -p clearline-app app_default_features_do_not_enable_rnnoise_backend -- --nocapture
```

Expected: FAIL because current UI exposes low latency and `clearline-core` dependency enables `rnnoise`.

- [ ] **Step 3: Implement minimal change**

Change `user_selectable_suppressor_modes` to return `[SuppressorMode::HighQuality]`, remove `features = ["rnnoise"]` from the `clearline-core` dependency in `clearline-app/Cargo.toml`, and update labels/tests that claimed RNNoise was a normal App path.

- [ ] **Step 4: Verify GREEN**

Run:

```bash
cargo test -p clearline-app user_selectable_modes_only_show_high_quality -- --nocapture
cargo test -p clearline-app app_default_features_do_not_enable_rnnoise_backend -- --nocapture
```

Expected: PASS.

### Task 3: No RNNoise fallback when DeepFilterNet is unavailable

**Files:**
- Modify: `clearline-app/src/main.rs`
- Modify: `clearline-core/src/suppressor.rs`
- Modify: `clearline-core/src/pipeline.rs`

- [ ] **Step 1: Write failing tests**

Assert missing/incomplete model messages do not contain RNNoise and strict DeepFilterNet creation returns an error instead of low latency fallback:

```rust
let warning = deepfilter_startup_warning(SuppressorMode::HighQuality, &DeepFilterModelUiStatus::MissingAsset("enc.onnx".to_owned())).unwrap();
assert!(!warning.contains("RNNoise"));
```

Core should expose a strict creation path used by `AudioPipeline` that returns `Err(ClearLineError::ModelLoad(_))` when the high-quality path has no DeepFilterNet bundle instead of falling back.

- [ ] **Step 2: Run tests to verify RED**

Run: `cargo test -p clearline-app deepfilter_startup_warning_reports_model_unavailable -- --nocapture`
Run: `cargo test -p clearline-core --features deepfilternet strict_deepfilternet_creation_requires_model_bundle -- --nocapture`

Expected: FAIL because current messages and core fallback mention/use RNNoise.

- [ ] **Step 3: Implement strict startup**

App start should refuse high quality startup unless `packaged_deepfilter_model_status()` is valid and `deepfilter_model_bundle_for_pipeline()` returns a bundle. Core pipeline should use strict suppressor creation so model load errors propagate instead of falling back.

- [ ] **Step 4: Verify GREEN**

Run the same two tests. Expected: PASS.

### Task 4: Docs, full verification, package

**Files:**
- Modify: `README.md`
- Update: `dist/ClearLine.exe`

- [ ] **Step 1: Update docs**

Rewrite MVP notes so the user path is DeepFilterNet + AEC, RNNoise is legacy/dev-only, and model missing means startup error rather than fallback.

- [ ] **Step 2: Run verification**

Run:

```bash
cargo fmt --all
cargo test -p clearline-app
cargo test -p clearline-core --features deepfilternet strict_deepfilternet_creation_requires_model_bundle -- --nocapture
cargo check
```

- [ ] **Step 3: Build Windows exe and copy to dist**

Run:

```powershell
Set-Location E:\Dev\ClearLine
cargo build -p clearline-app --release
Copy-Item -Force .\target\release\clearline-app.exe .\dist\ClearLine.exe
```

- [ ] **Step 4: Commit**

```bash
git add clearline-app clearline-core README.md docs/superpowers/plans/2026-07-07-deepfilternet-aec-default.md
git commit -m "feat: default to DeepFilterNet AEC path"
```
