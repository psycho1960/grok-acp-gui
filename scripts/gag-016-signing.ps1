param(
  [Parameter(Mandatory = $true)]
  [ValidateSet('Prepare', 'Cleanup')]
  [string]$Action,

  [Parameter(Mandatory = $true)]
  [string]$ConfigPath,

  [Parameter(Mandatory = $true)]
  [string]$ReceiptPath
)

$ErrorActionPreference = 'Stop'

if ($Action -eq 'Cleanup') {
  if (Test-Path -LiteralPath $ReceiptPath) {
    $thumbprint = (Get-Content -LiteralPath $ReceiptPath -Raw).Trim()
    if ($thumbprint -match '^[A-Fa-f0-9]{40}$') {
      $certificatePath = "Cert:\CurrentUser\My\$thumbprint"
      if (Test-Path -LiteralPath $certificatePath) {
        Remove-Item -LiteralPath $certificatePath -Force
      }
    }
    Remove-Item -LiteralPath $ReceiptPath -Force
  }
  if (Test-Path -LiteralPath $ConfigPath) {
    Remove-Item -LiteralPath $ConfigPath -Force
  }
  exit 0
}

$certificateBase64 = $env:GAG016_SIGNING_CERTIFICATE_BASE64
$certificatePassword = $env:GAG016_SIGNING_CERTIFICATE_PASSWORD
$timestampUrl = $env:GAG016_SIGNING_TIMESTAMP_URL
if ([string]::IsNullOrWhiteSpace($certificateBase64) -or
    [string]::IsNullOrWhiteSpace($certificatePassword) -or
    [string]::IsNullOrWhiteSpace($timestampUrl)) {
  throw 'Signed packaging requires the GAG016 signing certificate, password, and timestamp URL secrets.'
}

$temporaryPfx = Join-Path ([System.IO.Path]::GetTempPath()) ("gag016-{0}.pfx" -f [Guid]::NewGuid().ToString('N'))
try {
  [System.IO.File]::WriteAllBytes($temporaryPfx, [Convert]::FromBase64String($certificateBase64))
  $securePassword = ConvertTo-SecureString -String $certificatePassword -AsPlainText -Force
  $certificate = Import-PfxCertificate -FilePath $temporaryPfx -CertStoreLocation 'Cert:\CurrentUser\My' -Password $securePassword -Exportable:$false
  if ($null -eq $certificate -or $certificate.Thumbprint -notmatch '^[A-Fa-f0-9]{40}$') {
    throw 'The code-signing certificate could not be imported.'
  }

  Set-Content -LiteralPath $ReceiptPath -Value $certificate.Thumbprint -NoNewline
  $override = @{
    bundle = @{
      windows = @{
        certificateThumbprint = $certificate.Thumbprint
        digestAlgorithm = 'sha256'
        timestampUrl = $timestampUrl
        tsp = $true
      }
    }
  }
  $override | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $ConfigPath -Encoding utf8
}
finally {
  if (Test-Path -LiteralPath $temporaryPfx) {
    Remove-Item -LiteralPath $temporaryPfx -Force
  }
}
