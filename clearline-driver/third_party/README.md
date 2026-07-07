# Third-party driver sources

This directory vendors the minimum upstream code needed to build the first ClearLine virtual audio driver baseline.

## Microsoft Windows Driver Samples / SYSVAD

- Upstream: https://github.com/microsoft/Windows-driver-samples
- Imported commit: `2ee527bfeb0aeb6be11f0a8b6dce4011b358ce89`
- Imported path: `audio/sysvad`
- Purpose: WDM/WaveRT virtual audio driver baseline exposing render and capture endpoints.
- License: Microsoft Public License (MS-PL), copied to `windows-driver-samples/LICENSE`.

## Windows Implementation Library

- Upstream: https://github.com/microsoft/wil
- Imported commit: `3c00e7f1d8cf9930bbb8e5be3ef0df65c84e8928`
- Purpose: Required by the SYSVAD sample projects.
- License: MIT, preserved in `windows-driver-samples/wil/LICENSE`.

ClearLine-specific driver work should happen under `clearline-driver/ClearLineVirtualAudio` and scripts under `clearline-driver/scripts`; avoid editing vendored sources unless the change is copied into a ClearLine-owned fork in a later step.
