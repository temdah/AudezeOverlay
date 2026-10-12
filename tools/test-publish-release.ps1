# Contract tests for the real publisher with an in-memory GitHub CLI substitute.
# No network calls, credentials, or device access are used.
$ErrorActionPreference = 'Stop'
$global:AudezeReleaseTestsha = '1234567890abcdef1234567890abcdef12345678'
$global:AudezeReleaseTestbase = 'abcdef1234567890abcdef1234567890abcdef12'
$version = (Select-String -LiteralPath "$PSScriptRoot/../Cargo.toml" -Pattern '^version = "([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$global:AudezeReleaseTesttag = "v$version"
$testDirectory = Join-Path ([IO.Path]::GetTempPath()) ("audeze-release-test-" + [guid]::NewGuid())
$null = New-Item -ItemType Directory -Path $testDirectory

function Assert-That([bool]$Condition, [string]$Message) {
    if (!$Condition) { throw $Message }
}

function Reset-Fixture {
    $global:AudezeReleaseTeststate = @{
        publishedSha = $global:AudezeReleaseTestsha; head = $global:AudezeReleaseTestsha; calls = [Collections.Generic.List[object]]::new()
        failUpload = $false; failList = $false; supersedeAfterUpload = $false; tagExists = $false
        releases = @(@{
            id = 1; tag_name = 'development'; name = 'Latest main build'; draft = $false
            prerelease = $false; published_at = '2026-01-01T00:00:00Z'
            body = "Built from commit $global:AudezeReleaseTestbase."; assets = @(@{ name = 'old.zip'; size = 10 })
        })
        latest = 'development'
    }
    Set-Content -LiteralPath (Join-Path $testDirectory 'highlights.md') -Value 'Hear EQ changes live and keep your favorite profiles.'
    $artifactRoot = Join-Path $testDirectory 'artifacts'
    foreach ($platform in @('windows', 'linux', 'flatpak')) {
        $null = New-Item -ItemType Directory -Force -Path (Join-Path $artifactRoot $platform)
    }
    $groups = @{
        windows = @('Audeze-Overlay-Setup.exe', 'Audeze-Overlay-Portable.zip')
        linux = @('Audeze-Overlay-Linux-x86_64.tar.gz')
        flatpak = @('Audeze-Overlay.flatpak')
    }
    foreach ($platform in $groups.Keys) {
        $directory = Join-Path $artifactRoot $platform
        $lines = foreach ($name in $groups[$platform]) {
            $path = Join-Path $directory $name
            Set-Content -LiteralPath $path -Value "fixture: $name"
            "{0}  {1}" -f (Get-FileHash -LiteralPath $path).Hash.ToLowerInvariant(), $name
        }
        if ($platform -eq 'windows') { $lines | Set-Content -LiteralPath (Join-Path $directory 'SHA256SUMS.txt') }
        if ($platform -eq 'linux') { $lines | Set-Content -LiteralPath (Join-Path $directory 'Audeze-Overlay-Linux-x86_64.tar.gz.sha256') }
    }
    & "$PSScriptRoot/prepare-release-downloads.ps1" -ArtifactRoot $artifactRoot -OutputDirectory $testDirectory
}

function gh {
    $a = @($args)
    $global:AudezeReleaseTeststate.calls.Add($a)
    $global:LASTEXITCODE = 0
    if ($a[0] -eq 'api') {
        $endpoint = $a | Where-Object { $_ -like 'repos/*' } | Select-Object -First 1
        switch -Wildcard ($endpoint) {
            '*/git/ref/heads/main' { return (@{ object = @{ sha = $global:AudezeReleaseTeststate.head } } | ConvertTo-Json) }
            '*/releases?per_page=100' {
                if ($global:AudezeReleaseTeststate.failList) { $global:LASTEXITCODE = 1; return }
                # Match gh --paginate --slurp: an outer array of pages.
                return ConvertTo-Json -InputObject @(,@($global:AudezeReleaseTeststate.releases)) -Depth 10
            }
            '*/commits/*' {
                $value = if ($endpoint.EndsWith('/development')) { $global:AudezeReleaseTestbase } else { $global:AudezeReleaseTeststate.publishedSha }
                return (@{ sha = $value } | ConvertTo-Json)
            }
            '*/git/matching-refs/tags/*' {
                if ($global:AudezeReleaseTeststate.tagExists) {
                    return ConvertTo-Json -InputObject @(@{ ref = "refs/tags/$global:AudezeReleaseTesttag" })
                }
                return '[]'
            }
            '*/releases/latest' {
                return ($global:AudezeReleaseTeststate.releases | Where-Object tag_name -eq $global:AudezeReleaseTeststate.latest | ConvertTo-Json -Depth 10)
            }
            '*/releases/tags/*' {
                $target = $endpoint.Split('/')[-1]
                $release = $global:AudezeReleaseTeststate.releases | Where-Object tag_name -eq $target
                if ($release.draft) { $global:LASTEXITCODE = 1; return }
                return ($release | ConvertTo-Json -Depth 10)
            }
            '*/releases/2' {
                return ($global:AudezeReleaseTeststate.releases | Where-Object id -eq 2 | ConvertTo-Json -Depth 10)
            }
            default { throw "Unexpected API request: $endpoint" }
        }
    }
    if ($a[0] -ne 'release') { throw "Unexpected command: $a" }
    $target = $a[2]
    $release = $global:AudezeReleaseTeststate.releases | Where-Object tag_name -eq $target | Select-Object -First 1
    switch ($a[1]) {
        'view' {
            Assert-That ('databaseId' -in $a) 'Unexpected release lookup'
            return $release.id
        }
        'create' {
            Assert-That (!$release) 'Duplicate release creation'
            Assert-That ('--draft' -in $a) 'New release must start as a draft'
            Assert-That ($a[$a.IndexOf('--target') + 1] -eq $global:AudezeReleaseTestsha) 'Wrong target commit'
            $global:AudezeReleaseTeststate.releases += @{
                id = 2; tag_name = $target; name = ''; draft = $true; prerelease = $false
                published_at = $null; body = ''; assets = @()
            }
        }
        'upload' {
            Assert-That $release.draft 'Published assets must never be overwritten'
            if ($global:AudezeReleaseTeststate.failUpload) { $global:LASTEXITCODE = 1; return }
            $uploads = @($a[3..($a.IndexOf('--repo') - 1)])
            Assert-That ($uploads.Count -eq 5) 'Unexpected uploaded file count'
            $release.assets = @($uploads | ForEach-Object { @{ name = [IO.Path]::GetFileName($_); size = 7 } })
            if ($global:AudezeReleaseTeststate.supersedeAfterUpload) { $global:AudezeReleaseTeststate.head = $global:AudezeReleaseTestbase }
        }
        'edit' {
            Assert-That ($null -ne $release) 'Editing a nonexistent release'
            if ('--title' -in $a) { $release.name = $a[$a.IndexOf('--title') + 1] }
            if ('--notes-file' -in $a) { $release.body = Get-Content -Raw -LiteralPath $a[$a.IndexOf('--notes-file') + 1] }
            if ('--draft=false' -in $a) {
                Assert-That ($release.assets.Count -eq 5) 'Published before downloads were uploaded'
                $release.draft = $false
                $release.published_at = '2026-02-01T00:00:00Z'
                $release.prerelease = '--prerelease=true' -in $a
            }
            if ('--latest' -in $a) { $global:AudezeReleaseTeststate.latest = $target }
        }
        default { throw "Unexpected release operation: $a" }
    }
}

function Invoke-Publisher([string]$RefType = 'branch', [string]$RefName = 'main') {
    & "$PSScriptRoot/publish-release.ps1" -Repository 'owner/repo' -CommitSha $global:AudezeReleaseTestsha -RefType $RefType -RefName $RefName -ArtifactDirectory $testDirectory -ReleaseNotesPath (Join-Path $testDirectory 'highlights.md')
}

try {
    Reset-Fixture
    Invoke-Publisher
    $new = $global:AudezeReleaseTeststate.releases | Where-Object tag_name -eq $global:AudezeReleaseTesttag
    $old = $global:AudezeReleaseTeststate.releases | Where-Object tag_name -eq 'development'
    Assert-That ($global:AudezeReleaseTeststate.latest -eq $global:AudezeReleaseTesttag -and !$new.draft) 'New main release was not published as Latest'
    Assert-That ($new.body.Contains('Hear EQ changes live and keep your favorite profiles.')) 'User-facing release notes were lost'
    Assert-That ($new.name -eq "Audeze Overlay $version") 'Release title must contain only the app name and version'
    Assert-That ($old.assets[0].name -eq 'old.zip' -and $old.name.StartsWith('Archived')) 'Old release was not preserved in history'
    Assert-That ($new.body.Contains("/download/$global:AudezeReleaseTesttag/Audeze-Overlay-Setup.exe")) 'Installer link points to a different release'
    foreach ($name in @('Audeze-Overlay-Linux-x86_64.tar.gz', 'Audeze-Overlay.flatpak')) {
        Assert-That ($new.body.Contains("/download/$global:AudezeReleaseTesttag/$name")) 'Linux link points to a different release'
    }
    Assert-That ($new.body.Contains('experimental') -and $new.body.Contains("/blob/$global:AudezeReleaseTesttag/docs/LINUX.md#install")) 'Linux caveats or versioned instructions missing'
    $checksums = @(Get-Content -LiteralPath (Join-Path $testDirectory 'SHA256SUMS.txt'))
    Assert-That ($checksums.Count -eq 4) 'Combined checksum manifest does not cover all downloads'
    foreach ($line in $checksums) {
        $hash, $name = $line -split '  ', 2
        Assert-That ((Get-FileHash -LiteralPath (Join-Path $testDirectory $name)).Hash -eq $hash) 'Combined checksum is incorrect'
    }
    $callsBefore = $global:AudezeReleaseTeststate.calls.Count
    Invoke-Publisher
    Assert-That (@($global:AudezeReleaseTeststate.calls | Select-Object -Skip $callsBefore | Where-Object { $_[0] -eq 'release' }).Count -eq 0) 'Rerun mutated a published release'

    Reset-Fixture
    $global:AudezeReleaseTeststate.failUpload = $true
    $failed = $false
    try { Invoke-Publisher } catch { $failed = $true }
    Assert-That ($failed -and $global:AudezeReleaseTeststate.latest -eq 'development') 'Failed upload replaced Latest'
    Assert-That (($global:AudezeReleaseTeststate.releases | Where-Object tag_name -eq $global:AudezeReleaseTesttag).draft) 'Failed upload exposed an incomplete release'
    $global:AudezeReleaseTeststate.failUpload = $false
    Invoke-Publisher
    Assert-That ($global:AudezeReleaseTeststate.latest -eq $global:AudezeReleaseTesttag -and $global:AudezeReleaseTeststate.releases.Count -eq 2) 'Draft retry failed'

    Reset-Fixture
    $global:AudezeReleaseTeststate.head = $global:AudezeReleaseTestbase
    Invoke-Publisher
    Assert-That ($global:AudezeReleaseTeststate.releases.Count -eq 1) 'Superseded main created a release'

    Reset-Fixture
    $global:AudezeReleaseTeststate.supersedeAfterUpload = $true
    Invoke-Publisher
    Assert-That ($global:AudezeReleaseTeststate.latest -eq 'development') 'Superseded upload became Latest'

    Reset-Fixture
    $global:AudezeReleaseTeststate.failList = $true
    $failed = $false
    try { Invoke-Publisher } catch { $failed = $true }
    Assert-That ($failed -and $global:AudezeReleaseTeststate.releases.Count -eq 1) 'API failure was mistaken for an empty release history'

    Reset-Fixture
    $version = (Select-String -LiteralPath "$PSScriptRoot/../Cargo.toml" -Pattern '^version = "([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
    Invoke-Publisher -RefType tag -RefName "v$version"
    $new = $global:AudezeReleaseTeststate.releases | Where-Object tag_name -eq "v$version"
    Assert-That ($new.body.Contains('Hear EQ changes live and keep your favorite profiles.')) 'Version release lost curated notes'
    Assert-That (@($global:AudezeReleaseTeststate.calls | Where-Object { $_[1] -eq 'create' -and '--verify-tag' -in $_ }).Count -eq 1) 'Version release did not verify its tag'

    # A later docs-only commit cannot overwrite an already published version.
    $global:AudezeReleaseTeststate.publishedSha = $global:AudezeReleaseTestbase
    $callsBefore = $global:AudezeReleaseTeststate.calls.Count
    Invoke-Publisher
    Assert-That (@($global:AudezeReleaseTeststate.calls | Select-Object -Skip $callsBefore | Where-Object { $_[0] -eq 'release' }).Count -eq 0) 'Reused version mutated its published release'

    Reset-Fixture
    Set-Content -LiteralPath (Join-Path $testDirectory 'highlights.md') -Value ' '
    $failed = $false
    try { Invoke-Publisher } catch { $failed = $true }
    Assert-That ($failed -and $global:AudezeReleaseTeststate.releases.Count -eq 1) 'Empty highlights published a release'

    Reset-Fixture
    $global:AudezeReleaseTeststate.tagExists = $true
    $global:AudezeReleaseTeststate.publishedSha = $global:AudezeReleaseTestbase
    $failed = $false
    try { Invoke-Publisher } catch { $failed = $true }
    Assert-That ($failed -and $global:AudezeReleaseTeststate.releases.Count -eq 1) 'Mismatched tag was published'

    # Missing, empty or corrupt Linux downloads must never become public.
    foreach ($case in @('missing-flatpak', 'empty-flatpak', 'corrupt-linux', 'missing-checksum', 'unexpected-checksum')) {
        Reset-Fixture
        $artifactRoot = Join-Path $testDirectory 'artifacts'
        $bundle = Join-Path $artifactRoot 'flatpak/Audeze-Overlay.flatpak'
        $manifest = Join-Path $artifactRoot 'linux/Audeze-Overlay-Linux-x86_64.tar.gz.sha256'
        switch ($case) {
            'missing-flatpak' { Remove-Item -LiteralPath $bundle }
            'empty-flatpak' { [IO.File]::WriteAllBytes($bundle, [byte[]]@()) }
            'corrupt-linux' { Add-Content -LiteralPath (Join-Path $artifactRoot 'linux/Audeze-Overlay-Linux-x86_64.tar.gz') -Value 'corruption' }
            'missing-checksum' { Set-Content -LiteralPath $manifest -Value '' }
            'unexpected-checksum' { Add-Content -LiteralPath $manifest -Value (('a' * 64) + '  ../other') }
        }
        $failed = $false
        try { & "$PSScriptRoot/prepare-release-downloads.ps1" -ArtifactRoot $artifactRoot -OutputDirectory $testDirectory } catch { $failed = $true }
        Assert-That $failed "Incomplete/corrupt packages accepted: $case"
    }

    Reset-Fixture
    Remove-Item -LiteralPath (Join-Path $testDirectory 'Audeze-Overlay.flatpak')
    $failed = $false
    try { Invoke-Publisher } catch { $failed = $true }
    Assert-That ($failed -and $global:AudezeReleaseTeststate.releases.Count -eq 1) 'Publisher accepted missing Linux download'

    $realNotes = Get-Content -Raw -LiteralPath "$PSScriptRoot/../docs/releases/$version.md"
    Assert-That (![string]::IsNullOrWhiteSpace($realNotes)) 'Current version is missing release highlights'
    Write-Host 'PASS: combined package checksums, missing/corrupt Linux assets, versioned releases, curated notes, reruns, draft recovery, stale builds, API errors and missing highlights.'
} finally {
    # Only this test's uniquely named, resolved temporary directory is removed.
    $resolved = [IO.Path]::GetFullPath($testDirectory)
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    if (!$resolved.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase) -or !(Split-Path $resolved -Leaf).StartsWith('audeze-release-test-')) {
        throw 'Refusing to remove a directory outside the test temporary location'
    }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
