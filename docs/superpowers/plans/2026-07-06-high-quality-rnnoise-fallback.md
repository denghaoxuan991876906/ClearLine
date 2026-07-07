# High Quality RNNoise Fallback Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make ClearLine high-quality mode fall back to RNNoise when DeepFilterNet is unavailable, and remove user-facing “built-in high quality” wording.

**Architecture:** Change the official suppressor factory path in `clearline-core` so `SuppressorMode::HighQuality` without a successfully loaded DeepFilterNet backend returns `LowLatencySuppressor`. Keep direct `HighQualitySuppressor` construction available for internal tests, but the pipeline entry point should no longer expose `adaptive-quality-v1` as the user fallback. Update app labels and startup warnings to describe DeepFilterNet/RNNoise/final safety fallback accurately.

**Tech Stack:** Rust 2021, existing `clearline-core` suppressor abstractions, existing `clearline-app` egui UI and tests.

---

## File Map

- Modify `clearline-core/src/suppressor.rs`
  - Add a fallible DeepFilterNet high-quality constructor.
  - Change `create_suppressor_with_deepfilternet_bundle` fallback for high-quality mode to `LowLatencySuppressor`.
  - Update tests around high-quality fallback.
- Modify `clearline-app/src/main.rs`
  - Replace “内置高质量后端” startup warning with RNNoise fallback wording.
  - Update status labels for RNNoise, DeepFilterNet, and final safety fallback.
  - Update tests.
- Modify `README.md`
  - Remove user-facing adaptive/high-quality built-in wording.
  - Document DeepFilterNet -> RNNoise -> safety fallback chain.
- Build artifacts after final verification
  - Rebuild Windows release exe and copy to `dist/ClearLine.exe`.

---

### Task 1: Change core high-quality fallback to RNNoise

**Files:**
- Modify: `clearline-core/src/suppressor.rs`

- [ ] **Step 1: Write failing core tests**

Update `create_suppressor_falls_back_when_deepfilternet_model_cannot_load` expected backend from `adaptive-quality-v1` to `nnnoiseless-rnnoise` when both `rnnoise` and `deepfilternet` features are enabled:

```rust
assert_eq!(suppressor.runtime_info().mode(), SuppressorMode::LowLatency);
assert_eq!(suppressor.runtime_info().backend_name(), "nnnoiseless-rnnoise");
assert_eq!(suppressor.runtime_info().frame_size_samples(), 480);
assert!(suppressor.runtime_info().is_real_noise_suppression());
```

Add this test:

```rust
#[test]
fn high_quality_create_suppressor_without_model_uses_low_latency_fallback() {
    let suppressor = create_suppressor(
        SuppressorMode::HighQuality,
        AudioFrameFormat::new(48_000, 1),
        SuppressionStrength::Balanced,
    );
    let info = suppressor.runtime_info();

    assert_eq!(info.mode(), SuppressorMode::LowLatency);
    assert_eq!(info.backend_name(), expected_low_latency_backend_name());
    assert_eq!(info.strength(), Some(SuppressionStrength::Balanced));
}

fn expected_low_latency_backend_name() -> &'static str {
    #[cfg(feature = "rnnoise")]
    {
        "nnnoiseless-rnnoise"
    }
    #[cfg(not(feature = "rnnoise"))]
    {
        "bypass-placeholder"
    }
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test -p clearline-core --features rnnoise,deepfilternet create_suppressor_falls_back_when_deepfilternet_model_cannot_load
cargo test -p clearline-core --features rnnoise high_quality_create_suppressor_without_model_uses_low_latency_fallback
```

Expected: tests fail because high-quality mode still uses `adaptive-quality-v1` fallback.

- [ ] **Step 3: Implement fallible DeepFilterNet construction and RNNoise fallback**

Add `try_new_with_deepfilternet_bundle` inside `impl HighQualitySuppressor`:

```rust
#[cfg(feature = "deepfilternet")]
fn try_new_with_deepfilternet_bundle(
    format: AudioFrameFormat,
    strength: SuppressionStrength,
    model_bundle: DeepFilterNetModelBundle,
) -> ClearLineResult<Self> {
    let backend = DeepFilterNetExperimentalBackend::new(format, strength, model_bundle)
        .map(HighQualityBackend::DeepFilterNet)?;
    let frame_size_samples = backend.frame_size_samples(format);
    Ok(Self {
        format,
        strength,
        chunker: FrameChunker::new(frame_size_samples),
        backend,
        frame_input: vec![0.0; frame_size_samples],
        frame_output: vec![0.0; frame_size_samples],
        pending_output: VecDeque::with_capacity(frame_size_samples * 2),
    })
}
```

In `create_suppressor_with_deepfilternet_bundle`, replace the high-quality branch with:

```rust
SuppressorMode::HighQuality => {
    #[cfg(feature = "deepfilternet")]
    {
        if let Some(model_bundle) = deepfilternet_model_bundle {
            match HighQualitySuppressor::try_new_with_deepfilternet_bundle(
                format,
                strength,
                model_bundle,
            ) {
                Ok(suppressor) => return Box::new(suppressor),
                Err(error) => eprintln!("ClearLine DeepFilterNet load failed, falling back to RNNoise: {error}"),
            }
        }
    }
    #[cfg(not(feature = "deepfilternet"))]
    let _ = deepfilternet_model_bundle;

    Box::new(LowLatencySuppressor::new_with_strength(format, strength))
}
```

- [ ] **Step 4: Run tests and verify pass**

Run:

