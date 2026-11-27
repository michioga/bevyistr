# Builds a per-user MSI from the staged Windows payload.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Bundle,
    [Parameter(Mandatory)][string]$AppIcon,
    [Parameter(Mandatory)][string]$Output,
    [string]$WixTool = 'wix',
    [switch]$AcceptWixEula
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$bundlePath = (Resolve-Path -LiteralPath $Bundle).Path
$appIconPath = (Resolve-Path -LiteralPath $AppIcon).Path
$outputPath = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Output)
$outputDirectory = Split-Path -Parent $outputPath
$info = Get-Content -LiteralPath (Join-Path $bundlePath 'BUILD-INFO.json') -Raw | ConvertFrom-Json
$version = [version]$info.version
if ($info.version -cne $version.ToString(3) -or $version.Major -gt 255 -or $version.Minor -gt 255 -or $version.Build -gt 65535) {
    throw 'Version must fit the three Windows Installer ProductVersion fields'
}
if ([IO.Path]::GetFileName($outputPath) -cne "bevyistr-v$version-windows-x86_64.msi") {
    throw 'MSI filename must match BUILD-INFO.json'
}
if ((Test-Path -LiteralPath $outputPath) -or !(Test-Path -LiteralPath $outputDirectory -PathType Container)) {
    throw 'Use an existing candidate directory and a new MSI filename'
}
$exe = Get-Item -LiteralPath (Join-Path $bundlePath 'bevyistr.exe')
if ($exe.VersionInfo.ProductVersion -cne $info.version -or $info.target -cne 'x86_64-pc-windows-msvc') {
    throw 'EXE version or build target differs from the package metadata'
}
$toolVersion = & $WixTool --version
if ($LASTEXITCODE -ne 0 -or $toolVersion -notmatch '^7\.0\.1(?:\+.+)?$') { throw 'WiX 7.0.1 is required' }

# Registry key paths keep MSI components per-user. Stable IDs and GUIDs
# preserve component identity across upgrades, even if another file is added.
function Get-IdentityHash([string]$Identity) {
    [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData(
        [Text.Encoding]::UTF8.GetBytes("bevyistr-msi/$($Identity.ToLowerInvariant())")
    ))
}
function Get-ComponentGuid([string]$Identity) {
    $bytes = [Convert]::FromHexString((Get-IdentityHash $Identity).Substring(0, 32))
    $bytes[7] = ($bytes[7] -band 0x0f) -bor 0x40
    $bytes[8] = ($bytes[8] -band 0x3f) -bor 0x80
    [Guid]::new($bytes).ToString('D')
}
function Add-WixElement([Xml.XmlElement]$Parent, [string]$Name, [hashtable]$Attributes) {
    $element = $Parent.OwnerDocument.CreateElement($Name, $Parent.NamespaceURI)
    foreach ($entry in $Attributes.GetEnumerator()) {
        $element.SetAttribute($entry.Key, [string]$entry.Value)
    }
    [void]$Parent.AppendChild($element)
    return $element
}

[xml]$source = '<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs"><Fragment /></Wix>'
$fragment = [Xml.XmlElement]$source.DocumentElement.FirstChild
$root = Add-WixElement $fragment 'DirectoryRef' @{ Id = 'INSTALLFOLDER' }
$group = Add-WixElement $fragment 'ComponentGroup' @{ Id = 'Payload' }
$directories = @{ '' = $root }

function Get-PayloadDirectory([string]$Relative) {
    if (!$directories.ContainsKey($Relative)) {
        $parent = Get-PayloadDirectory (Split-Path -Parent $Relative)
        $directories[$Relative] = Add-WixElement $parent 'Directory' @{
            Id = 'D_' + (Get-IdentityHash "directory/$Relative").Substring(0, 32)
            Name = Split-Path -Leaf $Relative
        }
    }
    return $directories[$Relative]
}

foreach ($file in Get-ChildItem -LiteralPath $bundlePath -Recurse -File | Sort-Object FullName) {
    $relative = [IO.Path]::GetRelativePath($bundlePath, $file.FullName)
    $id = (Get-IdentityHash "file/$relative").Substring(0, 32)
    $directory = Get-PayloadDirectory (Split-Path -Parent $relative)
    $component = Add-WixElement $group 'Component' @{
        Directory = $directory.GetAttribute('Id')
        Id = "C_$id"
        Guid = Get-ComponentGuid "file/$relative"
        Bitness = 'always64'
    }
    $fileId = if ($relative -ceq 'bevyistr.exe') { 'BevyistrExecutable' } else { "F_$id" }
    $null = Add-WixElement $component 'File' @{ Id = $fileId; Source = $file.FullName; Name = $file.Name }
    $null = Add-WixElement $component 'RegistryValue' @{
        Root = 'HKCU'; Key = 'Software\bevyistr\Installer\Files'
        Name = $id; Type = 'integer'; Value = 1; KeyPath = 'yes'
    }
}

$cleanup = Add-WixElement $group 'Component' @{
    Directory = 'INSTALLFOLDER'; Id = 'PayloadDirectories'; Guid = Get-ComponentGuid 'directories'; Bitness = 'always64'
}
foreach ($entry in $directories.GetEnumerator() | Sort-Object { $_.Key.Length } -Descending) {
    $id = $entry.Value.GetAttribute('Id')
    $null = Add-WixElement $cleanup 'RemoveFolder' @{ Id = "Remove_$id"; Directory = $id; On = 'uninstall' }
}
$null = Add-WixElement $cleanup 'RegistryValue' @{
    Root = 'HKCU'; Key = 'Software\bevyistr\Installer'
    Name = 'PayloadDirectories'; Type = 'integer'; Value = 1; KeyPath = 'yes'
}

$fragmentPath = Join-Path $outputDirectory 'installer-payload.wxs'
$source.Save($fragmentPath)
$licenseRtf = Join-Path $outputDirectory 'installer-license.rtf'
$licenseText = & (Join-Path $PSScriptRoot 'windows-license-rtf.ps1') -License (Join-Path $bundlePath 'LICENSE')
[IO.File]::WriteAllText($licenseRtf, $licenseText, [Text.Encoding]::ASCII)

$template = Join-Path (Split-Path -Parent $PSScriptRoot) 'distribution/bevyistr.wxs'
$wixArgs = @('build', $template, $fragmentPath, '-arch', 'x64', '-pdbtype', 'none',
    '-ext', 'WixToolset.UI.wixext/7.0.1', '-culture', 'ja-JP', '-bindvariable', "WixUILicenseRtf=$licenseRtf",
    '-d', "AppVersion=$version", '-d', "ProductCode=$(Get-ComponentGuid "product/$version")",
    '-d', "AppIcon=$appIconPath", '-o', $outputPath)
if ($AcceptWixEula) { $wixArgs += @('-acceptEula', 'wix7') }
& $WixTool @wixArgs
if ($LASTEXITCODE -ne 0 -or !(Test-Path -LiteralPath $outputPath -PathType Leaf)) { throw 'WiX MSI build failed' }
