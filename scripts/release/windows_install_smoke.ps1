param(
    [Parameter(Mandatory = $true)]
    [string] $Installer
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$installerPath = (Resolve-Path -LiteralPath $Installer).Path
$installRoot = Join-Path $env:LOCALAPPDATA 'PokeCon Controller'
$application = Join-Path $installRoot 'pokecon.exe'
$uninstaller = Join-Path $installRoot 'uninstall.exe'
$resourceManifest = Join-Path $installRoot 'resource-manifest.json'
$dataRoot = Join-Path $env:LOCALAPPDATA 'pokecon\data'
$sentinel = Join-Path $dataRoot 'package-smoke-sentinel'
$desktopWindowTitle = 'PokeCon Controller'
$desktopWindowTimeoutSeconds = 60
$desktopTerminationTimeoutSeconds = 15

if (Test-Path -LiteralPath $installRoot) {
    throw "Refusing to overwrite an existing installation: $installRoot"
}

function Invoke-CheckedProcess {
    param(
        [Parameter(Mandatory = $true)]
        [string] $FilePath,
        [string[]] $ArgumentList = @()
    )

    $process = Start-Process `
        -FilePath $FilePath `
        -ArgumentList $ArgumentList `
        -NoNewWindow `
        -PassThru `
        -Wait
    if ($process.ExitCode -ne 0) {
        throw "$FilePath exited with code $($process.ExitCode)"
    }
}

function Get-InstalledApplicationHash {
    if (-not (Test-Path -LiteralPath $application -PathType Leaf)) {
        throw "Installed application is missing: $application"
    }
    return ((Get-FileHash -LiteralPath $application -Algorithm SHA256).Hash).ToLowerInvariant()
}

function Assert-InstalledApplicationHash {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Expected,
        [Parameter(Mandatory = $true)]
        [string] $Probe
    )

    $actual = Get-InstalledApplicationHash
    if ($actual -cne $Expected) {
        throw (
            "Installed application changed during the $Probe probe: " +
            "expected SHA-256 $Expected, found $actual at $application"
        )
    }
}

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using Microsoft.Win32.SafeHandles;

namespace PokeConSmoke
{
    internal sealed class SafeJobHandle : SafeHandleZeroOrMinusOneIsInvalid
    {
        public SafeJobHandle() : base(true) { }

        protected override bool ReleaseHandle()
        {
            return NativeMethods.CloseHandle(handle);
        }
    }

    internal static class NativeMethods
    {
        internal const uint CREATE_SUSPENDED = 0x00000004;
        internal const uint JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x00002000;
        internal const int JobObjectBasicAccountingInformationClass = 1;
        internal const int JobObjectBasicProcessIdListClass = 3;
        internal const int JobObjectExtendedLimitInformationClass = 9;
        internal const int ERROR_MORE_DATA = 234;
        internal const uint WAIT_OBJECT_0 = 0;
        internal const uint WAIT_TIMEOUT = 258;
        internal const uint WAIT_FAILED = 0xffffffff;

        [StructLayout(LayoutKind.Sequential)]
        internal struct IoCounters
        {
            internal ulong ReadOperationCount;
            internal ulong WriteOperationCount;
            internal ulong OtherOperationCount;
            internal ulong ReadTransferCount;
            internal ulong WriteTransferCount;
            internal ulong OtherTransferCount;
        }

        [StructLayout(LayoutKind.Sequential)]
        internal struct JobObjectBasicLimitInformation
        {
            internal long PerProcessUserTimeLimit;
            internal long PerJobUserTimeLimit;
            internal uint LimitFlags;
            internal UIntPtr MinimumWorkingSetSize;
            internal UIntPtr MaximumWorkingSetSize;
            internal uint ActiveProcessLimit;
            internal UIntPtr Affinity;
            internal uint PriorityClass;
            internal uint SchedulingClass;
        }

        [StructLayout(LayoutKind.Sequential)]
        internal struct JobObjectExtendedLimitInformation
        {
            internal JobObjectBasicLimitInformation BasicLimitInformation;
            internal IoCounters IoInfo;
            internal UIntPtr ProcessMemoryLimit;
            internal UIntPtr JobMemoryLimit;
            internal UIntPtr PeakProcessMemoryUsed;
            internal UIntPtr PeakJobMemoryUsed;
        }

        [StructLayout(LayoutKind.Sequential)]
        internal struct JobObjectBasicAccountingInformation
        {
            internal long TotalUserTime;
            internal long TotalKernelTime;
            internal long ThisPeriodTotalUserTime;
            internal long ThisPeriodTotalKernelTime;
            internal uint TotalPageFaultCount;
            internal uint TotalProcesses;
            internal uint ActiveProcesses;
            internal uint TotalTerminatedProcesses;
        }

        [StructLayout(LayoutKind.Sequential)]
        internal struct StartupInfo
        {
            internal uint cb;
            internal IntPtr lpReserved;
            internal IntPtr lpDesktop;
            internal IntPtr lpTitle;
            internal uint dwX;
            internal uint dwY;
            internal uint dwXSize;
            internal uint dwYSize;
            internal uint dwXCountChars;
            internal uint dwYCountChars;
            internal uint dwFillAttribute;
            internal uint dwFlags;
            internal ushort wShowWindow;
            internal ushort cbReserved2;
            internal IntPtr lpReserved2;
            internal IntPtr hStdInput;
            internal IntPtr hStdOutput;
            internal IntPtr hStdError;
        }

        [StructLayout(LayoutKind.Sequential)]
        internal struct ProcessInformation
        {
            internal IntPtr hProcess;
            internal IntPtr hThread;
            internal uint dwProcessId;
            internal uint dwThreadId;
        }

        internal delegate bool EnumWindowsProc(IntPtr window, IntPtr parameter);

        [DllImport(
            "kernel32.dll",
            CharSet = CharSet.Unicode,
            ExactSpelling = true,
            SetLastError = true)]
        internal static extern SafeJobHandle CreateJobObjectW(
            IntPtr jobAttributes,
            string name);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool SetInformationJobObject(
            SafeJobHandle job,
            int informationClass,
            ref JobObjectExtendedLimitInformation information,
            uint informationLength);

        [DllImport(
            "kernel32.dll",
            CharSet = CharSet.Unicode,
            ExactSpelling = true,
            SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool CreateProcessW(
            string applicationName,
            StringBuilder commandLine,
            IntPtr processAttributes,
            IntPtr threadAttributes,
            [MarshalAs(UnmanagedType.Bool)] bool inheritHandles,
            uint creationFlags,
            IntPtr environment,
            string currentDirectory,
            ref StartupInfo startupInfo,
            out ProcessInformation processInformation);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool AssignProcessToJobObject(
            SafeJobHandle job,
            IntPtr process);

        [DllImport("kernel32.dll", SetLastError = true)]
        internal static extern uint ResumeThread(IntPtr thread);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool TerminateJobObject(
            SafeJobHandle job,
            uint exitCode);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool QueryInformationJobObject(
            SafeJobHandle job,
            int informationClass,
            out JobObjectBasicAccountingInformation information,
            uint informationLength,
            IntPtr returnLength);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool QueryInformationJobObject(
            SafeJobHandle job,
            int informationClass,
            IntPtr information,
            uint informationLength,
            IntPtr returnLength);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool TerminateProcess(IntPtr process, uint exitCode);

        [DllImport("kernel32.dll", SetLastError = true)]
        internal static extern uint WaitForSingleObject(
            IntPtr handle,
            uint milliseconds);

        [DllImport("user32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool EnumWindows(
            EnumWindowsProc callback,
            IntPtr parameter);

        [DllImport("user32.dll")]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool IsWindowVisible(IntPtr window);

        [DllImport("user32.dll", SetLastError = true)]
        internal static extern uint GetWindowThreadProcessId(
            IntPtr window,
            out uint processId);

        [DllImport(
            "user32.dll",
            CharSet = CharSet.Unicode,
            ExactSpelling = true,
            SetLastError = true)]
        internal static extern int GetWindowTextLengthW(IntPtr window);

        [DllImport(
            "user32.dll",
            CharSet = CharSet.Unicode,
            ExactSpelling = true,
            SetLastError = true)]
        internal static extern int GetWindowTextW(
            IntPtr window,
            StringBuilder title,
            int maxCount);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool CloseHandle(IntPtr handle);

        internal static Win32Exception LastError(string operation)
        {
            return new Win32Exception(Marshal.GetLastWin32Error(), operation);
        }
    }

    public sealed class VisibleWindowObservation
    {
        internal VisibleWindowObservation(
            long matchingHandle,
            int matchingCount,
            string visibleWindows)
        {
            MatchingHandle = matchingHandle;
            MatchingCount = matchingCount;
            VisibleWindows = visibleWindows;
        }

        public long MatchingHandle { get; private set; }

        public int MatchingCount { get; private set; }

        public string VisibleWindows { get; private set; }
    }

    public sealed class WindowsJobProcess : IDisposable
    {
        private SafeJobHandle job;
        private bool disposed;

        private WindowsJobProcess(
            SafeJobHandle jobHandle,
            Process rootProcess)
        {
            job = jobHandle;
            RootProcess = rootProcess;
        }

        public Process RootProcess { get; private set; }

        public static WindowsJobProcess StartDesktop(
            string executable,
            int cleanupTimeoutMilliseconds)
        {
            if (String.IsNullOrWhiteSpace(executable))
            {
                throw new ArgumentException("Executable path must not be empty.", "executable");
            }
            if (!File.Exists(executable))
            {
                throw new FileNotFoundException("Desktop executable is missing.", executable);
            }
            if (cleanupTimeoutMilliseconds <= 0)
            {
                throw new ArgumentOutOfRangeException(
                    "cleanupTimeoutMilliseconds",
                    "Cleanup timeout must be positive.");
            }

            SafeJobHandle jobHandle = NativeMethods.CreateJobObjectW(
                IntPtr.Zero,
                null);
            if (jobHandle == null || jobHandle.IsInvalid)
            {
                throw NativeMethods.LastError("CreateJobObjectW failed");
            }

            NativeMethods.ProcessInformation processInformation =
                new NativeMethods.ProcessInformation();
            Process rootProcess = null;
            bool assignedToJob = false;
            try
            {
                NativeMethods.JobObjectExtendedLimitInformation limitInformation =
                    new NativeMethods.JobObjectExtendedLimitInformation();
                limitInformation.BasicLimitInformation.LimitFlags =
                    NativeMethods.JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                if (!NativeMethods.SetInformationJobObject(
                        jobHandle,
                        NativeMethods.JobObjectExtendedLimitInformationClass,
                        ref limitInformation,
                        (uint)Marshal.SizeOf<NativeMethods.JobObjectExtendedLimitInformation>()))
                {
                    throw NativeMethods.LastError("SetInformationJobObject failed");
                }

                NativeMethods.StartupInfo startupInfo = new NativeMethods.StartupInfo();
                startupInfo.cb = (uint)Marshal.SizeOf<NativeMethods.StartupInfo>();
                StringBuilder commandLine = new StringBuilder(
                    "\"" + executable + "\" --ui desktop");
                if (!NativeMethods.CreateProcessW(
                        executable,
                        commandLine,
                        IntPtr.Zero,
                        IntPtr.Zero,
                        false,
                        NativeMethods.CREATE_SUSPENDED,
                        IntPtr.Zero,
                        null,
                        ref startupInfo,
                        out processInformation))
                {
                    throw NativeMethods.LastError("CreateProcessW failed");
                }

                if (!NativeMethods.AssignProcessToJobObject(
                        jobHandle,
                        processInformation.hProcess))
                {
                    throw NativeMethods.LastError("AssignProcessToJobObject failed");
                }
                assignedToJob = true;
                rootProcess = Process.GetProcessById((int)processInformation.dwProcessId);
                if (rootProcess.Handle == IntPtr.Zero)
                {
                    throw new InvalidOperationException(
                        "The managed root process handle was invalid.");
                }

                uint previousSuspendCount = NativeMethods.ResumeThread(
                    processInformation.hThread);
                if (previousSuspendCount == UInt32.MaxValue)
                {
                    throw NativeMethods.LastError("ResumeThread failed");
                }
                if (previousSuspendCount != 1)
                {
                    throw new InvalidOperationException(
                        "ResumeThread returned an unexpected suspend count: " +
                        previousSuspendCount);
                }

                WindowsJobProcess result = new WindowsJobProcess(jobHandle, rootProcess);
                jobHandle = null;
                rootProcess = null;
                return result;
            }
            catch (Exception launchFailure)
            {
                List<Exception> cleanupFailures = new List<Exception>();
                Stopwatch cleanupStopwatch = Stopwatch.StartNew();
                bool queryAssignedJob = false;
                if (processInformation.hProcess != IntPtr.Zero)
                {
                    if (assignedToJob)
                    {
                        if (NativeMethods.TerminateJobObject(jobHandle, 1))
                        {
                            queryAssignedJob = true;
                        }
                        else
                        {
                            cleanupFailures.Add(NativeMethods.LastError(
                                "TerminateJobObject failed during launch cleanup"));
                            try
                            {
                                jobHandle.Dispose();
                                jobHandle = null;
                            }
                            catch (Exception error)
                            {
                                cleanupFailures.Add(error);
                            }
                        }
                    }
                    else if (!NativeMethods.TerminateProcess(processInformation.hProcess, 1))
                    {
                        cleanupFailures.Add(NativeMethods.LastError(
                            "TerminateProcess failed during launch cleanup"));
                    }

                    if (processInformation.hThread != IntPtr.Zero)
                    {
                        if (NativeMethods.CloseHandle(processInformation.hThread))
                        {
                            processInformation.hThread = IntPtr.Zero;
                        }
                        else
                        {
                            cleanupFailures.Add(NativeMethods.LastError(
                                "CloseHandle failed for the primary thread"));
                        }
                    }
                    if (rootProcess != null)
                    {
                        try
                        {
                            rootProcess.Dispose();
                            rootProcess = null;
                        }
                        catch (Exception error)
                        {
                            cleanupFailures.Add(error);
                        }
                    }

                    uint waitResult = NativeMethods.WaitForSingleObject(
                        processInformation.hProcess,
                        GetRemainingCleanupMilliseconds(
                            cleanupStopwatch,
                            cleanupTimeoutMilliseconds));
                    if (waitResult == NativeMethods.WAIT_OBJECT_0 &&
                        cleanupStopwatch.ElapsedMilliseconds > cleanupTimeoutMilliseconds)
                    {
                        cleanupFailures.Add(new TimeoutException(
                            "The partial desktop root terminated after the cleanup deadline."));
                    }
                    else if (waitResult == NativeMethods.WAIT_FAILED)
                    {
                        cleanupFailures.Add(NativeMethods.LastError(
                            "WaitForSingleObject failed during launch cleanup"));
                    }
                    else if (waitResult == NativeMethods.WAIT_TIMEOUT)
                    {
                        cleanupFailures.Add(new TimeoutException(
                            "The partial desktop root did not terminate within the cleanup deadline."));
                    }
                    else if (waitResult != NativeMethods.WAIT_OBJECT_0)
                    {
                        cleanupFailures.Add(new InvalidOperationException(
                            "WaitForSingleObject returned unexpected status " + waitResult));
                    }

                    if (NativeMethods.CloseHandle(processInformation.hProcess))
                    {
                        processInformation.hProcess = IntPtr.Zero;
                    }
                    else
                    {
                        cleanupFailures.Add(NativeMethods.LastError(
                            "CloseHandle failed for the root process"));
                    }

                    if (queryAssignedJob)
                    {
                        uint activeProcesses = UInt32.MaxValue;
                        bool jobDrained = false;
                        while (cleanupStopwatch.ElapsedMilliseconds <=
                            cleanupTimeoutMilliseconds)
                        {
                            try
                            {
                                activeProcesses = QueryActiveProcessCount(jobHandle);
                            }
                            catch (Exception error)
                            {
                                cleanupFailures.Add(error);
                                break;
                            }
                            if (activeProcesses == 0 &&
                                cleanupStopwatch.ElapsedMilliseconds <=
                                    cleanupTimeoutMilliseconds)
                            {
                                jobDrained = true;
                                break;
                            }
                            if (activeProcesses == 0)
                            {
                                break;
                            }
                            uint remaining = GetRemainingCleanupMilliseconds(
                                cleanupStopwatch,
                                cleanupTimeoutMilliseconds);
                            if (remaining == 0)
                            {
                                break;
                            }
                            Thread.Sleep((int)Math.Min(100L, (long)remaining));
                        }
                        if (!jobDrained)
                        {
                            cleanupFailures.Add(new TimeoutException(
                                "The partial desktop job did not drain before the cleanup deadline; " +
                                "last ActiveProcesses=" + activeProcesses));
                        }
                    }

                    if (assignedToJob && jobHandle != null)
                    {
                        try
                        {
                            jobHandle.Dispose();
                            jobHandle = null;
                        }
                        catch (Exception error)
                        {
                            cleanupFailures.Add(error);
                        }
                    }
                }
                if (cleanupFailures.Count != 0)
                {
                    cleanupFailures.Insert(0, launchFailure);
                    throw new AggregateException(
                        "Desktop launch and cleanup both failed.",
                        cleanupFailures);
                }
                throw;
            }
            finally
            {
                if (processInformation.hThread != IntPtr.Zero)
                {
                    NativeMethods.CloseHandle(processInformation.hThread);
                }
                if (processInformation.hProcess != IntPtr.Zero)
                {
                    NativeMethods.CloseHandle(processInformation.hProcess);
                }
                try
                {
                    if (rootProcess != null)
                    {
                        rootProcess.Dispose();
                    }
                }
                finally
                {
                    if (jobHandle != null)
                    {
                        jobHandle.Dispose();
                    }
                }
            }
        }

        private static uint GetRemainingCleanupMilliseconds(
            Stopwatch stopwatch,
            int timeoutMilliseconds)
        {
            long remaining = timeoutMilliseconds - stopwatch.ElapsedMilliseconds;
            return remaining > 0 ? (uint)remaining : 0;
        }

        private static uint QueryActiveProcessCount(SafeJobHandle jobHandle)
        {
            NativeMethods.JobObjectBasicAccountingInformation information;
            if (!NativeMethods.QueryInformationJobObject(
                    jobHandle,
                    NativeMethods.JobObjectBasicAccountingInformationClass,
                    out information,
                    (uint)Marshal.SizeOf<NativeMethods.JobObjectBasicAccountingInformation>(),
                    IntPtr.Zero))
            {
                throw NativeMethods.LastError(
                    "QueryInformationJobObject accounting failed");
            }
            return information.ActiveProcesses;
        }

        public VisibleWindowObservation ObserveVisibleTopLevelWindows(string exactTitle)
        {
            ThrowIfDisposed();
            if (exactTitle == null)
            {
                throw new ArgumentNullException("exactTitle");
            }

            uint rootProcessId = unchecked((uint)RootProcess.Id);
            List<string> visibleWindows = new List<string>();
            long matchingHandle = 0;
            int matchingCount = 0;
            NativeMethods.EnumWindowsProc callback = delegate(IntPtr window, IntPtr parameter)
            {
                uint windowProcessId;
                if (!NativeMethods.IsWindowVisible(window) ||
                    NativeMethods.GetWindowThreadProcessId(window, out windowProcessId) == 0 ||
                    windowProcessId != rootProcessId)
                {
                    return true;
                }

                int titleLength = NativeMethods.GetWindowTextLengthW(window);
                StringBuilder titleBuffer = new StringBuilder(Math.Max(1, titleLength + 1));
                NativeMethods.GetWindowTextW(window, titleBuffer, titleBuffer.Capacity);
                string title = titleBuffer.ToString();
                visibleWindows.Add(
                    "handle=0x" + window.ToInt64().ToString("X") + " title=[" + title + "]");
                if (String.Equals(title, exactTitle, StringComparison.Ordinal))
                {
                    matchingHandle = window.ToInt64();
                    ++matchingCount;
                }
                return true;
            };
            if (!NativeMethods.EnumWindows(callback, IntPtr.Zero))
            {
                throw NativeMethods.LastError("EnumWindows failed");
            }

            string summary = visibleWindows.Count == 0
                ? "<none>"
                : String.Join(", ", visibleWindows.ToArray());
            return new VisibleWindowObservation(matchingHandle, matchingCount, summary);
        }

        public void Terminate(uint exitCode)
        {
            SafeJobHandle jobHandle = GetOpenJobHandle();
            if (!NativeMethods.TerminateJobObject(jobHandle, exitCode))
            {
                throw NativeMethods.LastError("TerminateJobObject failed");
            }
        }

        public void CloseJobForKill()
        {
            ThrowIfDisposed();
            CloseJobHandle();
        }

        public uint GetActiveProcessCount()
        {
            return QueryActiveProcessCount(GetOpenJobHandle());
        }

        public long[] GetActiveProcessIds()
        {
            SafeJobHandle jobHandle = GetOpenJobHandle();
            int capacity = 16;
            for (int attempt = 0; attempt < 8; ++attempt)
            {
                int bufferLength = checked(8 + (capacity * IntPtr.Size));
                IntPtr buffer = Marshal.AllocHGlobal(bufferLength);
                try
                {
                    bool succeeded = NativeMethods.QueryInformationJobObject(
                        jobHandle,
                        NativeMethods.JobObjectBasicProcessIdListClass,
                        buffer,
                        (uint)bufferLength,
                        IntPtr.Zero);
                    int error = succeeded ? 0 : Marshal.GetLastWin32Error();
                    uint assigned = unchecked((uint)Marshal.ReadInt32(buffer, 0));
                    uint returned = unchecked((uint)Marshal.ReadInt32(buffer, 4));
                    if (succeeded)
                    {
                        if (returned > capacity)
                        {
                            throw new InvalidOperationException(
                                "Job process ID query returned more entries than allocated.");
                        }
                        int returnedCount = checked((int)returned);
                        long[] processIds = new long[returnedCount];
                        for (int index = 0; index < returnedCount; ++index)
                        {
                            int offset = 8 + (index * IntPtr.Size);
                            processIds[index] = IntPtr.Size == 8
                                ? Marshal.ReadInt64(buffer, offset)
                                : unchecked((uint)Marshal.ReadInt32(buffer, offset));
                        }
                        return processIds;
                    }
                    if (error != NativeMethods.ERROR_MORE_DATA)
                    {
                        throw new Win32Exception(
                            error,
                            "QueryInformationJobObject process IDs failed");
                    }
                    capacity = Math.Max(capacity * 2, checked((int)assigned));
                }
                finally
                {
                    Marshal.FreeHGlobal(buffer);
                }
            }
            throw new InvalidOperationException(
                "Job process ID list kept growing during diagnostics.");
        }

        public void Dispose()
        {
            if (disposed)
            {
                return;
            }
            disposed = true;
            try
            {
                RootProcess.Dispose();
            }
            finally
            {
                CloseJobHandle();
            }
        }

        private SafeJobHandle GetOpenJobHandle()
        {
            ThrowIfDisposed();
            if (job == null)
            {
                throw new InvalidOperationException("The desktop job handle is closed.");
            }
            return job;
        }

        private void CloseJobHandle()
        {
            if (job != null)
            {
                job.Dispose();
                job = null;
            }
        }

        private void ThrowIfDisposed()
        {
            if (disposed)
            {
                throw new ObjectDisposedException("WindowsJobProcess");
            }
        }
    }
}
'@

function Stop-DesktopProbeProcess {
    param(
        [Parameter(Mandatory = $true)]
        [PokeConSmoke.WindowsJobProcess] $JobProcess
    )

    $process = $JobProcess.RootProcess
    $processId = $process.Id
    $timeoutMilliseconds = $desktopTerminationTimeoutSeconds * 1000
    $terminationStopwatch = [Diagnostics.Stopwatch]::StartNew()
    $terminateFailure = $null
    $jobCloseFailure = $null
    $accountingFailure = $null
    $waitFailure = $null
    $rootDisposeFailure = $null
    $activeProcesses = $null
    $residualProcessIds = @()
    $jobDrainedWithinDeadline = $false
    $jobClosedForKill = $false
    $reaped = $false
    $rootReapedWithinDeadline = $false
    try {
        try {
            $JobProcess.Terminate(1)
        }
        catch {
            $terminateFailure = $_.Exception.ToString()
            try {
                # If explicit termination fails, close the job immediately so
                # KILL_ON_JOB_CLOSE runs before waiting for the retained root.
                $JobProcess.CloseJobForKill()
                $jobClosedForKill = $true
            }
            catch {
                $jobCloseFailure = $_.Exception.ToString()
            }
        }

        $remainingMilliseconds = [Int32][Math]::Max(
            0,
            $timeoutMilliseconds - $terminationStopwatch.ElapsedMilliseconds
        )
        try {
            $reaped = $process.WaitForExit($remainingMilliseconds)
            $rootReapedWithinDeadline = (
                $reaped -and
                $terminationStopwatch.ElapsedMilliseconds -le $timeoutMilliseconds
            )
        }
        catch {
            $waitFailure = $_.Exception.ToString()
        }
        try {
            # Job accounting can retain the terminated root until every open
            # process handle has been released.
            $process.Dispose()
        }
        catch {
            $rootDisposeFailure = $_.Exception.ToString()
        }

        if (-not $jobClosedForKill) {
            while ($terminationStopwatch.ElapsedMilliseconds -lt $timeoutMilliseconds) {
                try {
                    $activeProcesses = $JobProcess.GetActiveProcessCount()
                }
                catch {
                    $accountingFailure = $_.Exception.ToString()
                    break
                }
                if ($activeProcesses -eq 0) {
                    $jobDrainedWithinDeadline = (
                        $terminationStopwatch.ElapsedMilliseconds -le $timeoutMilliseconds
                    )
                    break
                }
                $sleepMilliseconds = [Int32][Math]::Min(
                    100,
                    $timeoutMilliseconds - $terminationStopwatch.ElapsedMilliseconds
                )
                if ($sleepMilliseconds -gt 0) {
                    Start-Sleep -Milliseconds $sleepMilliseconds
                }
            }
        }

        if (-not $jobClosedForKill) {
            try {
                $activeProcesses = $JobProcess.GetActiveProcessCount()
                if ($activeProcesses -ne 0) {
                    try {
                        $residualProcessIds = @($JobProcess.GetActiveProcessIds())
                    }
                    catch {
                        $residualProcessIds = @(
                            "PID query failed: $($_.Exception.ToString())"
                        )
                    }
                }
            }
            catch {
                $failure = $_.Exception.ToString()
                if ($null -eq $accountingFailure) {
                    $accountingFailure = $failure
                }
                else {
                    $accountingFailure += "`n$failure"
                }
            }
        }

        if (-not $jobDrainedWithinDeadline -or
            -not $rootReapedWithinDeadline -or
            $null -ne $terminateFailure -or
            $null -ne $jobCloseFailure -or
            $null -ne $accountingFailure -or
            $null -ne $waitFailure -or
            $null -ne $rootDisposeFailure) {
            $diagnostics = [Collections.Generic.List[string]]::new()
            if ($null -ne $activeProcesses -and $activeProcesses -ne 0) {
                $null = $diagnostics.Add(
                    "job ActiveProcesses=$activeProcesses; residual PIDs: " +
                    "[$($residualProcessIds -join ', ')]"
                )
            }
            if (-not $jobDrainedWithinDeadline) {
                $null = $diagnostics.Add(
                    'the job was not observed at ActiveProcesses=0 before the deadline'
                )
            }
            if ($jobClosedForKill) {
                $null = $diagnostics.Add(
                    'KILL_ON_JOB_CLOSE fallback ran; descendant quiescence is unverified'
                )
            }
            if (-not $rootReapedWithinDeadline) {
                $null = $diagnostics.Add(
                    "direct root pid=$processId was not reaped before the deadline"
                )
            }
            if ($null -ne $terminateFailure) {
                $null = $diagnostics.Add(
                    "TerminateJobObject failed: $terminateFailure"
                )
            }
            if ($null -ne $jobCloseFailure) {
                $null = $diagnostics.Add(
                    "KILL_ON_JOB_CLOSE fallback failed: $jobCloseFailure"
                )
            }
            if ($null -ne $accountingFailure) {
                $null = $diagnostics.Add(
                    "job accounting failed: $accountingFailure"
                )
            }
            if ($null -ne $waitFailure) {
                $null = $diagnostics.Add(
                    "direct root WaitForExit failed: $waitFailure"
                )
            }
            if ($null -ne $rootDisposeFailure) {
                $null = $diagnostics.Add(
                    "direct root Process.Dispose failed: $rootDisposeFailure"
                )
            }
            throw (
                "Desktop probe job for root $processId did not terminate " +
                "cleanly within the shared $desktopTerminationTimeoutSeconds-second deadline; " +
                ($diagnostics -join '; ')
            )
        }
    }
    finally {
        try {
            $terminationStopwatch.Stop()
        }
        finally {
            $JobProcess.Dispose()
        }
    }
}

