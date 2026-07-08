# ClearLine

ClearLine 是一个 Windows-only 的通用麦克风降噪工具。

目标用户流程：选择真实麦克风设备，ClearLine 对输入做实时降噪，然后把处理后的音频写入 VB-CABLE 播放端点（`CABLE Input` 或官方新版包里的 `CABLE In 16 Ch`）。用户再把 VB-CABLE 录音端点 `CABLE Output` 设为 Windows 默认录音设备，或在 Discord、微信、QQ、浏览器会议、游戏语音等应用里选择它，使这些应用都使用降噪后的输入。

## 当前 MVP 范围

第一轮先实现可编译的项目骨架、音频管线基础抽象和最小可验证的本地音频入口：

- Rust workspace。
- `clearline-core`：设备枚举、输入/输出设备选择、降噪器 trait、直通/占位降噪器、管线状态。
- `clearline-app`：基于 `eframe/egui` 的中文桌面 UI。
- Windows 上通过 `cpal` 枚举输入和输出设备。
- Windows 上启动输入 stream、输出 stream，并通过环形缓冲做直通输出。
- 输出到音频设备时会使用输出设备自己的默认采样率 / 声道数；真实麦克风会先通过 `rubato 0.14` 重采样到输出设备采样率，再进入回音消除、抗风噪、DeepFilterNet 和输出缓冲，避免输入/输出默认格式不一致导致电子音或无声。
- VB-CABLE 主路径会强制选择播放端点 `CABLE Input` 或 `CABLE In 16 Ch`，不再因为旧设置里的系统扬声器 ID 而误输出到非 VB-CABLE 设备。
- 输出端启动时会等待约 20ms 预缓冲，避免启动瞬间误报欠载。
- 音频回调会复用临时缓冲，减少实时路径中的堆分配和延迟抖动。
- `FrameChunker` 可以把连续音频样本切成固定大小帧，并保留不足一帧的尾部样本，为 DeepFilterNet 等固定帧后端接入做准备。
- `LowLatencySuppressor` 已走固定帧处理路径：当前按约 10ms 帧切片，首帧凑满前输出短暂静音。
- `rnnoise` / `LowLatencySuppressor` 仍保留在 `clearline-core` 作为 legacy/dev feature，方便后续对比测试；默认 `clearline-app` 不再启用 RNNoise，也不再把它作为用户可选或回退路径。
- 高质量降噪的用户路径以 DeepFilterNet 模型为准：应用会自动探测随安装包放在 `models/deepfilternet` 的 `enc.onnx`、`erb_dec.onnx`、`df_dec.onnx` 和 `config.ini`；当用户选择“高质量降噪”且打包模型有效时，启动后会选择 `deepfilternet-tract-worker` 后端。
- 当 DeepFilterNet 打包模型缺失、不完整或加载失败时，ClearLine 会拒绝启动降噪并显示模型不可用，不再静默回退到 RNNoise。
- 可选 `抗风噪增强` 前处理：高通滤波、低频冲击限幅和 soft limiter。
- 显示实时输入电平、实际处理后端、降噪强度、抗风噪状态、DeepFilterNet 打包模型状态、输入/输出格式、固定帧大小、缓冲水位、估算缓冲延迟、算法帧延迟、欠载样本和溢出丢弃样本。
- 顶部常驻开启/关闭开关，主界面只保留“设备 / 状态”两个 tab；默认进入设备页。
- 用户路径收敛为“高质量降噪”：默认使用 DeepFilterNet + 回音消除；无降噪直通和低延迟 RNNoise 仅作为 core 内部/开发测试路径，不再作为 UI 模式入口。
- 状态页会显示回音消除后端、系统播放参考音频电平、参考缓冲量、参考缺帧和丢弃计数。
- UI 会显示版本号、debug/release 构建类型和 Git commit，方便区分测试 exe。
- 本地设置会保存到 `%APPDATA%\ClearLine\settings.json`：输入设备、是否启用降噪、强度、抗风噪、回音消除和开机自启动开关会在下次启动时恢复；旧版低延迟模式设置会自动迁移到高质量模式。
- 设备页提供“打开声音设置”按钮，引导用户在 Windows 或各语音软件中手动选择 `CABLE Output`；ClearLine 不再尝试自动修改默认麦克风。
- 软件打开后会自动接入当前麦克风并输出到 VB-CABLE；顶部“降噪：开启/关闭”只控制算法是否启用，关闭降噪时仍保持真实麦克风到 VB-CABLE 的直通输出。
- 只有开启降噪时，抗风噪增强和回音消除才会进入实际音频链路。
- 开机自启动和退出程序放在托盘右键菜单中；开机自启动写注册表在后台线程完成，避免卡住 UI。
- 关闭窗口不会退出程序；窗口会隐藏到系统托盘，双击托盘图标可以恢复，右键托盘图标可用勾选菜单快速切换降噪、开机自启动、抗风噪、回音消除或退出程序。
- 运行中切换真实麦克风、降噪开关、降噪强度、抗风噪、回音消除会自动重启音频管线并实时生效。
- 非 Windows 主机只保证开发期可编译；设备枚举返回空列表。

