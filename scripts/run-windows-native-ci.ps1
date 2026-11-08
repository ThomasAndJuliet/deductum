# Run native verification with the same standard-user ownership as the Windows VM.
param(
    [string]$SourceRoot = (Split-Path $PSScriptRoot -Parent),
    [string]$ArtifactRoot = (Join-Path $env:RUNNER_TEMP 'deductum-artifact'),
    [string]$ProverCacheRoot = (Join-Path $env:RUNNER_TEMP 'windows-test-provers'),
    [ValidateSet('All', 'Host')][string]$Gate = 'All',
    [ValidateRange(0.1, 80)][double]$TimeoutMinutes = 80
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Creating disposable CI accounts requires the hosted runner administrator'
}
$id = [Guid]::NewGuid().ToString('N')
$root = Join-Path "$env:SystemDrive\" "DeductumCI-$id"
$testName = "dct$($id.Substring(0, 10))"
$otherName = "dci$($id.Substring(0, 10))"
$createdUsers = [Collections.Generic.List[string]]::new()
$createdSids = @{}
$process = $null
$processStarted = $false
$createdRoot = $false
$job = $null
$desktopAccess = $null

# The parent owns the only Job handle; it is never inherited by test processes.
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Threading;
public sealed class DeductumCiJob : IDisposable {
    [StructLayout(LayoutKind.Sequential)] struct BasicLimits {
        public long PerProcessUserTimeLimit, PerJobUserTimeLimit;
        public uint LimitFlags;
        public UIntPtr MinimumWorkingSetSize, MaximumWorkingSetSize;
        public uint ActiveProcessLimit;
        public UIntPtr Affinity;
        public uint PriorityClass, SchedulingClass;
    }
    [StructLayout(LayoutKind.Sequential)] struct IoCounters {
        public ulong ReadOperationCount, WriteOperationCount, OtherOperationCount;
        public ulong ReadTransferCount, WriteTransferCount, OtherTransferCount;
    }
    [StructLayout(LayoutKind.Sequential)] struct ExtendedLimits {
        public BasicLimits BasicLimitInformation;
        public IoCounters IoInfo;
        public UIntPtr ProcessMemoryLimit, JobMemoryLimit, PeakProcessMemoryUsed, PeakJobMemoryUsed;
    }
    [StructLayout(LayoutKind.Sequential)] struct Accounting {
        public long TotalUserTime, TotalKernelTime, ThisPeriodTotalUserTime, ThisPeriodTotalKernelTime;
        public uint TotalPageFaultCount, TotalProcesses, ActiveProcesses, TotalTerminatedProcesses;
    }
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr CreateJobObject(IntPtr attributes, string name);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool SetInformationJobObject(IntPtr job, int type, ref ExtendedLimits limits, uint size);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool TerminateJobObject(IntPtr job, uint exitCode);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool QueryInformationJobObject(IntPtr job, int type, out Accounting accounting, uint size, out uint returned);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    IntPtr handle;
    static void Check(bool ok) { if (!ok) throw new Win32Exception(Marshal.GetLastWin32Error()); }
    public DeductumCiJob() {
        handle = CreateJobObject(IntPtr.Zero, null);
        if (handle == IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error());
        try {
            var limits = new ExtendedLimits();
            limits.BasicLimitInformation.LimitFlags = 0x2000; // KILL_ON_JOB_CLOSE, no breakaway.
            Check(SetInformationJobObject(handle, 9, ref limits, (uint)Marshal.SizeOf<ExtendedLimits>()));
        } catch { Dispose(); throw; }
    }
    public void Assign(Process process) { Check(AssignProcessToJobObject(handle, process.Handle)); }
    public void Stop() {
        Check(TerminateJobObject(handle, 1));
        var deadline = DateTime.UtcNow.AddSeconds(15);
        while (true) {
            Accounting info; uint returned;
            Check(QueryInformationJobObject(handle, 1, out info, (uint)Marshal.SizeOf<Accounting>(), out returned));
            if (info.ActiveProcesses == 0) return;
            if (DateTime.UtcNow >= deadline) throw new TimeoutException("Owned native CI processes did not stop");
            Thread.Sleep(50);
        }
    }
    public void Dispose() { if (handle != IntPtr.Zero) { CloseHandle(handle); handle = IntPtr.Zero; } }
}
// CreateProcessWithLogonW requires the target user to access the inherited
// window station and desktop. This lease restores their exact DACL bytes.
public sealed class DeductumCiDesktopAccess : IDisposable {
    [DllImport("user32.dll")] static extern IntPtr GetProcessWindowStation();
    [DllImport("user32.dll")] static extern IntPtr GetThreadDesktop(uint thread);
    [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
    [DllImport("user32.dll", SetLastError=true)] static extern bool GetUserObjectSecurity(IntPtr handle, ref uint info, byte[] descriptor, uint length, out uint needed);
    [DllImport("user32.dll", SetLastError=true)] static extern bool SetUserObjectSecurity(IntPtr handle, ref uint info, byte[] descriptor);
    IntPtr station, desktop;
    byte[] stationAcl, desktopAcl;
    bool restored;
    static void Check(bool ok) { if (!ok) throw new Win32Exception(Marshal.GetLastWin32Error()); }
    static byte[] Read(IntPtr handle) {
        uint info=4, needed;
        GetUserObjectSecurity(handle, ref info, null, 0, out needed);
        if (needed==0) throw new Win32Exception(Marshal.GetLastWin32Error());
        var bytes=new byte[needed];
        Check(GetUserObjectSecurity(handle, ref info, bytes, needed, out needed));
        return bytes;
    }
    static void Write(IntPtr handle, byte[] bytes) { uint info=4; Check(SetUserObjectSecurity(handle, ref info, bytes)); }
    static void Grant(IntPtr handle, byte[] bytes, string user, int mask) {
        var descriptor=new System.Security.AccessControl.RawSecurityDescriptor(bytes,0);
        if (descriptor.DiscretionaryAcl==null) return; // Existing null DACL already allows access.
        descriptor.DiscretionaryAcl.InsertAce(descriptor.DiscretionaryAcl.Count,
            new System.Security.AccessControl.CommonAce(System.Security.AccessControl.AceFlags.None,
            System.Security.AccessControl.AceQualifier.AccessAllowed, mask,
            new System.Security.Principal.SecurityIdentifier(user),false,null));
        var updated=new byte[descriptor.BinaryLength]; descriptor.GetBinaryForm(updated,0); Write(handle,updated);
    }
    static void Restore(IntPtr handle, byte[] bytes) {
        if (bytes==null) return;
        Write(handle,bytes);
        var actual=Read(handle);
        if (actual.Length!=bytes.Length) throw new InvalidOperationException("CI desktop DACL restoration changed length");
        for(int i=0;i<bytes.Length;i++) if(bytes[i]!=actual[i]) throw new InvalidOperationException("CI desktop DACL restoration changed bytes");
    }
    public DeductumCiDesktopAccess(string user) {
        station=GetProcessWindowStation(); desktop=GetThreadDesktop(GetCurrentThreadId());
        stationAcl=Read(station); desktopAcl=Read(desktop);
        try { Grant(station,stationAcl,user,0x000f037f); Grant(desktop,desktopAcl,user,0x000f01ff); }
        catch { Dispose(); throw; }
    }
    public void Dispose() {
        if (restored) return;
        try { Restore(desktop,desktopAcl); } finally { Restore(station,stationAcl); }
        restored=true;
    }
}
'@

function New-PrivatePassword {
    $bytes = [byte[]]::new(32)
    [Security.Cryptography.RandomNumberGenerator]::Fill($bytes)
    return 'Aa1!' + [Convert]::ToBase64String($bytes)
}

function Set-PrivateDirectoryAcl([string]$Path, [Security.Principal.SecurityIdentifier]$Owner,
                                 [Security.Principal.SecurityIdentifier]$Reader,
                                 [switch]$Writable) {
    $acl = [Security.AccessControl.DirectorySecurity]::new()
    $acl.SetAccessRuleProtection($true, $false)
    $acl.SetOwner($Owner)
    $inheritance = [Security.AccessControl.InheritanceFlags]'ContainerInherit, ObjectInherit'
    $propagation = [Security.AccessControl.PropagationFlags]::None
    $allow = [Security.AccessControl.AccessControlType]::Allow
    foreach ($sid in @($identity.User, [Security.Principal.SecurityIdentifier]::new('S-1-5-18'))) {
        $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($sid, 'FullControl', $inheritance, $propagation, $allow))
    }
    $rights = if ($Writable) { 'FullControl' } else { 'ReadAndExecute' }
    $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($Reader, $rights, $inheritance, $propagation, $allow))
    Set-Acl -LiteralPath $Path -AclObject $acl
}

