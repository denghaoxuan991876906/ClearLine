# Windows 签名与 Smart App Control

ClearLine 当前是本地开发构建。如果直接运行未签名的 `dist/ClearLine.exe`，Windows 11 的 Smart App Control 可能会把它判定为不受信任并阻止运行。

## 当前确认结果

本地检查：

```powershell
Get-AuthenticodeSignature -FilePath E:\Dev\ClearLine\dist\ClearLine.exe
```

当前结果是：

```text
Status: NotSigned
```

这说明被 Smart App Control 阻止不是 Rust 代码或 UI 本身的问题，而是发布产物缺少 Windows 认可的代码签名。

## 开发期处理

用于本机开发测试时，有两个选择：

1. 在 Windows Security 中关闭 Smart App Control。
   - 路径：`Windows Security` → `App & browser control` → `Smart App Control settings`
   - 当前 Windows 版本允许在设置中开关 Smart App Control。
2. 继续保留 Smart App Control，则需要使用受信任证书签名后的构建产物。

Smart App Control 不支持给单个被拦截应用单独加白名单。

## 发布期目标

正式发布时需要：

- 使用受信任提供方签发的 RSA 代码签名证书，或使用 Microsoft Trusted Signing。
- 签名所有会被加载或执行的产物，包括：
  - `.exe`
  - `.dll`
  - installer
  - uninstaller
  - 发布脚本或临时安装二进制
- 使用时间戳服务器，避免证书过期后旧版本签名失效。
- 发布前运行签名检查脚本。

## 签名检查

PowerShell：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\check-authenticode.ps1 -Path .\dist\ClearLine.exe
```

如果输出 `Status: Valid`，说明 Authenticode 签名有效。如果输出 `NotSigned`、`UnknownError` 或其他状态，不能视为 Smart App Control 兼容发布产物。

## 后续签名命令模板

安装 Windows SDK / Visual Studio Build Tools 并获得证书后，可以使用 `signtool.exe`：

```powershell
signtool sign `
  /fd SHA256 `
  /tr http://timestamp.digicert.com `
  /td SHA256 `
  /a `
  .\dist\ClearLine.exe
```

签完后再检查：

```powershell
signtool verify /pa /v .\dist\ClearLine.exe
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\check-authenticode.ps1 -Path .\dist\ClearLine.exe
```

## 当前不做的事

- 不把自签名证书当作正式解决方案。
- 不把关闭 Smart App Control 当作面向用户的发布方案。
- 不在第一阶段做 MSIX / Store 分发。
