param(
    [Parameter(Mandatory)][string]$Repository,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{40}$')][string]$CommitSha,
    [Parameter(Mandatory)][ValidateSet('branch', 'tag')][string]$RefType,
    [Parameter(Mandatory)][string]$RefName,
    [string]$ReleaseNotesPath,
    [string]$ArtifactDirectory = 'release'
)

$ErrorActionPreference = 'Stop'

function Invoke-GitHub {
    param([Parameter(ValueFromRemainingArguments)][string[]]$Arguments)
    $output = & gh @Arguments
    if ($LASTEXITCODE -ne 0) { throw "GitHub command failed: $($Arguments[0..1] -join ' ')" }
    return $output
}

function Test-CurrentMain {
    $head = Invoke-GitHub api "repos/$Repository/git/ref/heads/main" | ConvertFrom-Json
    return $head.object.sha -eq $CommitSha
}

function Assert-ReleaseAssets {
    param($Release)
    $actual = @($Release.assets | ForEach-Object name)
    if ($actual.Count -ne $assetNames.Count -or @(Compare-Object $assetNames $actual).Count) {
        throw 'Release must contain exactly the Windows installer, portable ZIP, Linux archive, Flatpak and checksums'
    }
    if (@($Release.assets | Where-Object { $_.size -le 0 }).Count) {
        throw 'Release contains an empty download'
    }
    if (!$Release.body.Contains("Built from commit $CommitSha.") -or !$Release.body.Contains("## What's new")) {
        throw 'Release notes do not identify this build and its changes'
    }
}

$mainBuild = $RefType -eq 'branch'
if ($mainBuild -and $RefName -ne 'main') { throw 'Only main and version tags publish releases' }
if ($mainBuild -and !(Test-CurrentMain)) {
    Write-Host 'A newer main commit exists; skipping this superseded build.'
    return
}
$versionMatch = Select-String -LiteralPath "$PSScriptRoot/../Cargo.toml" -Pattern '^version = "([^"]+)"' | Select-Object -First 1
$version = $versionMatch.Matches[0].Groups[1].Value
if (!$mainBuild -and $RefName -ne "v$version") { throw 'Release tag must match Cargo.toml' }
$tag = "v$version"
$title = "Audeze Overlay $version"
$prerelease = $tag.Contains('-')
$assetNames = @('Audeze-Overlay-Setup.exe', 'Audeze-Overlay-Portable.zip', 'Audeze-Overlay-Linux-x86_64.tar.gz', 'Audeze-Overlay.flatpak', 'SHA256SUMS.txt')
$files = @($assetNames | ForEach-Object { (Get-Item -LiteralPath (Join-Path $ArtifactDirectory $_)).FullName })

# Read all pages so rerunning an older tag cannot accidentally create a duplicate.
$pages = Invoke-GitHub api --paginate --slurp "repos/$Repository/releases?per_page=100" | ConvertFrom-Json
$releases = @($pages | ForEach-Object { $_ })
$existing = $releases | Where-Object tag_name -eq $tag | Select-Object -First 1
if ($existing -and !$existing.draft) {
    $commit = Invoke-GitHub api "repos/$Repository/commits/$tag" | ConvertFrom-Json
    if ($commit.sha -ne $CommitSha) {
        if (!$mainBuild) { throw 'Published tag points to a different commit' }
        Write-Host "Version $version is already released. Increment Cargo.toml and add release notes for the next release."
        return
    }
    Assert-ReleaseAssets $existing
    Write-Host "Release $tag is already published; preserving its downloads and release notes."
    return
}

# A manually created tag must identify the tested commit too. Draft targets can
# be updated, but changing target_commitish does not move an existing Git tag.
$tagRefs = Invoke-GitHub api "repos/$Repository/git/matching-refs/tags/$tag" | ConvertFrom-Json
if (@($tagRefs | Where-Object ref -eq "refs/tags/$tag").Count) {
    $tagCommit = Invoke-GitHub api "repos/$Repository/commits/$tag" | ConvertFrom-Json
    if ($tagCommit.sha -ne $CommitSha) { throw 'Release tag already points to a different commit' }
}

