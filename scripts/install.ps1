# AIDrive installer for Windows (no WSL, no admin rights, no Rust or Node).
#   irm <release-url>/install.ps1 | iex
# or, from an extracted release folder: <folder>\install.cmd
# Run it in your project folder: it installs AIDrive for this Windows user,
# starts it, and connects Claude Code, Codex and OpenCode to a space for that
# folder. Running a newer release the same way updates AIDrive; data is kept.

# ---- Release location: the one setting to change when the release is hosted.
# A URL (https://.../download) or a local folder that holds the release files.
$DefaultRelease = 'https://github.com/OWNER/aidrive/releases/latest/download'
# ----

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$asset = 'aidrive-windows-x86_64.zip'
# Run from an extracted release folder, the archive next to this script wins.
$release = if ($env:AIDRIVE_RELEASE_URL) { $env:AIDRIVE_RELEASE_URL }
    elseif ($PSScriptRoot -and (Test-Path (Join-Path $PSScriptRoot $asset))) { $PSScriptRoot }
    else { $DefaultRelease }
$release = $release.TrimEnd('/', '\')
if ($release -notmatch '^https?://' -and $release -match '\.zip$') {
    $asset = Split-Path -Leaf $release
    $release = Split-Path -Parent $release
}
if ($release -match '/OWNER/') {
    throw 'This installer has no release location. Run install.cmd from the extracted AIDrive folder, or set AIDRIVE_RELEASE_URL to that folder.'
}
$bin = if ($env:AIDRIVE_INSTALL_DIR) { $env:AIDRIVE_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'AIDrive\bin' }
if (-not [Environment]::Is64BitOperatingSystem) { throw 'AIDrive needs 64-bit Windows.' }
$work = Join-Path ([IO.Path]::GetTempPath()) ("aidrive-install-" + [guid]::NewGuid())
New-Item -ItemType Directory -Force $work | Out-Null

function Get-ReleaseFile([string]$name) {
    $target = Join-Path $work $name
    if ($release -match '^https?://') {
        Invoke-WebRequest -UseBasicParsing -Uri "$release/$name" -OutFile $target
    } else {
        Copy-Item -LiteralPath (Join-Path $release $name) -Destination $target
    }
    $target
}

try {
    Write-Host "Downloading AIDrive from $release"
    $zip = Get-ReleaseFile $asset
    $sums = Get-Content (Get-ReleaseFile 'SHA256SUMS')
    $expected = ($sums | Where-Object { $_ -match "\s\*?$([regex]::Escape($asset))$" } | ForEach-Object { ($_ -split '\s+')[0] }) | Select-Object -First 1
    if (-not $expected) { throw "SHA256SUMS has no entry for $asset" }
    $actual = (Get-FileHash -Algorithm SHA256 $zip).Hash
    if ($actual -ne $expected.ToUpper()) { throw "Checksum mismatch for $asset" }

    $old = Join-Path $bin 'aidrive.exe'
    if (Test-Path $old) { & $old down | Out-Null }
    New-Item -ItemType Directory -Force $bin | Out-Null
    # An assistant may still be running the MCP server; Windows lets a running
    # program be renamed but not replaced.
    foreach ($exe in 'aidrive.exe', 'aidrive-mcp-server.exe') {
        $path = Join-Path $bin $exe
        if (Test-Path $path) {
            $aside = "$exe.old"
            if (Test-Path "$path.old") {
                try { Remove-Item -Force "$path.old" -ErrorAction Stop }
                catch { $aside = "$exe.$([guid]::NewGuid().ToString('N')).old" }
            }
            Rename-Item -Force $path $aside
        }
    }
    if (Test-Path (Join-Path $bin 'web')) { Remove-Item -Recurse -Force (Join-Path $bin 'web') }
    Expand-Archive -Force -Path $zip -DestinationPath $bin
    Write-Host "Installed to $bin"

    if (-not $env:AIDRIVE_NO_PATH) {
        $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
        if (-not (($userPath -split ';') -contains $bin)) {
            [Environment]::SetEnvironmentVariable('Path', (($userPath, $bin) | Where-Object { $_ }) -join ';', 'User')
            Write-Host "Added $bin to your PATH (new terminals see it)."
        }
    }
    $env:Path = "$bin;$env:Path"
} finally {
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}

& (Join-Path $bin 'aidrive.exe') up
# throw, not exit: under `irm | iex` exit would close the person's window.
if ($LASTEXITCODE -ne 0) { throw "aidrive up failed (exit $LASTEXITCODE); see the messages above." }
# Signs you into the web app with a one-time link; nothing to do if it fails.
& (Join-Path $bin 'aidrive.exe') open
if ($LASTEXITCODE -ne 0) { Write-Host "Open the web app later with: aidrive open" }
