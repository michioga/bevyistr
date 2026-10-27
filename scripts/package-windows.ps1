# Creates a reviewable ZIP. Never commits, tags, uploads, or publishes.
[CmdletBinding()]
param(
    [switch]$AllowDirty,
    [string]$Tag = '',
    [string]$AboutTool = 'cargo-about',
    [string]$ResultPath = 'target/windows-package-result.json'
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repoRoot = Split-Path -Parent $PSScriptRoot
$toolchain = '1.99.1'
$target = 'x86_64-pc-windows-msvc'
$rustFlagsName = 'CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS'
$oldFlags = [Environment]::GetEnvironmentVariable($rustFlagsName, 'Process')
Push-Location -LiteralPath $repoRoot
try {
    if (!$IsWindows) { throw 'Build this package on Windows with PowerShell 7.' }
    if ($env:RUSTFLAGS -or $env:CARGO_ENCODED_RUSTFLAGS) { throw 'Unset RUSTFLAGS/CARGO_ENCODED_RUSTFLAGS for the controlled release build.' }
    $dirty = [bool]@(git status --porcelain --untracked-files=normal).Count
    if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect work tree' }
    if ($dirty -and !$AllowDirty) { throw 'Commit changes first, or use -AllowDirty for a local candidate.' }
    $sha = (git rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0) { throw 'Cannot read source commit' }
    $raw = & cargo "+$toolchain" metadata --format-version 1 --no-deps --locked
    if ($LASTEXITCODE -ne 0) { throw 'Cargo metadata failed' }
    $metadata = ($raw -join [Environment]::NewLine) | ConvertFrom-Json
    $app = $metadata.packages | Where-Object name -EQ 'bevyistr'
    $version = $app.version
    if ($version -notmatch '^\d+\.\d+\.\d+$') { throw 'Expected a release version X.Y.Z' }
    if ($Tag) {
        if ($Tag -cne "v$version" -or $dirty) { throw 'Tag/version mismatch or dirty tagged build' }
        $tagType = & git cat-file -t "refs/tags/$Tag"
        if ($LASTEXITCODE -ne 0 -or $tagType -ne 'tag') { throw 'An existing annotated tag is required' }
        $tagSha = & git rev-parse "refs/tags/$Tag^{}"
        if ($LASTEXITCODE -ne 0 -or $tagSha -ne $sha) { throw 'Checkout the tagged commit first' }
    }
    if ((& $AboutTool --version) -ne 'cargo-about 0.9.2') { throw 'Install cargo-about 0.9.2 with --features cli' }

    # A unique output directory prevents silently replacing a reviewed candidate.
    $name = "bevyistr-v$version-windows-x86_64"
    $runName = "$name-$($sha.Substring(0,8))-$([Guid]::NewGuid().ToString('N').Substring(0,8))"
    $runRoot = Join-Path $repoRoot "target/dist/$runName"
    $bundle = Join-Path $runRoot $name
    New-Item -ItemType Directory -Path $bundle -Force | Out-Null
    $noticeJson = Join-Path $runRoot 'licenses-private.json'
    & $AboutTool generate --manifest-path app/Cargo.toml --config distribution/about-windows.toml --locked --fail --format json --output-file $noticeJson
    if ($LASTEXITCODE -ne 0) { throw 'License gathering failed; do not distribute this directory' }
    $licenses = Get-Content -LiteralPath $noticeJson -Raw | ConvertFrom-Json
    & $AboutTool generate --manifest-path app/Cargo.toml --config distribution/about-windows.toml --locked --fail --output-file (Join-Path $bundle 'THIRD-PARTY-LICENSES.txt') distribution/licenses.hbs
    if ($LASTEXITCODE -ne 0) { throw 'License text generation failed' }

    [Environment]::SetEnvironmentVariable($rustFlagsName, '-C target-feature=+crt-static', 'Process')
    & cargo "+$toolchain" build --package bevyistr --release --locked --target $target --target-dir target --jobs 2
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed' }
    $exe = Join-Path $repoRoot "target/$target/release/bevyistr.exe"
    $info = (Get-Item -LiteralPath $exe).VersionInfo
    if ($info.ProductVersion -ne $version) { throw 'EXE version does not match Cargo version' }
    Copy-Item -LiteralPath $exe -Destination $bundle
    Copy-Item -LiteralPath 'materials.toml', 'LICENSE' -Destination $bundle
    $readme = [IO.File]::ReadAllText((Join-Path $repoRoot 'distribution/README-windows.txt'))
    if (!$readme.StartsWith("bevyistr $version ")) { throw 'Update distribution/README-windows.txt version' }
    [IO.File]::WriteAllText((Join-Path $bundle 'README.txt'), $readme, [Text.UTF8Encoding]::new($false))
    Copy-Item -LiteralPath 'distribution/licenses' -Destination $bundle -Recurse

    # Preserve crate copyright notices, including licenses of bundled native code.
    # Raw cargo-about JSON stays OUTSIDE the ZIP: it contains machine-local paths.
    foreach ($entry in $licenses.crates) {
        $package = $entry.package
        if (!$package.source) { continue }
        $sourceRoot = Split-Path -Parent $package.manifest_path
        $destRoot = Join-Path $bundle "licenses/crates/$($package.name)-$($package.version)"
        foreach ($file in Get-ChildItem -LiteralPath $sourceRoot -Recurse -File) {
            if ($file.Name -notmatch '^(LICENSE|LICENCE|COPYING|COPYRIGHT|NOTICE)([-.].*)?$|^OFL(\.txt)?$') { continue }
            $relative = [IO.Path]::GetRelativePath($sourceRoot, $file.FullName)
            $dest = Join-Path $destRoot $relative
            New-Item -ItemType Directory -Path (Split-Path -Parent $dest) -Force | Out-Null
            Copy-Item -LiteralPath $file.FullName -Destination $dest
        }
        # Some embedded Bevy shaders carry additional author notices in-source.
        if ($package.name -like 'bevy_*') {
            foreach ($shader in Get-ChildItem -LiteralPath $sourceRoot -Filter '*.wgsl' -Recurse -File) {
                $text = [IO.File]::ReadAllText($shader.FullName)
                if ($text -match '(?s)\A\s*(/\*.*?\*/)') {
                    $header = $Matches[1]
                    if ($header -match '(?i)copyright') {
                        $relative = [IO.Path]::GetRelativePath($sourceRoot, $shader.FullName) + '.notice.txt'
                        $dest = Join-Path $destRoot $relative
                        New-Item -ItemType Directory -Path (Split-Path -Parent $dest) -Force | Out-Null
                        [IO.File]::WriteAllText($dest, $header, [Text.UTF8Encoding]::new($false))
                    }
                }
            }
        }
    }
    $sysroot = (& rustc "+$toolchain" --print sysroot).Trim()
    if ($LASTEXITCODE -ne 0) { throw 'Cannot locate Rust standard-library notices' }
    $rustDocs = Join-Path $sysroot 'share/doc/rust'
    $rustNotices = Join-Path $bundle 'licenses/rust'
    New-Item -ItemType Directory -Path $rustNotices -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $rustDocs 'COPYRIGHT-library.html') -Destination $rustNotices
    Copy-Item -LiteralPath (Join-Path $rustDocs 'licenses') -Destination $rustNotices -Recurse

    $dumpbin = Get-Command dumpbin.exe -ErrorAction SilentlyContinue
    if ($dumpbin) { $dumpbinPath = $dumpbin.Source } else {
        $vswhere = Join-Path ([Environment]::GetFolderPath('ProgramFilesX86')) 'Microsoft Visual Studio/Installer/vswhere.exe'
        $paths = @(& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -find 'VC\Tools\MSVC\**\bin\Hostx64\x64\dumpbin.exe')
        if ($LASTEXITCODE -ne 0 -or !$paths.Count) { throw 'MSVC dumpbin is required for DLL inspection' }
        $dumpbinPath = $paths[-1]
    }
    $imports = & $dumpbinPath /DEPENDENTS $exe
    if ($LASTEXITCODE -ne 0) { throw 'PE dependency inspection failed' }
    $dlls = @($imports | ForEach-Object { if ($_ -match '^\s+([A-Za-z0-9_.-]+\.dll)\s*$') { $Matches[1] } } | Sort-Object -Unique)
    if (!$dlls.Count) { throw 'No PE imports found; inspect dumpbin output' }
    if ($dlls -match '^(?i:VCRUNTIME|MSVCP|libiomp|impi|zstd)') { throw "Unexpected redistributable DLL dependency: $dlls" }
    $dlls | Set-Content -LiteralPath (Join-Path $bundle 'DLL-DEPENDENCIES.txt') -Encoding utf8

    $fileHashes = @{}
    Get-ChildItem -LiteralPath $bundle -File -Recurse | ForEach-Object {
        $relative = [IO.Path]::GetRelativePath($bundle, $_.FullName).Replace('\','/')
        $fileHashes[$relative] = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    $record = [ordered]@{
        version = $version; source_commit = $sha; source_tag = $Tag; source_dirty = $dirty
        target = $target; rustc = (& rustc "+$toolchain" --version); cargo = (& cargo "+$toolchain" --version)
        rustflags = '-C target-feature=+crt-static'; profile = 'release'
        built_utc = [DateTime]::UtcNow.ToString('o')
        runner_image = $env:ImageOS; runner_image_version = $env:ImageVersion
        cargo_lock_sha256 = (Get-FileHash -LiteralPath Cargo.lock).Hash.ToLowerInvariant()
        license_tool = 'cargo-about 0.9.2'; license_crates = $licenses.crates.Count
        direct_dll_imports = $dlls; files_sha256 = $fileHashes
    }
    $record | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $bundle 'BUILD-INFO.json') -Encoding utf8
    $zip = Join-Path $runRoot "$name.zip"
    Compress-Archive -LiteralPath $bundle -DestinationPath $zip -CompressionLevel Optimal
    $hash = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $name.zip" | Set-Content -LiteralPath (Join-Path $runRoot 'SHA256SUMS.txt') -Encoding ascii
    $public = Join-Path $runRoot 'artifacts'
    New-Item -ItemType Directory -Path $public | Out-Null
    Copy-Item -LiteralPath $zip, (Join-Path $runRoot 'SHA256SUMS.txt') -Destination $public
    $result = [ordered]@{ version=$version; source_commit=$sha; source_dirty=$dirty; tag=$Tag; artifact_directory=$public; zip=$zip; sha256=$hash; bundle=$bundle }
    $result | ConvertTo-Json | Set-Content -LiteralPath $ResultPath -Encoding utf8
    Write-Host "Created $zip"
    Write-Host "SHA256 $hash"
} finally {
    [Environment]::SetEnvironmentVariable($rustFlagsName, $oldFlags, 'Process')
    Pop-Location
}
