# Regenerates the images in docs/images from the live UI.
# Needs the dev server running (npm run dev) and Microsoft Edge.
# Edge runs headless with a throwaway profile; your browser isn't touched.
param([string]$Server = "http://127.0.0.1:1420")

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$out = Join-Path $root "docs\images"
New-Item -ItemType Directory -Force $out | Out-Null
$edge = @("${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe", "$env:ProgramFiles\Microsoft\Edge\Application\msedge.exe") |
  Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $edge) { throw "Microsoft Edge not found." }
$profileDir = Join-Path $env:TEMP "clear-shots-profile"

$shots = @(
  @{ Name = "banner.png";               Url = "/docs/media-src/banner.html";                      W = 1280; H = 640; Scale = 2 },
  @{ Name = "social-preview.png";       Url = "/docs/media-src/banner.html";                      W = 1280; H = 640; Scale = 1 },
  @{ Name = "popup-menu.png";           Url = "/popup.html?demo=menu&theme=light&bg=light";       W = 520;  H = 400; Scale = 2 },
  @{ Name = "popup-fix.png";            Url = "/popup.html?demo=fix&theme=light&bg=light";        W = 520;  H = 400; Scale = 2 },
  @{ Name = "popup-translate-dark.png"; Url = "/popup.html?demo=rtl&theme=dark&bg=dark";          W = 520;  H = 400; Scale = 2 },
  @{ Name = "settings-shortcuts.png";   Url = "/settings.html?section=shortcuts&theme=light";     W = 880;  H = 600; Scale = 2 },
  @{ Name = "settings-provider-dark.png"; Url = "/settings.html?section=provider&theme=dark";     W = 880;  H = 600; Scale = 2 }
)

foreach ($s in $shots) {
  $file = Join-Path $out $s.Name
  & $edge --headless=new --disable-gpu --hide-scrollbars --no-first-run --user-data-dir="$profileDir" `
    --force-device-scale-factor=$($s.Scale) --window-size="$($s.W),$($s.H)" --virtual-time-budget=8000 `
    --screenshot="$file" "$Server$($s.Url)" 2>$null | Out-Null
  Start-Sleep -Milliseconds 300
  if (Test-Path $file) { "{0,-28} {1,7:N0} KB" -f $s.Name, ((Get-Item $file).Length / 1KB) } else { "FAILED: $($s.Name)" }
}
