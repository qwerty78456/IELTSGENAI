param([string]$Connection = 'ielts-portable-builder')
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
function Check-Exit { if ($LASTEXITCODE -ne 0) { throw "Linux verification failed: $LASTEXITCODE" } }
$version = [regex]::Match((Get-Content Cargo.toml -Raw), '(?m)^version = "([^"]+)"').Groups[1].Value
$artifact = "listening-exam-generator-$version-linux-x86_64.AppImage"
foreach ($baseline in @('22.04', '24.04')) {
    Write-Output "Verifying Ubuntu $baseline"
    $container = podman --connection $Connection create "docker.io/library/ubuntu:$baseline" sleep infinity
    Check-Exit
    try {
        podman --connection $Connection start $container
        Check-Exit
        podman --connection $Connection exec $container mkdir /test
        Check-Exit
        podman --connection $Connection exec $container chmod 777 /test
        Check-Exit
        podman --connection $Connection cp "dist/$artifact" ($container + ':/test/' + $artifact)
        Check-Exit
        podman --connection $Connection cp packaging/smoke.py ($container + ':/test/smoke.py')
        Check-Exit
        podman --connection $Connection cp packaging/linux/check-clean.sh ($container + ':/test/check-clean.sh')
        Check-Exit
        podman --connection $Connection exec $container chmod a+rx "/test/$artifact"
        Check-Exit
        podman --connection $Connection exec --user 65534 $container bash /test/check-clean.sh "/test/$artifact"
        Check-Exit
        podman --connection $Connection exec $container bash -c 'apt-get update -qq && apt-get install -y --no-install-recommends python3 > /tmp/python-install.log 2>&1'
        Check-Exit
        podman --connection $Connection exec --user 65534 $container python3 /test/smoke.py "/test/$artifact"
        Check-Exit
    } finally {
        podman --connection $Connection rm -f $container
    }
}
