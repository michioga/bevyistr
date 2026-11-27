# Inspects and extracts a built MSI without installing it.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Msi,
    [Parameter(Mandatory)][string]$Checksums,
    [string]$WixTool = 'wix',
    [switch]$AcceptWixEula
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$msiPath = (Resolve-Path -LiteralPath $Msi).Path
$name = [IO.Path]::GetFileName($msiPath)
$line = @(Get-Content -LiteralPath $Checksums | Where-Object { $_ -match ('^[0-9a-fA-F]{64}  ' + [regex]::Escape($name) + '$') })
if ($line.Count -ne 1) { throw 'Expected exactly one checksum for the MSI' }
if ((Get-FileHash -LiteralPath $msiPath).Hash -ne $line[0].Substring(0,64)) { throw 'MSI checksum mismatch' }

$repoRoot = Split-Path -Parent $PSScriptRoot
$work = Join-Path $repoRoot "target/package-smoke/$([Guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $work | Out-Null
$extract = Join-Path $work 'extracted'
$xmlPath = Join-Path $work 'installer.wxs'
$argsForWix = @('msi', 'decompile', $msiPath, '-x', $extract, '-o', $xmlPath)
if ($AcceptWixEula) { $argsForWix += @('-acceptEula', 'wix7') }
& $WixTool @argsForWix
if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect and extract the MSI' }

[xml]$definition = Get-Content -LiteralPath $xmlPath -Raw
$ns = [Xml.XmlNamespaceManager]::new($definition.NameTable)
$ns.AddNamespace('w', 'http://wixtoolset.org/schemas/v4/wxs')
$package = $definition.SelectSingleNode('/w:Wix/w:Package', $ns)
if (!$package -or $package.GetAttribute('Name') -cne 'bevyistr' -or $package.GetAttribute('Scope') -cne 'perUser') {
    throw 'Expected the bevyistr per-user MSI'
}
if ($package.GetAttribute('UpgradeCode').Trim('{}') -ine 'E00A7055-6478-4A7B-9CB4-0ED5A16C99CD' -or
    !$package.SelectSingleNode('w:MajorUpgrade', $ns)) {
    throw 'MSI upgrade identity is inconsistent'
}
# Read the actual MSI sequence. WiX decompile may infer a different MajorUpgrade
# schedule from a non-default sequence number.
$installer = New-Object -ComObject WindowsInstaller.Installer
$database = $installer.OpenDatabase($msiPath, 0)
$view = $database.OpenView('SELECT `Action`, `Sequence` FROM `InstallExecuteSequence`')
$sequence = @{}
try {
    $view.Execute()
    while ($row = $view.Fetch()) {
        $sequence[$row.StringData(1)] = $row.IntegerData(2)
    }
} finally {
    $view.Close()
}
if (!$sequence.ContainsKey('RemoveExistingProducts') -or
    $sequence['RemoveExistingProducts'] -le $sequence['InstallInitialize'] -or
    $sequence['RemoveExistingProducts'] -ge $sequence['ProcessComponents']) {
    throw 'Major upgrade must remove the old version inside the rollback transaction, before installing the new version'
}
$shortcut = $package.SelectSingleNode('.//w:Shortcut[@Id="BevyistrShortcut"]', $ns)
if (!$shortcut -or !$shortcut.SelectSingleNode('ancestor::w:StandardDirectory[@Id="ProgramMenuFolder"]', $ns) -or
    $shortcut.GetAttribute('Target') -cne '[#BevyistrExecutable]' -or
    $shortcut.GetAttribute('WorkingDirectory') -cne 'INSTALLFOLDER') {
    throw 'MSI is missing the Start menu shortcut for bevyistr.exe'
}
$icon = $package.SelectSingleNode('w:Property[@Id="ARPPRODUCTICON" and @Value="BevyistrApp.ico"]', $ns)
$iconPath = Join-Path $extract 'Icon/BevyistrApp.ico'
if (!$icon -or !(Test-Path -LiteralPath $iconPath -PathType Leaf) -or $shortcut.HasAttribute('Icon')) {
    throw 'Expected the app-list icon stream and a Start menu shortcut that uses the EXE icon'
}
$components = $package.SelectNodes('.//w:Component', $ns)
foreach ($component in $components) {
    if ($component.GetAttribute('Bitness') -cne 'always64' -or
        !$component.SelectSingleNode('w:RegistryValue[@Root="HKCU" and @KeyPath="yes"]', $ns)) {
        throw "Non-user or non-x64 component: $($component.GetAttribute('Id'))"
    }
}

