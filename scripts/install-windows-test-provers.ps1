# Native Windows integration dependencies. Only disposable test runners use this.
param([string]$Tools = (Join-Path $env:RUNNER_TEMP 'deductum-test-provers'))
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
if (-not $env:RUNNER_TEMP) { throw 'RUNNER_TEMP must identify a disposable test directory' }
New-Item -ItemType Directory -Path $Tools -Force | Out-Null

function Get-Verified([string]$Url, [string]$Name, [string]$Sha256) {
    $path = Join-Path $Tools $Name
    if ((Test-Path $path) -and (Get-FileHash $path -Algorithm SHA256).Hash.ToLowerInvariant() -eq $Sha256) {
        return $path
    }
    $partial = "$path.part"
    try {
        # A Linux build job can provide the same pinned bytes when an upstream
        # download host is unreachable from the hosted Windows runner.
        $cached = if ($env:DEDUCTUM_TEST_PROVER_CACHE) { Join-Path $env:DEDUCTUM_TEST_PROVER_CACHE $Name }
        if ($cached -and (Test-Path $cached)) {
            Copy-Item $cached $partial -Force
            if ((Get-FileHash $partial -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Sha256) {
                throw "Checksum mismatch in prover cache: $Name"
            }
        } else {
            for ($attempt = 1; $attempt -le 3; $attempt++) {
                try {
                    Write-Host "Downloading $Name (attempt $attempt of 3)"
                    Invoke-WebRequest $Url -OutFile $partial -TimeoutSec 600
                    if ((Get-FileHash $partial -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Sha256) {
                        throw "Checksum mismatch: $Name"
                    }
                    break
                } catch {
                    Remove-Item $partial -Force -ErrorAction SilentlyContinue
                    if ($attempt -eq 3) { throw }
                    Write-Warning "Download of $Name failed; retrying: $($_.Exception.Message)"
                    Start-Sleep -Seconds (5 * $attempt)
                }
            }
        }
        Move-Item $partial $path -Force
        return $path
    } finally {
        Remove-Item $partial -Force -ErrorAction SilentlyContinue
    }
}

$elan = Get-Verified 'https://github.com/leanprover/elan/releases/download/v4.2.4/elan-x86_64-pc-windows-msvc.zip' 'elan.zip' 'fad2e980a191c15884cc1d80d170ffc5fa84f3774541020145b66d1a644c6111'
Expand-Archive $elan (Join-Path $Tools 'elan-installer') -Force
$env:ELAN_HOME = Join-Path $Tools 'elan'
& "$Tools/elan-installer/elan-init.exe" -y --default-toolchain leanprover/lean4:v4.19.0 --no-modify-path
if ($LASTEXITCODE -ne 0) { throw 'Native Lean installation failed' }
$env:Path = "$env:ELAN_HOME/bin;$env:Path"
& "$env:ELAN_HOME/bin/lean.exe" --version
if ($LASTEXITCODE -ne 0) { throw 'Native Lean is unavailable' }

$isabelle = Get-Verified 'https://isabelle.in.tum.de/website-Isabelle2025/dist/Isabelle2025.exe' 'Isabelle2025.exe' '5225d0e28a3c9cb3e4e348b508d332165c65b806201f3936a7a861a6bb39a748'
# GitHub's Windows runner supplies 7-Zip; no additional executable is downloaded.
$sevenZip = (Get-Command 7z.exe -ErrorAction Stop).Source
& $sevenZip x -y "-o$Tools" $isabelle | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Isabelle extraction failed' }
$runtime = Join-Path $Tools 'Isabelle2025'
$initialization = Start-Process -FilePath "$runtime/Isabelle2025.exe" -ArgumentList '-init' -WorkingDirectory $runtime -Wait -PassThru
# The distribution's GUI launcher can return STILL_ACTIVE (259) after delegation.
# Require a successful native CLI probe as independent evidence of initialization.
if ($initialization.ExitCode -notin @(0, 259)) { throw "Isabelle initialization failed: $($initialization.ExitCode)" }
$bash = Join-Path $runtime 'contrib/cygwin/bin/bash.exe'
$cygpath = Join-Path $runtime 'contrib/cygwin/bin/cygpath.exe'
$launcher = (& $cygpath -u "$runtime/bin/isabelle").Trim()
if ($LASTEXITCODE -ne 0) { throw 'Bundled Cygwin path conversion failed' }
& $bash -lc ('exec "' + $launcher + '" version')
if ($LASTEXITCODE -ne 0) { throw 'Native Isabelle version probe failed' }
if ($env:GITHUB_ENV) {
    "ELAN_HOME=$env:ELAN_HOME" >> $env:GITHUB_ENV
    "DEDUCTUM_ISABELLE=$runtime/bin/isabelle" >> $env:GITHUB_ENV
    "$env:ELAN_HOME/bin" >> $env:GITHUB_PATH
}
