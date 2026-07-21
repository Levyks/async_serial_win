# Rebuilds the prebuilt DLLs committed under rust/prebuilt/<triple>/.
# Run this from a machine with the Rust toolchain + MSVC ARM64/x86/x64 build
# tools installed whenever native source changes. Consumers of the package
# never run this — hook/build.dart only reads the committed output.

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $MyInvocation.MyCommand.Path

$targets = @(
    "x86_64-pc-windows-msvc",
    "i686-pc-windows-msvc",
    "aarch64-pc-windows-msvc"
)

foreach ($target in $targets) {
    rustup target add $target
    if ($LASTEXITCODE -ne 0) { throw "rustup target add $target failed" }

    Push-Location $root
    try {
        cargo build --release --target $target
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed for $target" }
    } finally {
        Pop-Location
    }

    $destDir = Join-Path $root "prebuilt\$target"
    New-Item -ItemType Directory -Force -Path $destDir | Out-Null
    Copy-Item -Force (Join-Path $root "target\$target\release\async_serial_win.dll") $destDir
    Write-Host "Updated $destDir\async_serial_win.dll"
}