function Invoke-DesktopWindowProbe {
    $jobProcess = [PokeConSmoke.WindowsJobProcess]::StartDesktop(
        $application,
        $desktopTerminationTimeoutSeconds * 1000
    )
    $process = $null

    try {
        $process = $jobProcess.RootProcess
        $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
        $lastVisibleWindows = '<not observed>'
        $observedWindow = $false
        while ($stopwatch.Elapsed.TotalSeconds -lt $desktopWindowTimeoutSeconds) {
            $process.Refresh()
            if ($process.HasExited) {
                throw (
                    "Desktop probe process $($process.Id) exited early with code " +
                    "$($process.ExitCode) after $([Math]::Round($stopwatch.Elapsed.TotalSeconds, 2)) seconds; " +
                    "last visible top-level windows=[$lastVisibleWindows]"
                )
            }

            $observation = $jobProcess.ObserveVisibleTopLevelWindows($desktopWindowTitle)
            $lastVisibleWindows = $observation.VisibleWindows
            if ($observation.MatchingCount -eq 1 -and $observation.MatchingHandle -ne 0) {
                $candidateHandle = $observation.MatchingHandle
                Start-Sleep -Milliseconds 200
                $process.Refresh()
                if (-not $process.HasExited) {
                    $confirmation = $jobProcess.ObserveVisibleTopLevelWindows(
                        $desktopWindowTitle
                    )
                    $lastVisibleWindows = $confirmation.VisibleWindows
                    if ($confirmation.MatchingCount -eq 1 -and
                        $confirmation.MatchingHandle -eq $candidateHandle) {
                        $observedWindow = $true
                        break
                    }
                }
            }
            else {
                Start-Sleep -Milliseconds 200
            }
        }

        if (-not $observedWindow) {
            throw (
                "Desktop probe process $($process.Id) did not expose a live native window " +
                "with exact title [$desktopWindowTitle] within $desktopWindowTimeoutSeconds seconds; " +
                "last visible top-level windows=[$lastVisibleWindows]"
            )
        }
    }
    catch {
        $probeFailure = $_.Exception.Message
        try {
            Stop-DesktopProbeProcess -JobProcess $jobProcess
        }
        catch {
            throw "$probeFailure; desktop probe cleanup also failed: $($_.Exception.Message)"
        }
        throw $probeFailure
    }

    Stop-DesktopProbeProcess -JobProcess $jobProcess
}