function Copy-InputTree([string]$From, [string]$To) {
    & robocopy.exe $From $To /E /COPY:DAT /DCOPY:DAT /R:1 /W:1 /NFL /NDL /NJH /NJS /NP /XJ /XF .git /XD .git target dist .local-design .flatpak-builder build-flatpak
    if ($LASTEXITCODE -gt 7) { throw "Could not copy CI inputs (robocopy exit $LASTEXITCODE)" }
}

try {
    if (Test-Path -LiteralPath $root) { throw 'Disposable CI directory already exists' }
    New-Item -ItemType Directory -Path $root | Out-Null
    $createdRoot = $true
    $testPassword = New-PrivatePassword
    $otherPassword = New-PrivatePassword
    # GitHub consumes these workflow commands; neither password is written to a file.
    if ($env:GITHUB_ACTIONS -eq 'true') {
        Write-Output "::add-mask::$testPassword"
        Write-Output "::add-mask::$otherPassword"
    }
    $securePassword = ConvertTo-SecureString $testPassword -AsPlainText -Force
    $testUser = New-LocalUser -Name $testName -Password $securePassword -AccountNeverExpires -PasswordNeverExpires -UserMayNotChangePassword
    $createdUsers.Add($testName)
    $createdSids[$testName] = $testUser.SID.Value
    $otherUser = New-LocalUser -Name $otherName -Password (ConvertTo-SecureString $otherPassword -AsPlainText -Force) -AccountNeverExpires -PasswordNeverExpires -UserMayNotChangePassword
    $createdUsers.Add($otherName)
    $createdSids[$otherName] = $otherUser.SID.Value
    $usersGroup = Get-LocalGroup -SID 'S-1-5-32-545'
    Add-LocalGroupMember -Group $usersGroup -Member $testUser, $otherUser
    $testSid = $testUser.SID
    Set-PrivateDirectoryAcl $root $identity.User $testSid
    $inputs = Join-Path $root 'inputs'
    $workParent = Join-Path $root 'workspace'
    $work = Join-Path $workParent 'work'
    New-Item -ItemType Directory -Path $inputs, $workParent | Out-Null
    Set-PrivateDirectoryAcl $workParent $identity.User $testSid -Writable
    # The standard child creates work itself, so its token owns every test root.
    # Assigning another user's ownership here would require extra host privileges.
    Copy-InputTree $SourceRoot (Join-Path $inputs 'source')
    Copy-InputTree $ArtifactRoot (Join-Path $inputs 'artifact')
    Copy-InputTree $ProverCacheRoot (Join-Path $inputs 'provers')

    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = Join-Path $PSHOME 'pwsh.exe'
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.LoadUserProfile = $true
    $start.UserName = $testName
    $start.Domain = $env:COMPUTERNAME
    $start.Password = $securePassword
    $start.WorkingDirectory = $inputs
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.StandardOutputEncoding = [Text.UTF8Encoding]::new($false)
    $start.StandardErrorEncoding = [Text.UTF8Encoding]::new($false)
    foreach ($argument in @('-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File',
                            (Join-Path $inputs 'source/scripts/windows-native-ci-user.ps1'), '-Inputs', $inputs, '-Work', $work,
                            '-ReleasePath', (Join-Path $inputs 'contained.ready'), '-Gate', $Gate)) {
        $start.ArgumentList.Add($argument)
    }
    # Do not pass runner credentials, GITHUB_ENV, or the administrator's profile.
    $start.Environment.Clear()
    foreach ($name in @('SystemRoot', 'SystemDrive', 'WINDIR', 'ComSpec', 'ProgramFiles', 'ProgramFiles(x86)', 'ProgramW6432', 'ProgramData', 'ALLUSERSPROFILE', 'NUMBER_OF_PROCESSORS', 'PROCESSOR_ARCHITECTURE', 'PATHEXT', 'OS', 'COMPUTERNAME', 'CommonProgramFiles', 'CommonProgramFiles(x86)', 'CommonProgramW6432')) {
        if ([Environment]::GetEnvironmentVariable($name)) { $start.Environment[$name] = [Environment]::GetEnvironmentVariable($name) }
    }
    $toolDirectories = @((Split-Path (Get-Command git.exe -ErrorAction Stop).Source),
                         (Split-Path (Get-Command 7z.exe -ErrorAction Stop).Source), $PSHOME)
    $start.Environment['PATH'] = ($toolDirectories -join ';') + ';' + [Environment]::GetEnvironmentVariable('Path', 'Machine')
    $start.Environment['USERNAME'] = $testName
    $start.Environment['USERDOMAIN'] = $env:COMPUTERNAME
    $start.Environment['DEDUCTUM_CI_EXPECTED_SID'] = $testSid.Value
    $start.Environment['DEDUCTUM_TEST_OTHER_USER'] = $otherName
    $start.Environment['DEDUCTUM_TEST_OTHER_PASSWORD'] = $otherPassword
    $desktopAccess = [DeductumCiDesktopAccess]::new($testSid.Value)
    $job = [DeductumCiJob]::new()
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $start
    if (-not $process.Start()) { throw 'Could not start standard-user verification' }
    $processStarted = $true
    $job.Assign($process)
    New-Item -ItemType File -Path (Join-Path $inputs 'contained.ready') | Out-Null
    $deadline = [DateTime]::UtcNow.AddMinutes($TimeoutMinutes)
    $heartbeat = [DateTime]::UtcNow.AddSeconds(30)
    $output = $process.StandardOutput.ReadLineAsync()
    $errorOutput = $process.StandardError.ReadLineAsync()
    $outputEnded = $false
    $errorEnded = $false
    while (-not $process.HasExited -or -not $outputEnded -or -not $errorEnded) {
        if (-not $outputEnded -and $output.IsCompleted) {
            $line = $output.GetAwaiter().GetResult()
            if ($null -eq $line) { $outputEnded = $true } else { Write-Output $line; $output = $process.StandardOutput.ReadLineAsync() }
        }
        if (-not $errorEnded -and $errorOutput.IsCompleted) {
            $line = $errorOutput.GetAwaiter().GetResult()
            if ($null -eq $line) { $errorEnded = $true } else { Write-Output $line; $errorOutput = $process.StandardError.ReadLineAsync() }
        }
        if ([DateTime]::UtcNow -gt $deadline) { throw 'Standard-user Windows verification timed out' }
        if ([DateTime]::UtcNow -gt $heartbeat) { Write-Output 'Standard-user Windows verification is running'; $heartbeat = [DateTime]::UtcNow.AddSeconds(30) }
        Start-Sleep -Milliseconds 20
    }
    $process.WaitForExit()
    if ($process.ExitCode -ne 0) { throw "Standard-user Windows verification failed (exit $($process.ExitCode))" }
} finally {
    $containedStopped = $true
    if ($job) {
        try { $job.Stop() }
        catch { $containedStopped = $false; Write-Warning "Could not finish private CI containment: $($_.Exception.Message)" }
        finally { $job.Dispose() }
    }
    if ($process) {
        try {
            # A failed assignment leaves only the trusted, bounded startup guard.
            if ($processStarted -and -not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
        } catch { Write-Warning "Could not stop the private CI child: $($_.Exception.Message)" }
        finally { $process.Dispose() }
    }
    if ($containedStopped -and $desktopAccess) {
        try { $desktopAccess.Dispose(); Write-Output 'Native CI window-station and desktop DACLs restored byte-for-byte' }
        catch { $containedStopped = $false; Write-Warning "Could not restore private CI desktop access: $($_.Exception.Message)" }
    }
    if ($containedStopped) {
        foreach ($name in $createdUsers) {
            $sid = $createdSids[$name]
            Get-CimInstance Win32_UserProfile -Filter "SID='$sid'" -ErrorAction SilentlyContinue |
                Remove-CimInstance -ErrorAction SilentlyContinue
            Remove-LocalUser -Name $name -ErrorAction SilentlyContinue
        }
        if ($createdRoot -and (Test-Path -LiteralPath $root)) { Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue }
    } else {
        throw 'Private CI process cleanup failed; accounts and directories were retained on the disposable runner'
    }
}

# GitHub appends exit $LASTEXITCODE; robocopy success includes nonzero codes.
$global:LASTEXITCODE = 0
