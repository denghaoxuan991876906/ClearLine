# ClearLine

ClearLine 是一款面向 Windows 的实时麦克风降噪工具。它在本机处理麦克风声音，并将降噪后的音频提供给微信、QQ、Discord、浏览器会议、游戏语音等应用。

音频不会上传到云端。ClearLine 使用 DeepFilterNet 进行本地降噪，并通过 VB-CABLE 向其他软件提供处理后的虚拟麦克风。

## 功能

- 实时麦克风降噪
- 柔和、标准、强力三档降噪强度
- 回音消除
- 抗风噪增强
- 麦克风音量增强
- 实时输入电平和运行状态
- 系统托盘运行及快捷开关
- 可选开机自动启动
- 应用内检查和安装更新
- 自动适配常见麦克风采样率

## 系统要求

- Windows 10 或 Windows 11
- 64 位系统
- 一个可用的真实麦克风
- 安装驱动所需的管理员权限

ClearLine 当前不支持 macOS、Linux、Android 或 iOS。

## 下载与安装

1. 打开 [Releases](https://github.com/denghaoxuan991876906/ClearLine/releases) 页面。
2. 下载最新版 `ClearLineSetup.exe`。
3. 双击安装包，并在 Windows 用户账户控制窗口中选择“是”。
4. 选择安装目录和是否开机自动启动。
5. 等待 ClearLine 和 VB-CABLE 虚拟音频组件安装完成。

安装程序会尽量保留 Windows 当前的默认播放和录音设备，不会主动把系统默认麦克风改成 VB-CABLE。

> 当前开发版本可能尚未进行 Authenticode 代码签名，Windows SmartScreen 可能显示安全提示。请只从本仓库 Releases 页面下载安装包。

## 开始使用

### 1. 选择真实麦克风

打开 ClearLine，在“设备”页面选择你实际说话使用的麦克风。ClearLine 会自动开始处理声音，无需再点击启动按钮。

### 2. 调整处理效果

- **降噪强度**：一般环境建议使用“标准”；轻微底噪可使用“柔和”；键盘声、风扇声较强时可尝试“强力”。
- **抗风噪增强**：有喷麦、低频冲击或户外风噪时开启。
- **回音消除**：使用扬声器外放时建议开启；使用耳机时也可以保持开启。
- **麦克风增强**：声音偏小时开启。

### 3. 在语音软件中选择麦克风

在需要使用降噪声音的软件中，将麦克风设置为：

```text
CABLE Output
```

适用于微信、QQ、Discord、OBS、浏览器会议、游戏语音等软件。

请注意两个容易混淆的名称：

- `CABLE Input` 或 `CABLE In 16 Ch`：ClearLine 写入声音的播放端点，通常不需要用户选择。
- `CABLE Output`：其他语音软件应选择的录音端点，也就是降噪后的虚拟麦克风。

ClearLine 的“设备”页面提供“打开声音设置”按钮，可直接打开 Windows 声音设置。

## 托盘运行

关闭主窗口不会退出 ClearLine，而是将它隐藏到系统托盘。

- 双击托盘图标：重新打开主窗口
- 右键托盘图标：快速切换降噪、抗风噪、回音消除和开机自动启动
- 右键托盘图标并选择“退出 ClearLine”：完全退出程序

## 软件更新

在“状态”页面找到“软件更新”，点击“检查更新”。发现新版本后，ClearLine 会：

1. 从本仓库 GitHub Releases 下载安装包。
2. 校验安装包 SHA256，确认文件完整。
3. 在用户确认后退出 ClearLine 并启动安装程序。

更新会覆盖安装到原目录，并保留当前设置。正式发布版本还应通过 Authenticode 签名验证发布者身份。

## 卸载

可以通过以下任一方式卸载：

- Windows“设置 > 应用 > 已安装的应用”
- 开始菜单中的“卸载 ClearLine”
- 安装目录中的 `installer\ClearLineUninstall.exe`

卸载时可以选择是否同时删除 VB-CABLE。其他软件也在使用 VB-CABLE 时，建议保留它。

## 常见问题

### 语音软件中没有声音

确认语音软件选择的是 `CABLE Output`，而不是 `CABLE Input`，并检查 ClearLine 顶部是否显示降噪已开启。

### 找不到 CABLE Output

重新运行 `ClearLineSetup.exe` 修复安装，然后重启 Windows。如果仍然不存在，请查看安装日志：

```text
%ProgramData%\ClearLine\logs
```

### 声音太小

开启“麦克风增强”，同时检查真实麦克风自身的 Windows 输入音量。

### 尾音被削弱或声音不自然

将降噪强度从“强力”调整为“标准”或“柔和”。降噪越强，对持续底噪的抑制越明显，也更容易影响较轻的人声尾音。

### 使用扬声器时对方仍能听到回音

确认“回音消除”已开启，并降低扬声器音量。扬声器与麦克风距离太近、房间混响过强时，软件无法完全消除所有回音。

### 点击关闭后程序仍在运行

这是正常行为。ClearLine 会继续在系统托盘处理麦克风。需要完全退出时，请右键托盘图标并选择“退出 ClearLine”。

## 隐私

ClearLine 的麦克风处理在本机完成，不会把音频上传到服务器。应用仅在用户点击“检查更新”或下载更新时访问 GitHub Releases。

## 开源与第三方组件

ClearLine 自有源码采用 `MIT OR Apache-2.0` 双许可，详见 [LICENSE-MIT](LICENSE-MIT)、[LICENSE-APACHE](LICENSE-APACHE) 和 [NOTICE.md](NOTICE.md)。

ClearLine 使用以下第三方组件：

- [DeepFilterNet](https://github.com/Rikorose/DeepFilterNet)：本地神经网络降噪
- [VB-Audio VB-CABLE](https://vb-audio.com/Cable/)：虚拟音频设备，属于 donationware

第三方模型、源码和二进制文件遵循各自的上游许可，不属于 ClearLine 自有源码许可范围。

## 开发者入口

普通用户无需从源码构建。参与开发或排查底层问题时，可阅读：

- [安装器构建说明](clearline-installer/README.md)
- [MVP 与内部架构](docs/mvp.md)
- [Windows 签名说明](docs/windows-signing.md)
- [虚拟音频驱动说明](docs/driver.md)

Windows CI 会自动运行测试、下载并校验构建载荷、生成 NSIS 安装包和 `update.json`。推送 `v*` 标签时，CI 会将它们上传为 GitHub Release 资源。