# Release highlights are written for users and reviewed with the version change.
# Never fall back to commit subjects or generated technical changelogs.
if (!$ReleaseNotesPath) { $ReleaseNotesPath = "$PSScriptRoot/../docs/releases/$version.md" }
$changes = [string](Get-Content -Raw -LiteralPath $ReleaseNotesPath)
$changes = $changes.Trim()
if ([string]::IsNullOrWhiteSpace($changes)) { throw "Release notes for $version must not be empty" }

$download = "https://github.com/$Repository/releases/download/$tag"
$notesPath = Join-Path $ArtifactDirectory 'release-notes.txt'
@(
    "## What's new", '', $changes, '',
    '## Download for Windows 11 (64-bit)', '',
    "**[Install Audeze Overlay — recommended]($download/Audeze-Overlay-Setup.exe)**", '',
    'Run the installer, then open Audeze Overlay from the Start menu.', '',
    "[Portable ZIP — no installation]($download/Audeze-Overlay-Portable.zip): extract and run Audeze Overlay.exe.", '',
    '## Download for Linux (64-bit, experimental)', '',
    "[Flatpak bundle]($download/Audeze-Overlay.flatpak) · [Linux archive]($download/Audeze-Overlay-Linux-x86_64.tar.gz)", '',
    "**[Read the Linux setup guide first](https://github.com/$Repository/blob/$tag/docs/LINUX.md#install).** Both options require a host udev rule for headset access.", '',
    'Requires Wayland with layer-shell (such as KDE Plasma or Sway). GNOME and X11 are not supported. Contributor hardware testing covers the original Maxwell Xbox dongle on KDE Plasma; other Linux configurations remain unverified. Expect bugs.', '',
    '<details>', '<summary>Package details</summary>', '',
    'All packages include licenses. Packages are unsigned. SHA256SUMS.txt covers all four downloads. Source code archives are for developers.', '',
    "Built from commit $CommitSha.", '',
    '</details>'
) | Set-Content -LiteralPath $notesPath -Encoding utf8

# Keep partial uploads private. A retry may repair a draft but never replace a public release.
if (!$existing) {
    $options = @('--draft', '--title', $title, '--notes-file', $notesPath, '--target', $CommitSha)
    if (!$mainBuild) { $options += '--verify-tag' }
    Invoke-GitHub release create $tag --repo $Repository @options
}
Invoke-GitHub release upload $tag @files --repo $Repository --clobber
Invoke-GitHub release edit $tag --repo $Repository --title $title --notes-file $notesPath --target $CommitSha
# GitHub's by-tag REST endpoint cannot find an unpublished tag. Resolve the draft
# through the CLI, which supports draft lookup, then inspect it by release ID.
$draftId = Invoke-GitHub release view $tag --repo $Repository --json databaseId --jq .databaseId
$draft = Invoke-GitHub api "repos/$Repository/releases/$draftId" | ConvertFrom-Json
Assert-ReleaseAssets $draft
if ($mainBuild -and !(Test-CurrentMain)) {
    Write-Host 'A newer main commit arrived during upload; leaving this build as a draft.'
    return
}
$publishOptions = @('--draft=false', "--prerelease=$($prerelease.ToString().ToLowerInvariant())")
$publishOptions += if ($prerelease) { '--latest=false' } else { '--latest' }
Invoke-GitHub release edit $tag --repo $Repository @publishOptions
$published = Invoke-GitHub api "repos/$Repository/releases/tags/$tag" | ConvertFrom-Json
Assert-ReleaseAssets $published
if ($published.draft -or $published.prerelease -ne $prerelease) { throw 'Release publication failed' }
$publishedCommit = Invoke-GitHub api "repos/$Repository/commits/$tag" | ConvertFrom-Json
if ($publishedCommit.sha -ne $CommitSha) { throw 'Published release does not point to the tested commit' }
if (!$prerelease) {
    $latest = Invoke-GitHub api "repos/$Repository/releases/latest" | ConvertFrom-Json
    if ($latest.tag_name -ne $tag) { throw 'New build was not marked Latest' }
}

# Retain the former rolling release and every download, but remove its misleading title.
$legacy = $releases | Where-Object { $_.tag_name -eq 'development' -and $_.name -eq 'Latest main build' } | Select-Object -First 1
if ($legacy) {
    Invoke-GitHub release edit development --repo $Repository --title 'Archived main build (before release history)'
}
Write-Host "Published https://github.com/$Repository/releases/tag/$tag"