$files = @{}
$cabFiles = Join-Path $extract 'File'
$payload = Join-Path $work 'payload'
$installPath = '//w:StandardDirectory[@Id="LocalAppDataFolder"]/w:Directory[@Name="Programs"]/w:Directory[@Id="INSTALLFOLDER" and @Name="bevyistr"]'
$install = $package.SelectSingleNode($installPath, $ns)
if (!$install) { throw 'MSI install directory must stay in the current user profile' }
foreach ($file in $package.SelectNodes('.//w:File', $ns)) {
    $segments = [Collections.Generic.List[string]]::new()
    $segments.Add($file.GetAttribute('Name'))
    $dir = $file.ParentNode.ParentNode
    while ($dir -and $dir -ne $install -and $dir.LocalName -ceq 'Directory') {
        $segments.Insert(0, $dir.GetAttribute('Name'))
        $dir = $dir.ParentNode
    }
    if ($dir -ne $install -or @($segments | Where-Object { $_ -eq '' -or $_ -in @('.', '..') -or $_ -match '[\\/:*?"<>|]' }).Count) {
        throw "Unexpected MSI payload path: $($segments -join '/')"
    }
    $relative = $segments -join '/'
    $id = $file.GetAttribute('Id')
    if ($files.ContainsKey($relative) -or $id -cnotmatch '^(BevyistrExecutable|F_[A-F0-9]{32})$') { throw "Unexpected duplicate or File ID: $relative" }
    $cabEntry = Join-Path $cabFiles $id
    if (!(Test-Path -LiteralPath $cabEntry -PathType Leaf)) { throw "Missing embedded file: $relative" }
    $files[$relative] = $cabEntry
}
if (@(Get-ChildItem -LiteralPath $cabFiles -File).Count -ne $files.Count) {
    throw 'Unexpected cabinet contents'
}

foreach ($file in @('bevyistr.exe','materials.toml','LICENSE','README.txt','THIRD-PARTY-LICENSES.txt','BUILD-INFO.json','DLL-DEPENDENCIES.txt','licenses/FiraMono-LICENSE.txt','licenses/UDEVGothic-LICENSE.txt','licenses/rust/COPYRIGHT-library.html')) {
    if (!$files.ContainsKey($file)) { throw "Missing packaged file: $file" }
}
$info = Get-Content -LiteralPath $files['BUILD-INFO.json'] -Raw | ConvertFrom-Json
if ($name -cne "bevyistr-v$($info.version)-windows-x86_64.msi" -or
    $info.target -cne 'x86_64-pc-windows-msvc' -or $package.GetAttribute('Version') -cne $info.version) {
    throw 'MSI, filename and build metadata disagree'
}
if ((Get-FileHash -LiteralPath $iconPath).Hash -ne $info.app_icon_sha256) {
    throw 'The MSI app-list icon does not match the ICO used to build bevyistr.exe'
}
if ($info.source_commit -notmatch '^[0-9a-f]{40}$') { throw 'Missing full source SHA' }
$recorded = @($info.files_sha256.PSObject.Properties)
if ($files.Count -ne $recorded.Count + 1) { throw 'Unexpected MSI contents or incomplete hash manifest' }
foreach ($relative in $files.Keys) {
    if ($relative -ne 'BUILD-INFO.json') {
        $property = $info.files_sha256.PSObject.Properties[$relative]
        if (!$property -or (Get-FileHash -LiteralPath $files[$relative]).Hash -ne $property.Value) {
            throw "Content checksum mismatch: $relative"
        }
    }
    if ([IO.Path]::GetFileName($relative) -in @('bevyistr.toml','licenses-private.json') -or
        [IO.Path]::GetExtension($relative) -in @('.msh','.res','.cnt','.pvtu')) {
        throw "Unexpected private/project file: $relative"
    }
    $destination = Join-Path $payload $relative
    New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
    Move-Item -LiteralPath $files[$relative] -Destination $destination
}
$exe = Join-Path $payload 'bevyistr.exe'
if ((Get-Item -LiteralPath $exe).VersionInfo.ProductVersion -cne $info.version) { throw 'EXE version mismatch' }
Write-Host "MSI verified: $($files.Count) files, version $($info.version), source $($info.source_commit)"
Write-Host "Extracted for manual smoke test: $payload"
