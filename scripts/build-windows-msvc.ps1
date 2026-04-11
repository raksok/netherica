param(
    [string]$OutputDir = "dist/windows-msvc"
)

$ErrorActionPreference = "Stop"

Write-Host "Building Netherica (Windows MSVC static CRT)..."

$target = "x86_64-pc-windows-msvc"
$binaryName = "netherica.exe"

New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null

$env:RUSTFLAGS = "-C target-feature=+crt-static"
cargo build --locked --release --target $target

$sourceBinary = Join-Path "target/$target/release" $binaryName
$destBinary = Join-Path $OutputDir $binaryName
Copy-Item $sourceBinary $destBinary -Force

$hash = Get-FileHash -Path $destBinary -Algorithm SHA256
$hashLine = "{0}  {1}" -f $hash.Hash.ToLowerInvariant(), $binaryName
Set-Content -Path (Join-Path $OutputDir "SHA256SUMS.txt") -Value $hashLine -Encoding ascii

Write-Host "Done -> $destBinary"
