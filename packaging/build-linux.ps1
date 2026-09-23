param([string]$Connection = 'ielts-portable-builder')
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
function Check-Exit { if ($LASTEXITCODE -ne 0) { throw "Linux build failed: $LASTEXITCODE" } }
podman --connection $Connection build -t localhost/ielts-portable-build:0.5.0 -f packaging/linux/Containerfile packaging/linux
Check-Exit
New-Item -ItemType Directory -Force target/portable,dist | Out-Null
# Explicit inputs: never send .env, .git, data, or local secrets to the builder.
tar --exclude=__pycache__ -cf target/portable/linux-source.tar Cargo.toml Cargo.lock Dioxus.toml src assets packaging
Check-Exit
$container = podman --connection $Connection create localhost/ielts-portable-build:0.5.0 sleep infinity
Check-Exit
try {
    podman --connection $Connection start $container
    Check-Exit
    podman --connection $Connection cp target/portable/linux-source.tar ($container + ':/tmp/source.tar')
    Check-Exit
    podman --connection $Connection exec $container tar -xf /tmp/source.tar -C /build
    Check-Exit
    podman --connection $Connection exec $container bash packaging/build-linux.sh
    Check-Exit
    podman --connection $Connection cp ($container + ':/build/dist/.') dist
    Check-Exit
} finally {
    podman --connection $Connection rm -f $container
}
