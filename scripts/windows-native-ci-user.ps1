# Invoked only by run-windows-native-ci.ps1 under its disposable standard account.
param([Parameter(Mandatory)][string]$Inputs, [Parameter(Mandatory)][string]$Work,
      [Parameter(Mandatory)][string]$ReleasePath, [ValidateSet('All', 'Host')][string]$Gate = 'All')
$ErrorActionPreference = 'Stop'
# No worker, installer, fixture or test runs before the parent assigns this
# process to its kill-on-close Job. The release file is in admin-owned RX input.
$guardDeadline = [DateTime]::UtcNow.AddSeconds(30)
while (-not (Test-Path -LiteralPath $ReleasePath)) {
    if ([DateTime]::UtcNow -ge $guardDeadline) { throw 'CI parent did not establish process containment' }
    Start-Sleep -Milliseconds 20
}
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$OutputEncoding = [Console]::OutputEncoding
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if ($identity.User.Value -ne $env:DEDUCTUM_CI_EXPECTED_SID -or
    $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Native verification must run under its dedicated standard-user token'
}
# LoadUserProfile selects this account's known folders; never inherit runneradmin.
$env:USERPROFILE = [Environment]::GetFolderPath([Environment+SpecialFolder]::UserProfile)
$env:APPDATA = [Environment]::GetFolderPath([Environment+SpecialFolder]::ApplicationData)
$env:LOCALAPPDATA = [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
if (-not $env:USERPROFILE -or -not $env:APPDATA -or -not $env:LOCALAPPDATA) { throw 'The test account profile was not loaded' }
$env:HOME = $env:USERPROFILE
$env:HOMEDRIVE = [IO.Path]::GetPathRoot($env:USERPROFILE).TrimEnd('\')
$env:HOMEPATH = $env:USERPROFILE.Substring($env:HOMEDRIVE.Length)
New-Item -ItemType Directory -Path $Work | Out-Null
$Work = (Resolve-Path -LiteralPath $Work).ProviderPath
$owner = (Get-Acl -LiteralPath $Work).GetOwner([Security.Principal.SecurityIdentifier])
if ($owner.Value -ne $identity.User.Value) { throw 'The private test work directory has the wrong owner' }
$env:TEMP = Join-Path $Work 'temp'
$env:TMP = $env:TEMP
$env:RUNNER_TEMP = Join-Path $Work 'runner-temp'
New-Item -ItemType Directory -Force -Path $env:TEMP, $env:RUNNER_TEMP | Out-Null
$env:PYTHONUTF8 = '1'
$env:PYTHONDONTWRITEBYTECODE = '1'
Write-Output "Native CI standard SID=$($identity.User.Value), profile=$env:USERPROFILE, TEMP=$env:TEMP"
$source = Join-Path $Inputs 'source'
$artifact = Join-Path $Inputs 'artifact'
$zip = @(Get-ChildItem $artifact -Filter '*.zip')
if ($zip.Count -ne 1) { throw 'Expected exactly one Windows package' }
$receipt = Get-Content ([IO.Path]::ChangeExtension($zip[0].FullName, '.json')) -Raw | ConvertFrom-Json
if ($receipt.source_dirty) { throw 'Native CI requires a clean source package' }
if ((Get-FileHash $zip[0].FullName -Algorithm SHA256).Hash.ToLowerInvariant() -ne $receipt.archive_sha256) { throw 'Package checksum mismatch' }
$destination = Join-Path $Work 'Deductum native β proof 😀'
Expand-Archive $zip[0].FullName $destination
$package = @(Get-ChildItem $destination -Directory)
if ($package.Count -ne 1) { throw 'Expected one package root' }
$env:DEDUCTUM_PACKAGE = $package[0].FullName
$env:DEDUCTUM_TEST_HOST_ROOT = Join-Path $env:DEDUCTUM_PACKAGE 'share/deductum/host'
$env:DEDUCTUM_TEST_ROOT = $source
$env:DEDUCTUM_TEST_SCRATCH = Join-Path $Work 'Deductum Lean α proof 😀'
$env:DEDUCTUM_TEST_PROVER_CACHE = Join-Path $Inputs 'provers'
Set-Location $source
. (Join-Path $source 'scripts/install-windows-test-provers.ps1')
$env:DEDUCTUM_ISABELLE = Join-Path $env:RUNNER_TEMP 'deductum-test-provers/Isabelle2025/bin/isabelle'
$python = Join-Path $env:DEDUCTUM_PACKAGE 'python/python.exe'
& $python -X utf8 -m unittest discover -s host/tests -p 'test_*.py' -v
if ($LASTEXITCODE -ne 0) { throw 'Windows host tests failed' }
if ($Gate -eq 'Host') { return }
& (Join-Path $artifact 'deductum-tests.exe') --test-threads=4
if ($LASTEXITCODE -ne 0) { throw 'Windows app tests failed' }
$env:DEDUCTUM_STATE_DIR = Join-Path $Work 'deductum-isabelle-state'
& $python -X utf8 host/tests/integration_lean.py --skip-dsrv --core-fixture (Join-Path $source 'scripts/ci-fixtures/lean')
if ($LASTEXITCODE -ne 0) { throw 'Native Lean integration failed' }
try {
    & $python -X utf8 host/tests/integration_host.py
    if ($LASTEXITCODE -ne 0) { throw 'Native Isabelle integration failed' }
} catch {
    $integrationError = $_
    # This is the disposable integration's own host log, before parent cleanup.
    $hostLog = Join-Path $env:DEDUCTUM_STATE_DIR 'host.log'
    try {
        if (Test-Path -LiteralPath $hostLog) {
            Write-Output 'Native Isabelle integration host log (last 100 lines):'
            Get-Content -LiteralPath $hostLog -Tail 100 | ForEach-Object {
                if ($_.Length -gt 4000) { $_.Substring($_.Length - 4000) } else { $_ }
            }
        }
    } catch { Write-Warning 'Could not read the disposable integration host log' }
    throw $integrationError
}
