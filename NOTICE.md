# ClearLine notices

ClearLine source code is licensed under either of:

- Apache License, Version 2.0 (`LICENSE-APACHE`)
- MIT license (`LICENSE-MIT`)

at your option, unless a file or directory states a different license.

## Third-party source code

### Microsoft Windows Driver Samples / SYSVAD

`clearline-driver/third_party/windows-driver-samples/audio/sysvad` contains code derived from Microsoft Windows Driver Samples at:

<https://github.com/microsoft/Windows-driver-samples>

The imported SYSVAD sample code is governed by the Microsoft Public License (MS-PL). A copy is included at `clearline-driver/third_party/windows-driver-samples/LICENSE`.

### Windows Implementation Library

`clearline-driver/third_party/windows-driver-samples/wil` contains Microsoft WIL source code from:

<https://github.com/microsoft/wil>

WIL is licensed under the MIT license. Its license and notices are preserved in:

- `clearline-driver/third_party/windows-driver-samples/wil/LICENSE`
- `clearline-driver/third_party/windows-driver-samples/wil/ThirdPartyNotices.txt`

## Third-party binary/runtime payloads

### VB-Audio VB-CABLE

ClearLine can build a development installer that embeds the official basic VB-Audio VB-CABLE package, but that package is not part of the ClearLine source license and is intentionally not tracked in Git.

Developers who need to build the installer must obtain `VBCABLE_Driver_Pack45.zip` from VB-Audio and place it at:

```text
third_party/vb-cable/VBCABLE_Driver_Pack45.zip
```

VB-CABLE source pages:

- <https://www.vb-cable.com>
- <https://vb-audio.com/Cable/>

### DeepFilterNet model files

ClearLine expects DeepFilterNet ONNX assets under `dist/models/deepfilternet` when building a packaged installer. These model files are not tracked in Git and remain governed by their upstream license/source metadata. Preserve upstream model notices when preparing release artifacts.
