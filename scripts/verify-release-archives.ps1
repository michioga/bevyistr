# Supplementary verification for Cargo workspace packaging issue #14396.
# No source edits, uploads, user Cargo config changes, or checksum bypasses.
param([Parameter(Mandatory)]$Metadata, [switch]$Offline)
$ErrorActionPreference = 'Stop'
$members = @($Metadata.packages | Where-Object { $_.id -in $Metadata.workspace_members })
$packageRoot = Join-Path $Metadata.target_directory 'package'
$stageParent = Join-Path $Metadata.target_directory 'release-archives'
$stage = Join-Path $stageParent ([Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $stage -Force | Out-Null
Write-Host "Extracted archive workspace: $stage"
$folders = @()
foreach ($package in $members) {
    $folder = "$($package.name)-$($package.version)"
    if ($folder -notmatch '^[a-zA-Z0-9_.+-]+$') { throw "Invalid package directory: $folder" }
    $archive = Join-Path $packageRoot "$folder.crate"
    $entries = @(& tar -tf $archive)
    if ($LASTEXITCODE -ne 0) { throw "Cannot inspect archive: $archive" }
    foreach ($entry in $entries) {
        if (!$entry.StartsWith("$folder/", [StringComparison]::Ordinal) -or
            $entry.Contains('\') -or '..' -in $entry.Split('/')) {
            throw "Unexpected archive path: $entry"
        }
    }
    foreach ($required in @('Cargo.toml', 'LICENSE', 'README.md')) {
        if ("$folder/$required" -notin $entries) { throw "$folder is missing $required" }
    }
    # These archives were just produced by cargo package from this checkout.
    & tar -xf $archive -C $stage
    if ($LASTEXITCODE -ne 0) { throw "Cannot extract archive: $archive" }
    $folders += $folder
    Write-Host "$folder SHA256=$((Get-FileHash -LiteralPath $archive).Hash)"
}

# Only redirect our own, unpublished dependencies to their extracted packages.
# All third-party packages retain the original crates.io checksums in Cargo.lock.
# Published Cargo.toml files are not modified; their version/alias constraints
# still have to resolve. No paths point back at the original source directories.
$quotedFolders = ($folders | ForEach-Object { '"' + $_ + '"' }) -join ', '
$manifest = "[workspace]`nresolver = `"3`"`nmembers = [$quotedFolders]`n`n[patch.crates-io]`n"
foreach ($package in $members) {
    $manifest += "$($package.name) = { path = `"$($package.name)-$($package.version)`" }`n"
}
$stagedManifest = Join-Path $stage 'Cargo.toml'
[IO.File]::WriteAllText($stagedManifest, $manifest, [Text.UTF8Encoding]::new($false))
Copy-Item -LiteralPath (Join-Path $Metadata.workspace_root 'Cargo.lock') -Destination (Join-Path $stage 'Cargo.lock')
$commonArgs = @('--manifest-path', $stagedManifest, '--locked', '--target-dir', $Metadata.target_directory)
if ($Offline) { $commonArgs += '--offline' }

& cargo build @commonArgs --package bevyistr --jobs 2
if ($LASTEXITCODE -ne 0) { throw 'Extracted application build failed' }
# Test binaries each link Bevy; linking all eleven simultaneously can exhaust
# memory even when the ordinary application build succeeds.
& cargo test @commonArgs --workspace --jobs 1
if ($LASTEXITCODE -ne 0) { throw 'Extracted package tests failed' }
# Avoid relying solely on feature unification from the application.
& cargo check @commonArgs --package bevyistr-ui --jobs 2
if ($LASTEXITCODE -ne 0) { throw 'Extracted UI standalone check failed' }
Write-Host 'Extracted archives verified. This is not a crates.io install or native publish dry-run.'
