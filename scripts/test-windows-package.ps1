# Verifies ZIP/checksums without starting the application.
param([Parameter(Mandatory)][string]$Zip, [Parameter(Mandatory)][string]$Checksums)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$zipPath = (Resolve-Path -LiteralPath $Zip).Path
$name = [IO.Path]::GetFileName($zipPath)
$line = @(Get-Content -LiteralPath $Checksums | Where-Object { $_ -match ('^[0-9a-fA-F]{64}  ' + [regex]::Escape($name) + '$') })
if ($line.Count -ne 1) { throw 'Expected exactly one checksum for the ZIP' }
$expected = $line[0].Substring(0,64)
if ((Get-FileHash -LiteralPath $zipPath).Hash -ne $expected) { throw 'ZIP checksum mismatch' }
$zipRoot = [IO.Path]::GetFileNameWithoutExtension($name)
$repoRoot = Split-Path -Parent $PSScriptRoot
$extract = Join-Path $repoRoot "target/package-smoke/$([Guid]::NewGuid().ToString('N'))"
Add-Type -AssemblyName System.IO.Compression
$archive = [IO.Compression.ZipFile]::OpenRead($zipPath)
try {
    $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($entry in $archive.Entries) {
        $path = $entry.FullName.Replace('\','/')
        if (!$path.StartsWith("$zipRoot/", [StringComparison]::Ordinal) -or '..' -in $path.Split('/') -or $path.Contains(':') -or !$seen.Add($path)) {
            throw "Invalid or duplicate ZIP path: $path"
        }
    }
} finally { $archive.Dispose() }
Expand-Archive -LiteralPath $zipPath -DestinationPath $extract
$bundle = Join-Path $extract $zipRoot
foreach ($file in @('bevyistr.exe','materials.toml','LICENSE','README.txt','THIRD-PARTY-LICENSES.txt','BUILD-INFO.json','DLL-DEPENDENCIES.txt','licenses/FiraMono-LICENSE.txt','licenses/rust/COPYRIGHT-library.html')) {
    if (!(Test-Path -LiteralPath (Join-Path $bundle $file) -PathType Leaf)) { throw "Missing packaged file: $file" }
}
$info = Get-Content -LiteralPath (Join-Path $bundle 'BUILD-INFO.json') -Raw | ConvertFrom-Json
if ($zipRoot -cne "bevyistr-v$($info.version)-windows-x86_64" -or $info.target -cne 'x86_64-pc-windows-msvc') { throw 'ZIP name/build target mismatch' }
if ($info.source_commit -notmatch '^[0-9a-f]{40}$') { throw 'Missing full source SHA' }
$files = @(Get-ChildItem -LiteralPath $bundle -File -Recurse)
$recorded = @($info.files_sha256.PSObject.Properties)
if ($files.Count -ne $recorded.Count + 1) { throw 'Unexpected ZIP contents or incomplete hash manifest' }
foreach ($file in $files) {
    $relative = [IO.Path]::GetRelativePath($bundle, $file.FullName).Replace('\','/')
    if ($relative -eq 'BUILD-INFO.json') { continue }
    $property = $info.files_sha256.PSObject.Properties[$relative]
    if (!$property -or (Get-FileHash -LiteralPath $file.FullName).Hash -ne $property.Value) { throw "Content checksum mismatch: $relative" }
    if ($file.Name -in @('bevyistr.toml','licenses-private.json') -or $file.Extension -in @('.msh','.res','.cnt','.pvtu')) { throw "Unexpected private/project file: $relative" }
}
$exe = Join-Path $bundle 'bevyistr.exe'
if ((Get-Item -LiteralPath $exe).VersionInfo.ProductVersion -ne $info.version) { throw 'EXE version mismatch' }
Write-Host "ZIP verified: $($files.Count) files, version $($info.version), source $($info.source_commit)"
Write-Host "Extracted for manual smoke test: $bundle"