当前不实现：

- 自研 Windows 虚拟音频驱动。
- DeepFilterNet 后台推理队列容量、迟到帧补偿策略和状态页诊断阈值调优。
- 所有输出设备后端的高级独占模式 / 低延迟格式协商；当前默认使用输出设备默认共享格式并做输入重采样。
- 安装时自动修改 Windows 默认录音/播放设备。
- 账号系统、云端处理、多麦克风混音、直播声卡功能。

## 项目结构

```text
.
├── Cargo.toml
├── clearline-core/
│   ├── Cargo.toml
│   └── src/
│       ├── device.rs
│       ├── frame.rs
│       ├── lib.rs
│       ├── pipeline.rs
│       ├── preprocess.rs
│       └── suppressor.rs
├── clearline-app/
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       └── settings.rs
└── docs/
    ├── mvp.md
    └── superpowers/
```


### AEC 回音消除离线验证

AEC 接入先使用 Microsoft AEC-Challenge 官方 synthetic 音频做离线分析，音频下载到 `.dev/aec-fixtures`，不会提交到仓库。

```bash
python3 scripts/download-aec-fixtures.py --fileid 0 --fileid 1 --fileid 10
cargo run -p clearline-core --features aec --example analyze_synthetic_aec
cargo run -p clearline-core --features aec --example analyze_aec_challenge_fixture -- 0 1 10
cargo test -p clearline-core --features aec --test aec_challenge_fixture -- --ignored --nocapture
```

当前验收阈值：内置 synthetic analyzer 要输出 `PASS`；官方 AEC-Challenge fixture 中，AEC 输出相对 nearend target 的残差至少改善 1 dB，且输出与 farend 的相关性低于输入。通过这些离线验证后，再进入 ClearLine 实时管线集成。

### AEC 回音消除实时探针

在 Windows 上播放任意系统音频，同时保持默认麦克风可用，然后运行：

```powershell
cargo run -p clearline-core --features aec --example probe_realtime_aec
```

预期输出：10 秒内持续打印 `processed_frames`，`reference_level` 在系统音频播放时应高于 0，最后打印 `ClearLine realtime AEC probe OK`。这个命令用于验证“默认播放设备 loopback 参考音频 + 默认麦克风输入 + AEC3 worker”能在线跑通，不用于听感验收。

验证 VB-CABLE 自身是否能从播放端点传到录音端点：

```powershell
cargo run -p clearline-core --example inject_vb_cable_sine
```

预期结果：20 秒内 Windows 录音设备里的 `CABLE Output` 有电平波动，监听 `CABLE Output` 能听到 440 Hz 测试音。这个命令绕过 ClearLine 主程序，只验证 VB-CABLE 驱动和端点连通性。

验证真实 `AudioPipeline -> VB-CABLE` 路径：

```powershell
cargo run -p clearline-core --features aec --example probe_pipeline_aec
```

预期输出：启动行里应显示 `output="CABLE Input"` 或 `output="CABLE In 16 Ch ..."`，`backend=Aec3`，`input_level` 说话时应高于 0，`reference_level` 在系统音频播放时高于 0，最后打印 `ClearLine AudioPipeline AEC probe OK`。这个命令会启动生产音频管线并写入 VB-CABLE 播放端点，用于确认主程序会走同一条 AEC API 路径。

## 开发命令

格式化：

```bash
cargo fmt
```

检查整个 workspace：

```bash
cargo check
```

运行 core 测试：

```bash
cargo test -p clearline-core
```

运行 legacy RNNoise core 对比测试（默认 App 不启用）：

```bash
cargo test -p clearline-core --features rnnoise
```

运行桌面 UI：

```bash
cargo run -p clearline-app
```

构建 Windows release exe：

```bash
cargo build -p clearline-app --release
```

如果在 WSL 中调用 Windows Rust 工具链，本机可用命令是：

```bash
/mnt/c/Users/DHX/.cargo/bin/cargo.exe build -p clearline-app --release
```

默认产物：

```text
target/release/clearline-app.exe
```

设置文件：

```text
%APPDATA%\ClearLine\settings.json
```

删除该文件可恢复默认选择。

