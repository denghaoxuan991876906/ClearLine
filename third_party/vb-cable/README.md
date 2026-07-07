# VB-Audio VB-CABLE payload

This directory is the local build payload location for the official basic VB-Audio VB-CABLE zip package used by the ClearLine development installer.

- Source: https://www.vb-cable.com / https://vb-audio.com/Cable/
- Package URL used for development snapshots: https://download.vb-audio.com/Download_CABLE/VBCABLE_Driver_Pack45.zip
- Expected package SHA256 for the current development snapshot: b950e39f01af1d04ea623c8f6d8eb9b6ea5c477c637295fabf20631c85116bfb

`VBCABLE_Driver_Pack45.zip` is a third-party binary package. It is intentionally ignored by Git and is not covered by the ClearLine source license.

To build the self-contained installer locally, download the official basic VB-CABLE package from VB-Audio and place it here as:

```text
third_party/vb-cable/VBCABLE_Driver_Pack45.zip
```

ClearLine extracts the official package during install without modifying the `.sys`, `.inf`, `.cat`, or setup executable files.

VB-CABLE is donationware. Users may support/license it through VB-Audio.
