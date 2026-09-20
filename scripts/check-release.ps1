param([switch]$Offline, [switch]$AllowDirty, [switch]$ArchiveBuild)
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $repoRoot
try {
    $metadataArgs = @('metadata', '--format-version', '1', '--no-deps', '--locked')
    if ($Offline) { $metadataArgs += '--offline' }
    $raw = & cargo @metadataArgs
    if ($LASTEXITCODE -ne 0) { throw 'Cargo metadata failed' }
    $metadata = ($raw -join "`n") | ConvertFrom-Json
    $members = @($metadata.packages | Where-Object { $_.id -in $metadata.workspace_members })
    $version = ($members | Where-Object name -EQ 'bevyistr').version
    if (!$version -or $members.Count -ne 11) { throw 'Expected the application and ten support crates' }
    $rootLicense = [IO.File]::ReadAllText((Join-Path $repoRoot 'LICENSE')).Replace("`r`n", "`n").TrimEnd()
    foreach ($package in $members) {
        if ($package.version -ne $version) { throw "Version mismatch: $($package.name)" }
        if ($package.name -notmatch '^bevyistr($|-)') { throw "Unprefixed package: $($package.name)" }
        if ($package.license -ne 'MIT' -or !$package.description -or !$package.readme) {
            throw "Missing metadata: $($package.name)"
        }
        $packageDir = Split-Path -Parent $package.manifest_path
        $license = [IO.File]::ReadAllText((Join-Path $packageDir 'LICENSE')).Replace("`r`n", "`n").TrimEnd()
        if ($license -ne $rootLicense) { throw "License differs: $($package.name)" }
        foreach ($dependency in $package.dependencies | Where-Object { $_.path }) {
            if ($dependency.req -ne "=$version" -or $dependency.name -notin $members.name) {
                throw "Invalid internal dependency: $($package.name) -> $($dependency.name)"
            }
        }
    }
    if ((Get-FileHash -LiteralPath 'assets/bevyistr.png').Hash -ne
        (Get-FileHash -LiteralPath 'app/assets/bevyistr.png').Hash) {
        throw 'Sync app/assets/bevyistr.png with assets/bevyistr.png'
    }
    $external = [IO.File]::ReadAllText((Join-Path $repoRoot 'materials.toml')).Replace("`r`n", "`n").TrimEnd()
    $embedded = [IO.File]::ReadAllText((Join-Path $repoRoot 'ui/assets/materials.toml')).Replace("`r`n", "`n").TrimEnd()
    if ($external -ne $embedded) { throw 'Sync the bundled material catalogue with materials.toml' }
    Write-Host "Metadata and assets: $($members.Count) packages, version $version"
    $packageArgs = @('package', '--workspace', '--locked')
    if ($Offline) { $packageArgs += '--offline' }
    if ($AllowDirty) { $packageArgs += '--allow-dirty' }
    if ($ArchiveBuild) { $packageArgs += '--no-verify' }
    & cargo @packageArgs
    if ($LASTEXITCODE -ne 0) { throw 'Cargo package failed' }
    if ($ArchiveBuild) {
        & (Join-Path $PSScriptRoot 'verify-release-archives.ps1') -Metadata $metadata -Offline:$Offline
        Write-Host 'Archive build/tests passed with local internal patches. Native package/dry-run remains a separate gate. Nothing published.'
    } else {
        Write-Host 'Cargo package verification passed; nothing has been published.'
    }
} finally {
    Pop-Location
}
