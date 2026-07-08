# Single Device Audio Recorder Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a small CLI tool that records one selected Windows audio capture device to a WAV file for later ClearLine vs reference-noise-suppression analysis.

**Architecture:** Add a `clearline-lab` workspace crate separate from the production app. The crate exposes `list` and `record` commands, uses `cpal` for device capture, and `hound` for writing mono 48 kHz-ish WAV data in the device's native input format converted to 32-bit float WAV. No formal comparison analysis is included in this step.

**Tech Stack:** Rust 2021, `cpal`, `hound`, `anyhow`, standard-library CLI parsing.

---

### Task 1: Add lab recorder crate

**Files:**
- Modify: `Cargo.toml`
- Create: `clearline-lab/Cargo.toml`
- Create: `clearline-lab/src/main.rs`
- Create: `clearline-lab/src/recorder.rs`

- [ ] **Step 1: Create workspace crate skeleton**

Add `clearline-lab` to workspace members and create a binary crate with dependencies:

```toml
[dependencies]
anyhow.workspace = true
cpal.workspace = true
hound.workspace = true
```

- [ ] **Step 2: Write CLI behavior tests through pure helpers**

In `clearline-lab/src/main.rs`, add tests for parsing:

```rust
assert!(matches!(parse_args(["clearline-lab", "list"]), Ok(Command::List)));
assert!(matches!(parse_args(["clearline-lab", "record", "--device", "mic", "--seconds", "2", "--out", "out.wav"]), Ok(Command::Record(_))));
```

- [ ] **Step 3: Implement minimal CLI parser**

Support:

```text
clearline-lab list
clearline-lab record --device <name-substring> --seconds <n> --out <path.wav>
```

- [ ] **Step 4: Implement device listing**

Use `cpal::default_host().input_devices()` and print each capture device with an index and name.

- [ ] **Step 5: Implement single-device recording**

Resolve a capture device by case-insensitive name substring, use its default input config, open an input stream, convert `f32`, `i16`, and `u16` samples to float, and write a mono float WAV. If the device is stereo, average channels to mono. Stop after `--seconds`.

- [ ] **Step 6: Verify**

Run:

```bash
cargo fmt --all -- --check
cargo test -p clearline-lab
cargo check --workspace
```

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml clearline-lab docs/superpowers/plans/2026-07-09-single-device-audio-recorder.md
git commit -m "feat: add single device lab recorder"
```

### Manual test instructions

After implementation on Windows PowerShell:

```powershell
cargo run -p clearline-lab -- list
cargo run -p clearline-lab -- record --device "你的设备关键词" --seconds 12 --out artifacts/lab/raw.wav
```

Repeat the record command three times with different device keywords to collect:

```text
artifacts/lab/raw.wav
artifacts/lab/clearline.wav
artifacts/lab/reference.wav
```
