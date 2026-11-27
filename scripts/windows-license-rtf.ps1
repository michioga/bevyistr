# Formats the packaged MIT license for the Windows Installer rich-text control.
param([Parameter(Mandatory)][string]$License)

$licenseText = [IO.File]::ReadAllText($License).TrimEnd([char[]]"`r`n")
$escaped = [regex]::Replace($licenseText, '[\\{}\u0080-\uFFFF]', {
    param($match)
    $code = [int][char]$match.Value
    if ($code -lt 128) { return '\' + $match.Value }
    if ($code -gt 32767) { $code -= 65536 }
    return "\u$code?"
})
'{\rtf1\ansi\deff0{\fonttbl{\f0 Segoe UI;}}\f0\fs18 ' + ($escaped -replace '\r\n?|\n', '\par ') + '}'
