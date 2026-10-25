# Download the static ffmpeg build bundled with the Windows app into
# app/src-tauri/binaries/, where `tauri.ffmpeg.windows.conf.json` picks it up
# as a sidecar. Pinned and checksummed; bump both together.
$ErrorActionPreference = "Stop"

$Url = "https://www.gyan.dev/ffmpeg/builds/packages/ffmpeg-9.0.2-essentials_build.zip"
$Sha256 = "60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba"
$Version = "9.0.2"

$root = Split-Path -Parent $PSScriptRoot
$dest = Join-Path $root "app/src-tauri/binaries"
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("ffmpeg-" + [Guid]::NewGuid())
New-Item -ItemType Directory -Force $tmp, $dest | Out-Null
try {
  $zip = Join-Path $tmp "ffmpeg.zip"
  curl.exe -fsSL -o $zip $Url
  $hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
  if ($hash -ne $Sha256) { throw "ffmpeg checksum mismatch: $hash" }
  Expand-Archive $zip -DestinationPath $tmp
  $dir = Get-ChildItem $tmp -Directory | Select-Object -First 1
  Copy-Item (Join-Path $dir.FullName "bin/ffmpeg.exe") (Join-Path $dest "ffmpeg-x86_64-pc-windows-msvc.exe")
  Copy-Item (Join-Path $dir.FullName "LICENSE") (Join-Path $dest "FFMPEG-LICENSE.txt")
  @"
Clémentine bundles ffmpeg $Version (https://ffmpeg.org), a separate program
licensed under the GNU GPL version 3 (FFMPEG-LICENSE.txt). This build comes
from $Url (https://www.gyan.dev/ffmpeg/builds/). Its source code is available
at https://ffmpeg.org/releases/ffmpeg-$Version.tar.xz
"@ | Out-File -Encoding utf8 (Join-Path $dest "FFMPEG-NOTICE.txt")
} finally {
  Remove-Item -Recurse -Force $tmp
}
