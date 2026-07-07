# Windows 默认录音设备前置封装设计

## 目标

为 ClearLine 增加“后续一键设置 Windows 默认录音设备”的前置架构，但本阶段不直接修改系统默认设备。

本阶段只做低风险能力：

- 在 app 层封装打开 Windows 声音设置的接口。
- 在 UI 中提示用户把 ClearLine 输出设备对应的虚拟音频线设为默认录音设备。
- 提供一个按钮，点击后打开 Windows 声音设置页面。
- 在文档中记录后续可通过 Windows PolicyConfig COM API 做一键设置，但不在当前阶段实现。

## 非目标

- 不直接调用未公开/不稳定 COM API 修改系统默认录音设备。
- 不要求检测当前 Windows 默认录音设备。
- 不改变现有音频管线、降噪算法和设备选择保存逻辑。
- 不引入安装器、管理员权限流程或驱动相关能力。

## 方案选择

推荐方案：**Settings Launcher + UI 提示**。

- 新增小模块，例如 `clearline-app/src/windows_settings.rs`。
- 模块提供：
  - `sound_settings_uri() -> &'static str`
  - `open_sound_settings() -> Result<(), WindowsSettingsError>`
- Windows 下使用系统 shell 打开 `ms-settings:sound`。
- 非 Windows 下返回明确错误，保持开发期可编译。
- UI 放在“输出设备”卡片下方，显示当前输出设备和说明。

备选方案 A：直接实现 PolicyConfig 一键设默认录音设备。

- 优点：体验最好。
- 缺点：接口不稳定，测试困难，容易触发权限/安全软件/兼容性问题。
- 当前不采用。

备选方案 B：只写文档，不做 UI。

- 优点：零风险。
- 缺点：用户每次都要自己找 Windows 设置入口，体验差。
- 当前不采用。

## UI 设计

在“输出设备”卡片中，设备选择框下面增加一行轻量提示：

- 标题：`Windows 默认录音设备`
- 文案：`如需让 Discord、微信、QQ、浏览器会议等应用使用降噪后的声音，请在 Windows 中把虚拟音频线的录音端设为默认录音设备。`
- 按钮：`打开声音设置`

按钮只打开 Windows 设置，不自动修改系统设置。

## 错误处理

- 打开设置成功：状态栏显示 `已打开 Windows 声音设置`。
- 打开设置失败：状态栏显示 `打开 Windows 声音设置失败：...`。
- 非 Windows：状态栏显示该功能仅 Windows 可用。

## 测试策略

自动测试覆盖：

- 声音设置 URI 固定为 `ms-settings:sound`。
- UI 提示文案包含 `默认录音设备`，防止后续误删。
- 非 Windows 下 `open_sound_settings()` 返回错误而不是 panic。

手动测试：

1. 打开 `dist/ClearLine.exe`。
2. 进入“设备”页。
3. 在输出设备区域看到“Windows 默认录音设备”提示和“打开声音设置”按钮。
4. 点击按钮，确认 Windows 设置打开到声音设置页。
5. 返回 ClearLine，确认状态栏显示已打开或失败原因。

## 后续扩展

后续如果要做一键设置默认录音设备，再单独设计：

- `DefaultRecordingDeviceManager` trait。
- Windows PolicyConfig COM API 封装。
- 当前默认录音设备检测。
- 明确的失败提示和用户确认流程。
