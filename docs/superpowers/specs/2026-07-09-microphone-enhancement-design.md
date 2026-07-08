# 麦克风增强自动模式设计

## 背景

ClearLine 当前主路径是：真实麦克风输入 -> 回音消除 -> 抗风噪 -> DeepFilterNet 降噪 -> VB-CABLE 输出。用户反馈希望在保证人声清晰的情况下把声音加大。这个需求不应通过简单乘大输入信号解决，因为那会把噪声、爆音和削波一起放大，并且会让降噪模型更容易收到失真的输入。

## 目标

新增“麦克风增强”自动模式，在降噪后的后处理阶段自动提高偏小的人声响度，同时通过峰值保护避免爆音、削波和明显电子音。

## 非目标

- 不做三档增强强度。
- 不做复杂压缩器、门限、ratio、attack、release 等专业参数 UI。
- 不修改 Windows 系统级麦克风增强设置。
- 不在降噪前放大输入。
- 不改变 VB-CABLE 输出目标和安装器行为。

## 用户体验

设备页在降噪强度和现有辅助开关附近增加一个开关：

```text
麦克风增强：开启 / 关闭
```

默认开启。开关应和当前“抗风噪增强”“回音消除”一样可以运行中实时切换；切换时复用现有管线重启机制，避免在音频回调里修改共享状态。

状态页“处理链路”增加一行：

```text
麦克风增强  已启用 / 未启用
```

本地设置保存开关状态到 `%APPDATA%\ClearLine\settings.json`，下次启动恢复。

## 音频处理设计

新增一个轻量后处理器 `AutoGainProcessor`，放在 `clearline-core`，处理 interleaved `f32` 音频。它在 `process_input_callback_frame` 中运行，顺序为：

```text
AEC -> WindNoiseReducer -> NoiseSuppressor -> AutoGainProcessor -> 输出声道转换 / VB-CABLE 缓冲
```

算法第一版保持简单、可预测：

- 统计每次回调处理后音频的 RMS 和 peak。
- 目标 RMS 使用保守值，例如 `0.18`，让普通人声更接近可用音量。
- 最大增益限制为 `4.0x`（约 +12 dB）。
- 最小增益为 `1.0x`，第一版只增强偏小音量，不主动压低正常音量。
- 当输入 RMS 很低时视为安静，不继续提高增益，避免把底噪拉起来。
- 增益变化做平滑：上升慢一点，下降快一点，避免忽大忽小和爆音。
- 输出最后经过 limiter，峰值钳制到 `0.891` 左右（约 -1 dBFS）。

## 数据模型和接口

在 `clearline-core` 增加：

```rust
pub struct AutoGainConfig {
    enabled: bool,
}

pub struct AutoGainProcessor { ... }
```

最小公开接口：

```rust
impl AutoGainConfig {
    pub fn enabled() -> Self;
    pub fn disabled() -> Self;
    pub fn is_enabled(&self) -> bool;
}

impl AutoGainProcessor {
    pub fn new(format: AudioFrameFormat, config: AutoGainConfig) -> Self;
    pub fn process_interleaved(&mut self, samples: &mut [f32]);
    pub fn is_enabled(&self) -> bool;
}
```

`AudioPipelineConfig` 增加 `microphone_boost_enabled: bool`，并提供：

```rust
pub fn with_microphone_boost(mut self, enabled: bool) -> Self;
pub fn microphone_boost_enabled(&self) -> bool;
```

`PipelineRuntimeInfo` 增加对应运行时状态，供 UI 状态页显示。

## 设置持久化

`PersistedSettings` 增加：

```rust
#[serde(default = "default_microphone_boost_enabled")]
pub microphone_boost_enabled: bool,
```

默认值为 `true`。旧设置文件缺字段时自动开启，符合“增强默认可用”的产品行为。

## 错误处理

自动增益不做 I/O，不应产生可恢复错误。若传入空 buffer，直接返回。所有计算结果必须保持有限值；遇到 NaN 或无穷值时按 0 处理。Limiter 保证输出样本始终在 `[-0.891, 0.891]`。

## 测试策略

核心单元测试：

1. 小音量正弦或类人声样本经过增强后 RMS 变大。
2. 安静 / 接近静音样本不被明显拉高。
3. 大音量样本不会超过 limiter 上限。
4. 连续两帧从小声到大声时增益会平滑下降，避免突然爆音。
5. `AudioPipelineConfig` 默认开启并能关闭。
6. `PipelineRuntimeInfo` 暴露麦克风增强状态。
7. `PersistedSettings` 缺字段时默认开启，保存和加载能 round-trip。
8. UI 文案为中文，状态页显示“麦克风增强”。

手动验收：

1. 安装新包后启动 ClearLine，确认设备页默认显示“麦克风增强：开启”。
2. 说话测试：`CABLE Output` 电平比关闭增强时更高。
3. 安静测试：不说话时底噪不应明显被拉高。
4. 大声测试：大声说话不应出现明显爆音或电子削波。
5. 运行中切换开关，确认声音链路会恢复并实时生效。