```bash
cargo test -p clearline-core --features rnnoise,deepfilternet create_suppressor_falls_back_when_deepfilternet_model_cannot_load
cargo test -p clearline-core --features rnnoise high_quality_create_suppressor_without_model_uses_low_latency_fallback
cargo test -p clearline-core --features rnnoise,deepfilternet
```

Expected: all targeted and combined feature tests pass.

- [ ] **Step 5: Commit**

Run:

```bash
git add clearline-core/src/suppressor.rs
git commit -m "feat: fallback high quality mode to rnnoise"
```

---

### Task 2: Update app runtime wording and status labels

**Files:**
- Modify: `clearline-app/src/main.rs`

- [ ] **Step 1: Write failing app tests**

Update `deepfilter_startup_warning_only_reports_invalid_non_empty_high_quality_path` so it expects RNNoise fallback wording:

```rust
assert!(warning.contains("高质量模型不可用"));
assert!(warning.contains("回退到 RNNoise"));
assert!(!warning.contains("内置高质量"));
```

Update `processing_status_labels_are_chinese` RNNoise expected label:

```rust
assert_eq!(
    noise_suppression_status_label(Some(&info)),
    "低延迟降噪（RNNoise）已启用"
);
```

Update `deepfilternet_status_reports_real_backend` expected label:

```rust
assert_eq!(
    noise_suppression_status_label(Some(&info)),
    "高质量降噪（DeepFilterNet 模型）已启用"
);
```

Add a test for final safety fallback:

```rust
#[test]
fn rnnoise_format_fallback_status_is_explicit() {
    let info = PipelineRuntimeInfo::new(
        clearline_core::AudioFrameFormat::new(44_100, 1),
        clearline_core::AudioFrameFormat::new(44_100, 2),
        clearline_core::SuppressorRuntimeInfo::new(
            SuppressorMode::LowLatency,
            "bypass-placeholder",
            441,
            false,
        ),
    );

    assert_eq!(
        noise_suppression_status_label(Some(&info)),
        "RNNoise 不支持当前格式，已使用安全回退"
    );
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test -p clearline-app deepfilter_startup_warning_only_reports_invalid_non_empty_high_quality_path
cargo test -p clearline-app processing_status_labels_are_chinese
cargo test -p clearline-app deepfilternet_status_reports_real_backend
cargo test -p clearline-app rnnoise_format_fallback_status_is_explicit
```

Expected: tests fail because app still mentions built-in high-quality or old labels.

- [ ] **Step 3: Update app labels**

Change `deepfilter_startup_warning` messages to:

```rust
"高质量模型不可用，已回退到 RNNoise：缺少模型文件 {filename}"
"高质量模型不可用，已回退到 RNNoise：{message}"
```

Change `noise_suppression_status_label` mappings:

```rust
"nnnoiseless-rnnoise" => "低延迟降噪（RNNoise）已启用".to_owned(),
"deepfilternet" | "deepfilternet-tract" | "deepfilternet-tract-worker" => {
    "高质量降噪（DeepFilterNet 模型）已启用".to_owned()
}
```

In the fallback match, change `SuppressorMode::LowLatency` to:

```rust
SuppressorMode::LowLatency => "RNNoise 不支持当前格式，已使用安全回退".to_owned(),
```

- [ ] **Step 4: Run tests and verify pass**

Run:

```bash
cargo test -p clearline-app deepfilter_startup_warning_only_reports_invalid_non_empty_high_quality_path
cargo test -p clearline-app processing_status_labels_are_chinese
cargo test -p clearline-app deepfilternet_status_reports_real_backend
cargo test -p clearline-app rnnoise_format_fallback_status_is_explicit
cargo test -p clearline-app
```

Expected: all app tests pass.

- [ ] **Step 5: Commit**

Run:

```bash
git add clearline-app/src/main.rs
git commit -m "feat: clarify rnnoise fallback status"
```

---

### Task 3: Update README and run full verification

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Update README wording**

Replace user-facing `adaptive-quality-v1` high-quality descriptions with DeepFilterNet/RNNoise fallback wording:

```markdown
- `HighQualitySuppressor` 的用户路径以 DeepFilterNet 模型为准；当模型目录为空、无效或模型加载失败时，启动高质量模式会回退到低延迟 RNNoise。RNNoise 也不支持当前格式时才使用安全回退。
```

Remove lines that describe `adaptive-quality-v1` as the current default high-quality user mode.

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
```

Expected: all commands exit 0.

- [ ] **Step 3: Commit docs/fixes**

Run:

```bash
git add README.md
git commit -m "docs: document rnnoise high quality fallback"
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
cp target/release/clearline-app.exe dist/ClearLine-rnnoise-fallback.exe
file dist/ClearLine.exe dist/ClearLine-rnnoise-fallback.exe
strings -a dist/ClearLine.exe | grep -F "$(git rev-parse --short HEAD)" | head -n 5
```

Expected: Windows checks and release build exit 0; `file` reports PE32+ GUI x86-64 executables; `strings` shows the current git commit.

- [ ] **Step 5: Final status check**

Run:

```bash
git status --short
git log --oneline -10
```

Expected: no uncommitted source/doc changes.

---

## Manual Testing Instructions After Implementation

1. Run `E:\Dev\ClearLine\dist\ClearLine.exe`.
2. Select high-quality mode with an empty model directory and start; status should mention fallback to RNNoise.
3. Select high-quality mode with an invalid model directory and start; status should mention `高质量模型不可用，已回退到 RNNoise`.
4. Select high-quality mode with a valid DeepFilterNet model directory and start; status page should show `高质量降噪（DeepFilterNet 模型）已启用`.
5. Select low-latency mode and start; status page should show `低延迟降噪（RNNoise）已启用` when format is supported.
