$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$Repo = if ($env:FABRICAUP_REPO) { $env:FABRICAUP_REPO } else { 'fabrica-eda/fabricaup' }
$FabricaHome = if ($env:FABRICAUP_HOME) { $env:FABRICAUP_HOME } else { Join-Path $HOME '.fabrica' }
$BinDir = Join-Path $FabricaHome 'bin'

if (-not [Environment]::Is64BitOperatingSystem) {
    throw '32-bit Windows is not supported'
}
$Arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
if ($Arch -ne 'X64') {
    throw "unsupported Windows architecture: $Arch"
}

$Target = 'x86_64-pc-windows-msvc'
$Asset = "fabricaup-$Target.zip"
$BaseUrl = "https://github.com/$Repo/releases/latest/download"
$TempDir = Join-Path ([IO.Path]::GetTempPath()) ("fabricaup-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $TempDir | Out-Null

try {
    $Archive = Join-Path $TempDir $Asset
    $Checksum = "$Archive.sha256"
    Write-Host "downloading fabricaup for $Target"
    Invoke-WebRequest -Uri "$BaseUrl/$Asset" -OutFile $Archive -UseBasicParsing
    Invoke-WebRequest -Uri "$BaseUrl/$Asset.sha256" -OutFile $Checksum -UseBasicParsing
    $Expected = ((Get-Content -Raw $Checksum) -split '\s+')[0].ToLowerInvariant()
    $Actual = (Get-FileHash -Algorithm SHA256 $Archive).Hash.ToLowerInvariant()
    if ($Expected -ne $Actual) { throw 'checksum mismatch' }

    Expand-Archive -LiteralPath $Archive -DestinationPath $TempDir -Force
    New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
    Copy-Item -LiteralPath (Join-Path $TempDir 'fabricaup.exe') -Destination $BinDir -Force
} finally {
    if (Test-Path -LiteralPath $TempDir) { Remove-Item -LiteralPath $TempDir -Recurse -Force }
}

if ($env:FABRICAUP_NO_MODIFY_PATH -ne '1') {
    $UserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $Entries = @($UserPath -split ';' | Where-Object { $_ })
    if ($Entries -notcontains $BinDir) {
        [Environment]::SetEnvironmentVariable('Path', (($Entries + $BinDir) -join ';'), 'User')
        Write-Host 'updated the user PATH'
    }
}

Write-Host "installed fabricaup to $BinDir"
if ($env:FABRICAUP_INIT_SKIP -ne '1') {
    & (Join-Path $BinDir 'fabricaup.exe') install
}
Write-Host 'open a new terminal to use texo and fabricaup'