设置录音设备：在 ClearLine 的设备页点击“打开声音设置”后手动把 `CABLE Output` 设为 Windows 默认录音设备，或在 Discord、微信、QQ、浏览器会议、游戏语音等软件中直接选择 `CABLE Output`。

当前也可以复制为更面向用户的文件名：

```text
dist/ClearLine.exe
```

### 自包含安装器

ClearLine 的安装器项目位于 `clearline-setup/` 和 `clearline-installer/`。当前主路径是 Rust 原生自包含安装器：生成的 `ClearLineSetup.exe` 已内嵌主程序、DeepFilterNet 模型、官方基础版 VB-Audio VB-CABLE zip 包和原生 `clearline-installer-helper.exe`。用户双击这一个 exe 即可安装，不需要安装第三方安装器、PowerShell 模块或 ClearLine 自研驱动测试签名环境。

普通用户安装体验：

- 双击 `ClearLineSetup.exe` 会显示类似 MSI 的原生安装向导，用户可以使用默认路径或选择其他安装路径。
- 安装界面会询问是否启用开机自启动；启用后 ClearLine 登录时以 `--minimized` 方式启动并进入托盘。
- 安装器会触发 UAC，因为需要安装/绑定 VB-CABLE 驱动。
- 安装完成后，开始菜单会有 `ClearLine` 和 `卸载 ClearLine` 两个入口。
- 安装目录的 `installer\ClearLineUninstall.exe` 是可双击卸载程序；Windows “应用和功能”也会显示 ClearLine 的卸载入口。

当前发布后端是 `vb-cable`：

- ClearLine uses VB-Audio VB-CABLE as the virtual audio device.
- VB-CABLE source: <https://www.vb-cable.com> / <https://vb-audio.com/Cable/>
- VB-CABLE is donationware and users may support/license it through VB-Audio.
- VB-CABLE is a third-party package and is not covered by the ClearLine source license; the development zip is kept local and ignored by Git.
- ClearLine 输出到 VB-CABLE 播放端点。官方基础版 VB-CABLE 在不同版本中可能显示为 `CABLE Input` 或 `CABLE In 16 Ch`。
- Discord、微信、QQ、浏览器会议、游戏语音等用户软件请选择录音设备 `CABLE Output`。
- 安装器会在安装 VB-CABLE 前保存当前默认播放/录音设备，安装后恢复它们，避免 Windows 自动把默认播放或录音设备切到 VB-CABLE。ClearLine 不会自动设置默认麦克风；需要用户在 Windows 或语音软件中手动选择 `CABLE Output`。

说明：这里的“只创建一个设备”指只创建一个 root-enumerated VB-CABLE 驱动设备实例，硬件 ID 为 `VBAudioVACWDM`。Windows 音频栈仍会为同一个驱动实例自动显示播放端点和录音端点；这些 `AudioEndpoint` / `SWD\MMDEVAPI` 条目不是 ClearLine 额外创建的 root 驱动设备。

`clearline-driver/` 中的 ClearLine 自研虚拟音频驱动仍保留，但默认安装流程暂时不安装它；未来拿到 Microsoft 驱动签名后再恢复该后端。

先编译原生安装 helper 并验证安装器 payload 是否完整，不生成安装器：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-installer\scripts\build-installer.ps1 -SkipCompile
```

生成自包含安装器：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-installer\scripts\build-installer.ps1
```

预期输出：

```text
artifacts\installer\ClearLineSetup.exe
```

从非管理员命令行启动静默安装时，安装器会触发 UAC，并等待提升后的安装/卸载进程结束后再返回退出码，便于开发期脚本验证真实结果：

```powershell
.\artifacts\installer\ClearLineSetup.exe --quiet
```

生成完成后也可以单独验证安装器产物：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-installer\scripts\verify-installer-artifact.ps1
```

该验证会输出文件大小、SHA256、版本信息和 Authenticode 签名状态。开发构建未签名时会显示签名警告，但不会阻止本地测试。

安装和卸载过程中会写日志到：

```text
%ProgramData%\ClearLine\logs\ClearLineSetup-*.log
```

日志会记录安装步骤、注册表操作、VB-CABLE zip 解压、`pnputil` 输出、单个 VB-CABLE root devnode 创建/复用/绑定结果、驱动 helper 的 stdout/stderr，以及安装后 `CABLE Input` / `CABLE In 16 Ch` 和 `CABLE Output` 检测结果。

卸载器行为：

- 从开始菜单点击“卸载 ClearLine”、双击 `installer\ClearLineUninstall.exe`、从 Windows “应用和功能”卸载或运行 `ClearLineSetup.exe --uninstall` 时，会询问是否同时卸载 VB-CABLE。
- 静默卸载默认保留 VB-CABLE：`"C:\Program Files\ClearLine\installer\ClearLineUninstall.exe" --quiet`。
- 静默卸载并移除 VB-CABLE：`"C:\Program Files\ClearLine\installer\ClearLineUninstall.exe" --quiet --remove-vb-cable`。
- 静默卸载并明确保留 VB-CABLE：`"C:\Program Files\ClearLine\installer\ClearLineUninstall.exe" --quiet --keep-vb-cable`。

安装器运行完成后，可以验证已安装状态：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-installer\scripts\verify-installed-clearline.ps1
```

