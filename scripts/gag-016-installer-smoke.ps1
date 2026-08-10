param(
  [Parameter(Mandatory = $true)]
  [string]$BundlePath
)

$ErrorActionPreference = 'Stop'
if ($env:GAG016_SMOKE_ALLOW_EPHEMERAL_RUNNER -ne '1') {
  throw 'Installer smoke tests are restricted to an explicitly marked ephemeral Windows runner.'
}

$bundleRoot = (Resolve-Path -LiteralPath $BundlePath).Path
$nsisFiles = @(Get-ChildItem -LiteralPath (Join-Path $bundleRoot 'nsis') -Filter '*.exe' -File)
$msiFiles = @(Get-ChildItem -LiteralPath (Join-Path $bundleRoot 'msi') -Filter '*.msi' -File)
if ($nsisFiles.Count -ne 1 -or $msiFiles.Count -ne 1) {
  throw 'Exactly one NSIS installer and one MSI installer are required.'
}
$nsis = $nsisFiles[0]
$msi = $msiFiles[0]

$dataRoot = Join-Path $env:APPDATA 'com.grokacpgui.desktop'
if (Test-Path -LiteralPath $dataRoot) {
  throw "Refusing to run against an existing application data root: $dataRoot"
}

$externalRoot = Join-Path $env:RUNNER_TEMP 'gag-016 external Unicode 数据'
$sentinels = @(
  (Join-Path $dataRoot 'grok_acp_gui.db'),
  (Join-Path $dataRoot 'worktrees\fixture\worktree.sentinel'),
  (Join-Path $dataRoot 'recovery\fixture\recovery.sentinel'),
  (Join-Path $externalRoot 'user-repo\repo.sentinel'),
  (Join-Path $externalRoot 'user-repo\.grok-acp-gui\artifacts\artifact.sentinel')
)
foreach ($sentinel in $sentinels) {
  New-Item -ItemType Directory -Path (Split-Path -Parent $sentinel) -Force | Out-Null
  Set-Content -LiteralPath $sentinel -Value 'GAG-016 retention fixture' -NoNewline
}

function Assert-Sentinels {
  foreach ($sentinel in $sentinels) {
    if (-not (Test-Path -LiteralPath $sentinel -PathType Leaf)) {
      throw "Installer lifecycle removed retained data: $sentinel"
    }
  }
}

function Invoke-CheckedProcess {
  param([string]$FilePath, [string[]]$ArgumentList)
  $process = Start-Process -FilePath $FilePath -ArgumentList $ArgumentList -Wait -PassThru -WindowStyle Hidden
  if ($process.ExitCode -ne 0) {
    throw "$FilePath exited with code $($process.ExitCode)"
  }
}

function Find-NsisUninstaller {
  $keys = Get-ChildItem -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall' -ErrorAction SilentlyContinue
  foreach ($key in $keys) {
    $entry = Get-ItemProperty -LiteralPath $key.PSPath
    if ($entry.DisplayName -eq 'Grok ACP GUI' -and -not [string]::IsNullOrWhiteSpace($entry.UninstallString)) {
      return $entry
    }
  }
  throw 'NSIS uninstall registration was not found.'
}

function Assert-ApplicationStarts {
  param([object]$Entry)
  $executable = $null
  if (-not [string]::IsNullOrWhiteSpace($Entry.DisplayIcon)) {
    $executable = ($Entry.DisplayIcon -replace ',\d+$', '').Trim('"')
  }
  if ([string]::IsNullOrWhiteSpace($executable) -or -not (Test-Path -LiteralPath $executable -PathType Leaf)) {
    throw 'Installed application executable was not found from the NSIS registration.'
  }
  $application = Start-Process -FilePath $executable -PassThru
  Start-Sleep -Seconds 3
  if ($application.HasExited) {
    throw "Installed application exited during startup with code $($application.ExitCode)."
  }
  Stop-Process -Id $application.Id -Force
  $application.WaitForExit()
}

Invoke-CheckedProcess -FilePath $nsis.FullName -ArgumentList @('/S')
$nsisEntry = Find-NsisUninstaller
Assert-ApplicationStarts -Entry $nsisEntry
Invoke-CheckedProcess -FilePath $nsisEntry.UninstallString.Trim('"') -ArgumentList @('/S')
Assert-Sentinels

$quotedMsiPath = '"' + $msi.FullName + '"'
Invoke-CheckedProcess -FilePath 'msiexec.exe' -ArgumentList @('/i', $quotedMsiPath, '/qn', '/norestart')
Invoke-CheckedProcess -FilePath 'msiexec.exe' -ArgumentList @('/x', $quotedMsiPath, '/qn', '/norestart')
Assert-Sentinels

Invoke-CheckedProcess -FilePath $nsis.FullName -ArgumentList @('/S')
$nsisEntry = Find-NsisUninstaller
Assert-ApplicationStarts -Entry $nsisEntry
Assert-Sentinels
Invoke-CheckedProcess -FilePath $nsisEntry.UninstallString.Trim('"') -ArgumentList @('/S')
Assert-Sentinels

Write-Output 'GAG-016 NSIS/MSI install, uninstall, reinstall, and retained-data assertions passed.'
