# Install a published AgentOS CLI binary on Windows. This does not install skills.
param(
    [string]$Prefix = $(if ($env:AGENTOS_PREFIX) { $env:AGENTOS_PREFIX } else { Join-Path $env:LOCALAPPDATA "agentos" }),
    [string]$Version = $(if ($env:AGENTOS_VERSION) { $env:AGENTOS_VERSION } else { "latest" }),
    [switch]$Uninstall
)

$ErrorActionPreference = "Stop"
$Repo = if ($env:AGENTOS_REPO) { $env:AGENTOS_REPO } else { "itsmohit124/agentos" }
$BinDir = Join-Path $Prefix "bin"
$Bin = Join-Path $BinDir "agentos.exe"

if ($Uninstall) {
    if (Test-Path $Bin) {
        Remove-Item -Force $Bin
        Write-Host "removed $Bin"
    } else {
        Write-Host "nothing to remove at $Bin"
    }
    exit 0
}

$Target = if ($env:AGENTOS_TARGET) { $env:AGENTOS_TARGET } else {
    switch ("$([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture)") {
        "X64" { "x86_64-pc-windows-msvc" }
        "Arm64" { "aarch64-pc-windows-msvc" }
        default { throw "unsupported Windows architecture" }
    }
}
$Archive = "agentos-$Target.zip"
$Sums = "SHA256SUMS"
$Base = if ($env:AGENTOS_RELEASE_BASE) { $env:AGENTOS_RELEASE_BASE.TrimEnd("/") } else {
    if ($Version -eq "latest") {
        throw "Set AGENTOS_VERSION to a published tag, or AGENTOS_RELEASE_BASE for a local test. Until a release exists, build with cargo build --release."
    }
    "https://github.com/$Repo/releases/download/$Version"
}

$Work = Join-Path ([System.IO.Path]::GetTempPath()) ("agentos-install-" + [guid]::NewGuid().ToString("n"))
New-Item -ItemType Directory -Path $Work | Out-Null
try {
    $archivePath = Join-Path $Work $Archive
    $sumsPath = Join-Path $Work $Sums
    Invoke-WebRequest -Uri "$Base/$Archive" -OutFile $archivePath -UseBasicParsing
    Invoke-WebRequest -Uri "$Base/$Sums" -OutFile $sumsPath -UseBasicParsing
    $expected = (Get-Content $sumsPath | Where-Object { $_ -match "\s$([regex]::Escape($Archive))$" } | ForEach-Object { ($_ -split "\s+")[0] } | Select-Object -First 1)
    if (-not $expected) { throw "$Archive is not listed in $Sums" }
    $actual = (Get-FileHash -Algorithm SHA256 $archivePath).Hash.ToLowerInvariant()
    if ($actual -ne $expected.ToLowerInvariant()) {
        throw "checksum mismatch for $Archive expected $expected actual $actual"
    }
    Expand-Archive -Path $archivePath -DestinationPath $Work -Force
    $payload = Join-Path $Work "agentos.exe"
    if (-not (Test-Path $payload)) { throw "archive did not contain agentos.exe" }
    New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
    Copy-Item -Force $payload $Bin
    Write-Host "installed $Bin"
    & $Bin --version
} finally {
    Remove-Item -Recurse -Force $Work -ErrorAction SilentlyContinue
}