function Invoke-StartupProbe {
    if (-not (Test-Path -LiteralPath $application -PathType Leaf)) {
        throw "Installed application is missing: $application"
    }
    if (-not (Test-Path -LiteralPath $resourceManifest -PathType Leaf)) {
        $topLevelEntries = @(
            Get-ChildItem -LiteralPath $installRoot -Force |
                Sort-Object -Property Name |
                ForEach-Object { $_.Name }
        ) -join ', '
        $nestedManifests = @(
            Get-ChildItem `
                -LiteralPath $installRoot `
                -Filter 'resource-manifest.json' `
                -File `
                -Recurse `
                -ErrorAction SilentlyContinue |
                ForEach-Object { $_.FullName }
        ) -join ', '
        throw (
            "Installed resource manifest is missing: $resourceManifest; " +
            "top-level entries: [$topLevelEntries]; " +
            "nested manifests: [$nestedManifests]"
        )
    }
    $applicationHash = Get-InstalledApplicationHash
    # Preserve probe diagnostics on the host without adding them to this
    # function's success output, which must contain only the SHA-256 string.
    Invoke-CheckedProcess `
        -FilePath $application `
        -ArgumentList @('--ui', 'web', '--exit-after-startup') |
        Out-Host
    Assert-InstalledApplicationHash -Expected $applicationHash -Probe 'Web startup' |
        Out-Host
    Invoke-DesktopWindowProbe | Out-Host
    Assert-InstalledApplicationHash -Expected $applicationHash -Probe 'Desktop window' |
        Out-Host
    return $applicationHash
}

try {
    Invoke-CheckedProcess -FilePath $installerPath -ArgumentList @('/S')
    $initialApplicationHash = Invoke-StartupProbe

    New-Item -ItemType Directory -Force -Path $dataRoot | Out-Null
    Set-Content -LiteralPath $sentinel -Value 'preserve-user-data' -NoNewline

    Invoke-CheckedProcess -FilePath $installerPath -ArgumentList @('/S')
    if (-not (Test-Path -LiteralPath $sentinel -PathType Leaf)) {
        throw 'Upgrade removed the user data sentinel'
    }
    $upgradedApplicationHash = Invoke-StartupProbe
    if ($upgradedApplicationHash -cne $initialApplicationHash) {
        throw (
            'Upgrade replaced the installed application with unexpected bytes: ' +
            "initial SHA-256 $initialApplicationHash, upgraded SHA-256 $upgradedApplicationHash"
        )
    }

    if (-not (Test-Path -LiteralPath $uninstaller -PathType Leaf)) {
        throw "Uninstaller is missing: $uninstaller"
    }
    Invoke-CheckedProcess -FilePath $uninstaller -ArgumentList @('/S')
    if (Test-Path -LiteralPath $application) {
        throw 'Uninstall left the application executable behind'
    }
    if (-not (Test-Path -LiteralPath $sentinel -PathType Leaf)) {
        throw 'Uninstall removed user data'
    }

    [ordered]@{
        installer = $installerPath
        application_sha256 = $initialApplicationHash
        desktop_window_probes = 2
        profile_preserved = $true
        startup_probes = 2
        user_data_preserved = $true
        web_startup_probes = 2
    } | ConvertTo-Json
}
finally {
    if (Test-Path -LiteralPath $uninstaller -PathType Leaf) {
        $cleanup = Start-Process `
            -FilePath $uninstaller `
            -ArgumentList @('/S') `
            -NoNewWindow `
            -PassThru `
            -Wait
        if ($cleanup.ExitCode -ne 0) {
            Write-Warning "Cleanup uninstaller exited with code $($cleanup.ExitCode)"
        }
    }
}