如果要先跳过 VB-CABLE 端点检查，只验证安装目录和注册表：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-installer\scripts\verify-installed-clearline.ps1 -SkipDevice
```

卸载完成后可以验证清理状态：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-installer\scripts\verify-uninstalled-clearline.ps1
```

当前自包含安装器安装的是官方基础版 VB-CABLE 驱动，不需要 Windows TESTSIGNING。正式公开分发仍需要对 ClearLine 安装器和主程序做生产 Authenticode 签名；ClearLine 自研驱动后端恢复前还需要单独完成 Microsoft 驱动签名流程。

检查 Windows Authenticode 签名：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\check-authenticode.ps1 -Path .\dist\ClearLine.exe
```

本地开发构建默认未签名，可能被 Windows 11 Smart App Control 阻止运行。详见 [`docs/windows-signing.md`](docs/windows-signing.md)。

## 当前核心接口

- `DeviceEnumerator`：音频输入/输出设备枚举接口。
- `CpalDeviceEnumerator`：Windows CPAL 输入/输出设备枚举实现。
- `InputDeviceSelector`：记录并解析用户选择的输入设备。
- `OutputDeviceSelector`：记录并解析用户选择的输出设备。
- `FrameChunker`：把连续音频流切成固定大小帧。
- `WindNoiseReducer`：可选抗风噪前处理，高通滤波后再做冲击限幅和 soft limiter。
- `NoiseSuppressor`：降噪器 trait。
- `BypassSuppressor`：无降噪直通，仅用于内部测试管线和安全回退，不作为用户可选模式。
- `LowLatencySuppressor`：legacy/dev 低延迟路径，已通过 `FrameChunker` 进入固定帧处理路径；`rnnoise` feature 下会在 48kHz 输入启用 `nnnoiseless` 后端，多声道会内部 downmix/upmix，并支持 `柔和 / 标准 / 强力` 三档强度。默认 App 不启用该路径。
- `DeepFilterNetModelBundle`：解析 DeepFilterNet ONNX 模型资源目录，要求包含 `enc.onnx`、`erb_dec.onnx`、`df_dec.onnx` 和 `config.ini`。
- `HighQualitySuppressor`：高质量模式的用户路径以 DeepFilterNet 模型为准；有效模型包会构造 `deepfilternet-tract-worker` 后端，在后台推理线程中调用 ONNX 模型。
- DeepFilterNet 打包模型校验：应用自动探测 exe 旁边的 `models/deepfilternet`，不在设备页暴露手动模型目录。模型不可用时，正式管线会拒绝启动并提示模型问题，不再回退到 RNNoise。
- `SuppressorRuntimeInfo`：描述当前降噪器后端名称、固定帧大小和是否为真实降噪后端。
- `PipelineRuntimeInfo`：描述运行中的输入格式、输出格式和降噪器运行时信息。
- `PipelineState`：`Stopped` / `Starting` / `Running` / `Error`。

## 许可

ClearLine 自有源码采用 `MIT OR Apache-2.0` 双许可，详见 `LICENSE-MIT` 和 `LICENSE-APACHE`。第三方源码、模型和二进制 payload 仍遵循各自上游许可，详见 `NOTICE.md`。

公开源码仓库不跟踪 VB-Audio VB-CABLE zip 包；需要构建安装器时，请按 `third_party/vb-cable/README.md` 将官方 zip 放到本地对应路径。

## 下一步建议

1. 根据高质量模式实测结果继续调整后台推理队列容量、迟到帧补偿策略和状态页诊断阈值。
2. 完善安装包脚本，把 DeepFilterNet 模型资源和 exe 一起发布到 `models/deepfilternet`。
3. 实测抗风噪增强的听感，并根据强风噪/喷麦样本微调高通截止频率、冲击阈值和 limiter 曲线。
4. 根据延迟与缓冲诊断的实测结果，调整缓冲容量、预缓冲和高质量模式帧策略。
5. 补充 44.1kHz 真实设备测试；当前没有对应设备，先保留为后续兼容性验证项。
6. 补充安装后首次运行引导，帮助普通用户理解如何在 Windows 或语音软件中选择 `CABLE Output`。
