# ClearLine Virtual Audio Driver

ClearLine 的自研虚拟音频设备从 Microsoft SYSVAD 开始做，不再把 VB-CABLE 作为长期方案。Rust 主程序继续负责真实麦克风采集、DeepFilterNet 降噪、回音消除和实时调度；内核态驱动负责向 Windows 暴露 `ClearLine Virtual Microphone` 录音设备。RNNoise 仅保留为 core legacy/dev 对比路径，不作为默认 App 用户路径。

## 当前驱动基线

当前仓库已引入 SYSVAD 和 WIL 的最小第三方源码：

- `clearline-driver/third_party/windows-driver-samples/audio/sysvad`
- `clearline-driver/third_party/windows-driver-samples/wil`

ClearLine 自有入口放在：

- `clearline-driver/ClearLineVirtualAudio/ClearLineVirtualAudio.vcxproj`
- `clearline-driver/ClearLineVirtualAudio/ClearLineVirtualAudio.inf`

`ClearLineVirtualAudio.inf` 使用 root-enumerated 硬件 ID：

```text
Root\ClearLineVirtualAudio
```

设备显示名称目标：

```text
ClearLine Virtual Microphone
```

当前 `.sys` 仍沿用 SYSVAD 的 `TabletAudioSample.sys` 构建产物。当前已把 SYSVAD 静态端点裁剪为单个 capture endpoint：`ClearLine Virtual Microphone`。下一阶段会把源码从第三方目录复制到 ClearLine 自有目录后再重命名二进制，并加入用户态 PCM 注入通道。

## 本机环境检查

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-driver\scripts\check-driver-env.ps1
```

需要：

- Visual Studio 2022 C++ build tools
- Windows SDK
- Windows Driver Kit 与 Visual Studio DriverKit MSBuild 集成
- `inf2cat.exe`
- `signtool.exe`
- 开发安装时开启 TESTSIGNING


## 安装 WDK / DriverKit 构建组件

如果环境检查显示缺少 `WDK Visual Studio DriverKit MSBuild integration`，运行：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-driver\scripts\install-wdk-components.ps1
```

脚本会请求管理员权限，安装或修复 `Microsoft.WindowsWDK.10.0.26100`，并用 `clearline-driver\wdk-desktop.vsconfig` 给 Visual Studio 2022 添加 `Component.Microsoft.Windows.DriverKit`。

单独检查 VS DriverKit 组件：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-driver\scripts\check-vs-driverkit.ps1
```

安装后快速确认：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-driver\scripts\verify-build-ready.ps1
```

如果输出 `READY`，就可以继续构建驱动包。

## 构建驱动包

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-driver\scripts\build-driver.ps1 -Platform x64
```

`build-driver.ps1` 默认构建 `Release`。如果需要内核调试，才手动传入 `-Configuration Debug`。不要把 Debug 驱动包安装到日常测试机上，因为 SYSVAD 的调试日志路径可能包含 `int 3` 断点；没有连接内核调试器时会直接变成蓝屏。

输出目录：

```text
clearline-driver\artifacts\package
```

包内应包含：

- `ClearLineVirtualAudio.inf`
- `TabletAudioSample.sys`
- `KeywordDetectorContosoAdapter.dll`，如果对应项目构建成功
- `*.cat`，当 `inf2cat.exe` 可用时生成

构建后验证驱动包：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-driver\scripts\verify-driver-package.ps1
```


## 一键准备测试机

管理员 PowerShell：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-driver\scripts\prepare-test-machine.ps1
```

脚本逻辑：

1. 如果 TESTSIGNING 未开启，自动执行 `bcdedit /set TESTSIGNING ON` 并提示重启。
2. 重启后再次运行，或执行 `-Action install`，脚本会把测试证书导入 LocalMachine、签名 catalog、安装驱动并检查设备。
3. 只查看状态可执行：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-driver\scripts\prepare-test-machine.ps1 -Action status
```

## 开启测试签名模式

安装本地测试签名内核驱动前，需要开启 Windows TESTSIGNING 并重启：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-driver\scripts\enable-testsigning.ps1 -Action enable
```

查看当前状态：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-driver\scripts\enable-testsigning.ps1 -Action status
```

