# 高质量模式 RNNoise 回退设计

## 目标

修正 ClearLine 高质量模式的回退语义：高质量模式只有在有效 DeepFilterNet 模型运行时才代表高质量降噪；当 DeepFilterNet 不可用时，不再使用或展示“内置高质量”路径，而是明确回退到低延迟 RNNoise。

## 用户可见行为

- 选择 `高质量降噪` 且 DeepFilterNet 模型目录有效：运行 `DeepFilterNet` 后端。
- 选择 `高质量降噪` 但模型目录为空、无效或模型加载失败：运行 `RNNoise` 低延迟后端。
- 如果 RNNoise 因格式不兼容不可用：最终使用安全回退，并明确显示 RNNoise 不支持当前格式。
- UI 和 README 不再出现“高质量降噪（内置）”“内置高质量后端”“adaptive-quality-v1 高质量模式”作为推荐或用户理解路径。

## 非目标

- 不删除 `AdaptiveHighQualityBackend` 的全部代码；它可以暂时保留为内部实验/旧测试对象，但不再作为高质量模式的用户路径。
- 不实现模型自动下载或模型选择器。
- 不改变 RNNoise 的 48kHz 格式约束。
- 不改变运行中锁定控件的 UI 行为。

## core 设计

`create_suppressor_with_deepfilternet_bundle(SuppressorMode::HighQuality, ...)` 的逻辑改为：

1. 如果提供了 DeepFilterNet 模型 bundle 且加载成功，返回 `HighQualitySuppressor`，后端为 `deepfilternet-tract-worker`。
2. 如果未提供 bundle 或加载失败，返回 `LowLatencySuppressor::new_with_strength(...)`。
3. 因为实际返回的是 `LowLatencySuppressor`，运行时 `SuppressorRuntimeInfo.mode()` 会是 `LowLatency`，后端会是：
   - `nnnoiseless-rnnoise`：RNNoise 生效。
   - `bypass-placeholder`：RNNoise 也因格式或 feature 不可用而安全回退。

`HighQualitySuppressor::new_with_deepfilternet_bundle(...)` 可以继续在加载失败时回退到 adaptive，用于旧单元测试或直接构造路径；但正式 pipeline 入口不再依赖它做用户可见回退。

## app 设计

启动时根据用户选择和模型目录生成状态提示：

- 高质量 + 模型有效：`高质量降噪（DeepFilterNet 模型）`。
- 高质量 + 模型未配置/无效：`高质量模型不可用，已回退到 RNNoise`。
- 高质量 + RNNoise 实际不可用：状态页显示 `RNNoise 不支持当前格式，已使用安全回退`。

状态页降噪文案：

- `nnnoiseless-rnnoise` -> `低延迟降噪（RNNoise）已启用`
- `deepfilternet-tract-worker` -> `高质量降噪（DeepFilterNet 模型）已启用`
- `bypass-placeholder` + mode `LowLatency` -> `RNNoise 不支持当前格式，已使用安全回退`

## 文档

README 需要改掉高质量模式中的 `adaptive-quality-v1` 用户路径描述，改成：

- 低延迟：RNNoise。
- 高质量：DeepFilterNet 模型。
- 高质量模型不可用：回退到 RNNoise；RNNoise 也不可用时安全回退。

## 测试策略

自动测试覆盖：

- core：高质量模式未提供 bundle 时回退到 RNNoise/低延迟 suppressor。
- core：高质量模式提供无效 bundle 时也回退到 RNNoise/低延迟 suppressor。
- app：启动状态警告不再包含“内置高质量”。
- app：状态页文案区分 RNNoise、DeepFilterNet、RNNoise 不支持格式的安全回退。
- 文档或源码测试确保用户可见文案不再出现“内置高质量后端”。

手动测试：

1. 高质量模式不填模型目录启动，确认运行状态提示回退 RNNoise。
2. 高质量模式填无效模型目录启动，确认提示回退 RNNoise。
3. 高质量模式填有效模型目录启动，确认状态页显示 DeepFilterNet。
4. 低延迟模式启动，确认状态页显示 RNNoise。
