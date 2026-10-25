# Releasing

Push a `v*` tag: `.github/workflows/release.yml` builds the macOS app, DMG and
CLI, the Windows installer and CLI, uploads them to the release and updates
the auto-update feed.

## Bundled ffmpeg

Both apps ship a static ffmpeg as a Tauri sidecar, next to the main binary,
where `find_tool` looks first. `scripts/fetch-ffmpeg.sh` (macOS arm64) and
`scripts/fetch-ffmpeg.ps1` (Windows x64) download pinned builds, check their
SHA-256 and write them to `app/src-tauri/binaries/`; the
`tauri.ffmpeg*.conf.json` overlays add them to the bundle with ffmpeg's GPL
license and a notice saying where its source is. To update ffmpeg, change the
URL, checksum and version together in both scripts.

Local builds don't need any of this: without the overlay the app uses the
ffmpeg installed on the machine. To try the bundled one:

```sh
scripts/fetch-ffmpeg.sh
cd app/src-tauri && cargo tauri build --bundles app --config tauri.ffmpeg.conf.json
```

## Code signing

Signing turns on by itself once these repository secrets exist
(Settings → Secrets and variables → Actions). Without them the macOS app is
signed ad hoc and the Windows installer is unsigned, as before.

### macOS (Developer ID + notarisation)

| Secret | Value |
|---|---|
| `APPLE_CERTIFICATE` | The *Developer ID Application* certificate exported from Keychain Access as `.p12`, in base64: `base64 -i cert.p12 \| pbcopy` |
| `APPLE_CERTIFICATE_PASSWORD` | The password chosen when exporting it |
| `APPLE_ID` | Apple account e-mail, for notarisation |
| `APPLE_PASSWORD` | An [app-specific password](https://support.apple.com/102654) for it |
| `APPLE_TEAM_ID` | The Team ID from [developer.apple.com/account](https://developer.apple.com/account) |

With the first two only, the app, its ffmpeg, the DMG and the CLI are signed;
the three `APPLE_ID…` secrets add notarisation (app and DMG are stapled). The
hardened runtime needs `Entitlements.plist` for Apple Events (Finder selection).

### Windows (Authenticode)

| Secret | Value |
|---|---|
| `WINDOWS_CERTIFICATE` | The code signing certificate as `.pfx`, in base64: `certutil -encode cert.pfx cert.txt` (keep only the base64 lines) |
| `WINDOWS_CERTIFICATE_PASSWORD` | Its password |

Tauri then signs `clementine-app.exe`, the ffmpeg sidecar and the installer,
timestamped by DigiCert. An OV certificate still shows SmartScreen warnings
until the app builds up a reputation; EV or Azure Trusted Signing avoid that.

Once signing is on, remove the "isn't notarized / isn't signed" notes from the
README.