测试结束后可关闭：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-driver\scripts\enable-testsigning.ps1 -Action disable
```

## 测试签名

开发机需要管理员 PowerShell：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-driver\scripts\sign-driver.ps1
```

如果只需要验证包签名链路、暂不安装驱动，可以使用当前用户证书签名：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-driver\scripts\sign-driver.ps1 -CurrentUserOnly
```

管理员模式下，该脚本创建或复用 `CN=ClearLine Driver Test Certificate`，导入 `LocalMachine\Root` 和 `LocalMachine\TrustedPublisher`，然后签名包内 `.cat`。

## 安装

管理员 PowerShell：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-driver\scripts\install-driver.ps1
```

脚本会：

1. 将 `ClearLineVirtualAudio.inf` 加入 Driver Store。
2. 使用 SetupAPI 创建 `Root\ClearLineVirtualAudio` root-enumerated 设备。
3. 调用 `UpdateDriverForPlugAndPlayDevices` 绑定驱动。
4. 扫描设备并输出 ClearLine 设备状态。

如果安装过程中出现 `0x0000001e` 蓝屏，并且 minidump 指向 `tabletaudiosample.sys` / `STATUS_BREAKPOINT`，先不要继续重复安装。当前已把 `CAdapterCommon::UpdatePowerRelations` 中“无 power relations PDO”的非错误分支从 `D_ERROR` 降级为 `D_TERSE`，并把默认构建改为 Release；重新构建、签名、验证包后再测试安装。

## 检查设备

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-driver\scripts\check-device.ps1
```

预期能看到 `ClearLine Virtual Microphone` 或 `ROOT\CLEARLINEVIRTUALAUDIO` 相关设备。

裁剪后的预期结果：

- Windows 录音设备中只出现一个 `ClearLine Virtual Microphone`。
- Windows 播放设备中不应再出现 ClearLine 输出设备。
- `cargo run -p clearline-core --example list_devices` 的 input devices 中应出现一个 ClearLine 输入设备，output devices 中不应出现 ClearLine 输出设备。

## App Control / CodeIntegrity Code 52 处理

如果设备安装后显示 `ProblemCode = 52`，并且 `check-device.ps1` 的 CodeIntegrity 事件包含：

```text
file hash could not be found on the system
0xC0000428
```

说明驱动包已经进 Driver Store，但当前 Windows App Control / WDAC 策略仍然拦截测试签名驱动。先不要继续反复重签名；应先检查并允许当前测试驱动 hash/page-hash。

生成 ClearLine 测试驱动的 supplemental allow policy：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-driver\scripts\allow-appcontrol-driver-policy.ps1 -Action generate
```

管理员 PowerShell 安装该 allow policy：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-driver\scripts\allow-appcontrol-driver-policy.ps1 -Action install
```

然后重新运行安装检查：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-driver\scripts\prepare-test-machine.ps1 -Action install
cargo run -p clearline-core --example list_devices
```

如果仍然是 Code 52，重启 Windows 后再运行同样的安装检查。移除测试 allow policy：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-driver\scripts\allow-appcontrol-driver-policy.ps1 -Action remove
```

## 卸载

管理员 PowerShell：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-driver\scripts\uninstall-driver.ps1
```


## 当前常见构建错误

如果 `build-driver.ps1 -SkipEnvironmentCheck` 出现：

```text
error MSB8020: The build tools for WindowsKernelModeDriver10.0 cannot be found
error MSB8020: The build tools for WindowsApplicationForDrivers10.0 cannot be found
```

说明 WDK 主体可能已经安装，但 Visual Studio 还缺 DriverKit 平台工具集。运行 `install-wdk-components.ps1`，在 UAC 中确认管理员权限，然后重新打开终端再执行 `check-driver-env.ps1`。

## 后续任务

1. 把 SYSVAD 源码从 third_party 复制到 ClearLine-owned driver 目录，正式重命名 driver service、binary、INF、resource 和 endpoint 字符串。
2. 增加用户态到驱动的数据通道，让 ClearLine Rust app 把降噪后的 PCM 写入驱动缓冲。
3. 在 Rust app 中增加 `ClearLine Virtual Microphone` 输出后端，替代 VB-CABLE。
4. 做 HLK / 签名发布链路。
