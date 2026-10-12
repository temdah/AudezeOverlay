param(
    [string]$ArtifactRoot = 'release-artifacts',
    [string]$OutputDirectory = 'release'
)

$ErrorActionPreference = 'Stop'

# Download only named artifacts from the current workflow run. Verify each
# package manifest before copying; never collect arbitrary files from dist/.
function Assert-Checksums([string]$Directory, [string]$Manifest, [string[]]$Names) {
    $seen = @{}
    foreach ($line in Get-Content -LiteralPath (Join-Path $Directory $Manifest)) {
        if ($line -notmatch '^([0-9a-fA-F]{64})  ([^/\\]+)$') { throw "Invalid checksum line in $Manifest" }
        $hash, $name = $Matches[1], $Matches[2]
        if ($name -notin $Names -or $seen.ContainsKey($name)) { throw "Unexpected or duplicate checksum: $name" }
        $actual = (Get-FileHash -LiteralPath (Join-Path $Directory $name) -Algorithm SHA256).Hash
        if ($actual -ne $hash) { throw "Checksum mismatch: $name" }
        $seen[$name] = $true
    }
    if ($seen.Count -ne $Names.Count) { throw "Missing package checksums in $Manifest" }
}

$windows = Join-Path $ArtifactRoot 'windows'
$linux = Join-Path $ArtifactRoot 'linux'
$flatpak = Join-Path $ArtifactRoot 'flatpak'
$windowsNames = @('Audeze-Overlay-Setup.exe', 'Audeze-Overlay-Portable.zip')
$linuxName = 'Audeze-Overlay-Linux-x86_64.tar.gz'
Assert-Checksums $windows 'SHA256SUMS.txt' $windowsNames
Assert-Checksums $linux "$linuxName.sha256" @($linuxName)
$sources = @($windowsNames | ForEach-Object { Join-Path $windows $_ })
$sources += Join-Path $linux $linuxName
$sources += Join-Path $flatpak 'Audeze-Overlay.flatpak'
foreach ($source in $sources) {
    if ((Get-Item -LiteralPath $source).Length -le 0) { throw "Empty package: $source" }
}

$null = New-Item -ItemType Directory -Force -Path $OutputDirectory
$checksums = foreach ($source in $sources) {
    $name = Split-Path $source -Leaf
    $destination = Join-Path $OutputDirectory $name
    Copy-Item -LiteralPath $source -Destination $destination -Force
    $hash = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $name"
}
[IO.File]::WriteAllLines((Join-Path ([IO.Path]::GetFullPath($OutputDirectory)) 'SHA256SUMS.txt'), $checksums, [Text.UTF8Encoding]::new($false))
