param([switch]$SkipAppBuild)
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
function Check-Exit { if ($LASTEXITCODE -ne 0) { throw "Build command failed: $LASTEXITCODE" } }
if ((rustc --version) -notmatch '^rustc 1\.92\.0 ') { throw 'Rust 1.92.0 is required' }
if ((dx --version) -notmatch '0\.7\.9') { throw 'Dioxus CLI 0.7.9 is required' }
$version = [regex]::Match((Get-Content Cargo.toml -Raw), '(?m)^version = "([^"]+)"').Groups[1].Value
New-Item -ItemType Directory -Force dist,target/portable | Out-Null
$savedFlags = $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS
try {
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = '-C target-feature=+crt-static'
    if (-not $SkipAppBuild) {
        $webOutput = Join-Path (Get-Location).Path 'target\dx\vmq_mvp\release\web'
        if (Test-Path -LiteralPath $webOutput) {
            $resolved = (Resolve-Path -LiteralPath $webOutput).Path
            if ($resolved -ne $webOutput) { throw "Unexpected build output path: $resolved" }
            Remove-Item -LiteralPath $resolved -Recurse -Force
        }
        dx build --release --web --windows-subsystem CONSOLE --cargo-args=--locked
        Check-Exit
    }
    $payload = Join-Path (Get-Location) 'target/portable/windows-payload.zip'
    $bundle = 'target/dx/vmq_mvp/release/web'
    python packaging/notices.py "$bundle/RUST-DEPENDENCIES.txt" Cargo.toml tools/portable-launcher/Cargo.toml
    Check-Exit
    Compress-Archive -Path "$bundle/server.exe","$bundle/public","$bundle/RUST-DEPENDENCIES.txt" -DestinationPath $payload -Force
    $env:IELTS_PORTABLE_PAYLOAD = $payload
    cargo build --locked --release --target x86_64-pc-windows-msvc --manifest-path tools/portable-launcher/Cargo.toml
    Check-Exit
    Copy-Item tools/portable-launcher/target/x86_64-pc-windows-msvc/release/listening-exam-generator.exe "dist/listening-exam-generator-$version-windows-x64.exe"
    Copy-Item packaging/PORTABLE-README.txt dist/README.txt
    $file = Get-Item "dist/listening-exam-generator-$version-windows-x64.exe"
    $checksum = "$((Get-FileHash $file -Algorithm SHA256).Hash.ToLower())  $($file.Name)`n"
    [IO.File]::WriteAllText((Join-Path (Get-Location).Path 'dist/SHA256SUMS-windows.txt'), $checksum, [Text.UTF8Encoding]::new($false))
} finally {
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = $savedFlags
    Remove-Item Env:IELTS_PORTABLE_PAYLOAD -ErrorAction SilentlyContinue
}
