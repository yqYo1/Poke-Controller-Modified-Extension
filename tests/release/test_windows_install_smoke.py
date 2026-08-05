from __future__ import annotations

from pathlib import Path

REPOSITORY = Path(__file__).resolve().parents[2]
SCRIPT = (REPOSITORY / "scripts/release/windows_install_smoke.ps1").read_text(
    encoding="utf-8"
)


def section(start: str, end: str) -> str:
    return SCRIPT.split(start, maxsplit=1)[1].split(end, maxsplit=1)[0]


def test_startup_probe_runs_web_and_optionally_desktop_with_one_hash() -> None:
    hash_reader = section(
        "function Get-InstalledApplicationHash {",
        "\nfunction Assert-InstalledApplicationHash {",
    )
    hash_assertion = section(
        "function Assert-InstalledApplicationHash {",
        "\nAdd-Type -TypeDefinition @'",
    )
    startup_probe = section(
        "function Invoke-StartupProbe {",
        "\ntry {",
    )
    initial_hash = "$applicationHash = Get-InstalledApplicationHash"
    web_probe = (
        "-ArgumentList @('--ui', 'web', '--exit-after-startup') |\n        Out-Host"
    )
    web_hash = (
        "Assert-InstalledApplicationHash -Expected $applicationHash "
        "-Probe 'Web startup' |\n        Out-Host"
    )
    desktop_probe = "Invoke-DesktopWindowProbe | Out-Host"
    desktop_hash = (
        "Assert-InstalledApplicationHash -Expected $applicationHash "
        "-Probe 'Desktop window'"
    )

    assert "Get-FileHash -LiteralPath $application -Algorithm SHA256" in hash_reader
    assert "$actual = Get-InstalledApplicationHash" in hash_assertion
    assert "$actual -cne $Expected" in hash_assertion
    assert startup_probe.count(initial_hash) == 1
    assert startup_probe.count(web_probe) == 1
    assert startup_probe.count(desktop_probe) == 1
    assert startup_probe.count("Assert-InstalledApplicationHash") == 2
    assert startup_probe.count("Out-Host") == 4
    assert "[switch] $ProbeDesktopWindow" in startup_probe
    assert "if ($ProbeDesktopWindow)" in startup_probe
    assert (
        startup_probe.index(initial_hash)
        < startup_probe.index(web_probe)
        < startup_probe.index(web_hash)
        < startup_probe.index(desktop_probe)
        < startup_probe.index(desktop_hash)
    )
    assert "-FilePath $application" in startup_probe
    assert startup_probe.rstrip().endswith("return $applicationHash\n}")


def test_desktop_probe_requires_a_live_exact_native_window() -> None:
    desktop_probe = section(
        "function Invoke-DesktopWindowProbe {",
        "\nfunction Invoke-StartupProbe {",
    )
    job_wrapper = section("Add-Type -TypeDefinition @'", "\n'@")
    window_observer = job_wrapper.split(
        "public VisibleWindowObservation ObserveVisibleTopLevelWindows(",
        maxsplit=1,
    )[1].split("public void Terminate(uint exitCode)", maxsplit=1)[0]

    assert "$desktopWindowTitle = 'PokeCon Controller'" in SCRIPT
    assert "$desktopWindowTimeoutSeconds = 120" in SCRIPT
    assert "$desktopTerminationTimeoutSeconds = 15" in SCRIPT
    assert "[PokeConSmoke.WindowsJobProcess]::StartDesktop(" in desktop_probe
    assert "$application," in desktop_probe
    assert "$desktopTerminationTimeoutSeconds * 1000" in desktop_probe
    assert "$process = $jobProcess.RootProcess" in desktop_probe
    assert '"\\"" + executable + "\\" --ui desktop"' in job_wrapper
    assert "--exit-after-startup" not in desktop_probe
    assert "$process.Refresh()" in desktop_probe
    assert "if ($process.HasExited)" in desktop_probe
    assert "MainWindowHandle" not in desktop_probe
    assert "MainWindowTitle" not in desktop_probe
    assert "NativeMethods.EnumWindows(" in window_observer
    assert "NativeMethods.IsWindowVisible(window)" in window_observer
    assert "NativeMethods.GetWindowThreadProcessId(window" in window_observer
    assert "NativeMethods.GetWindowTextLengthW(window)" in window_observer
    assert "NativeMethods.GetWindowTextW(window" in window_observer
    assert "uint rootProcessId = unchecked((uint)RootProcess.Id);" in window_observer
    assert "windowProcessId != rootProcessId" in window_observer
    assert "visibleWindows.Add(" in window_observer
    assert window_observer.count("return true;") >= 2
    assert (
        "String.Equals(title, exactTitle, StringComparison.Ordinal)" in window_observer
    )
    observation = (
        "$observation = $jobProcess.ObserveVisibleTopLevelWindows($desktopWindowTitle)"
    )
    confirmation = (
        "$confirmation = $jobProcess.ObserveVisibleTopLevelWindows(\n"
        "                        $desktopWindowTitle\n"
        "                    )"
    )
    assert desktop_probe.count(observation) == 1
    assert desktop_probe.count(confirmation) == 1
    assert "$observation.MatchingCount -eq 1" in desktop_probe
    assert "$observation.MatchingHandle -ne 0" in desktop_probe
    assert "$confirmation.MatchingCount -eq 1" in desktop_probe
    assert "$confirmation.MatchingHandle -eq $candidateHandle" in desktop_probe
    assert "last visible top-level windows" in desktop_probe
    assert "$desktopWindowTimeoutSeconds" in desktop_probe
    assert "exited early with code" in desktop_probe
    assert "did not expose a live native window" in desktop_probe
    assert desktop_probe.count("Stop-DesktopProbeProcess -JobProcess $jobProcess") == 2


def test_desktop_launch_assigns_a_suspended_root_to_a_kill_on_close_job() -> None:
    job_wrapper = section("Add-Type -TypeDefinition @'", "\n'@")
    launch = job_wrapper.split(
        "public static WindowsJobProcess StartDesktop(",
        maxsplit=1,
    )[1].split("public void Terminate(uint exitCode)", maxsplit=1)[0]
    dispose = job_wrapper.split("public void Dispose()", maxsplit=1)[1].split(
        "private void ThrowIfDisposed()",
        maxsplit=1,
    )[0]

    assert "CreateJobObjectW" in job_wrapper
    assert "JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x00002000" in job_wrapper
    assert "CREATE_SUSPENDED = 0x00000004" in job_wrapper
    assert "JobObjectBasicAccountingInformationClass = 1" in job_wrapper
    assert "JobObjectBasicProcessIdListClass = 3" in job_wrapper
    assert "JobObjectExtendedLimitInformationClass = 9" in job_wrapper
    assert (
        "NativeMethods.CreateJobObjectW(\n                IntPtr.Zero,\n                null)"
        in launch
    )
    set_limit = "NativeMethods.SetInformationJobObject("
    create = "NativeMethods.CreateProcessW("
    assign = "NativeMethods.AssignProcessToJobObject("
    root_process = "Process.GetProcessById((int)processInformation.dwProcessId)"
    resume = "NativeMethods.ResumeThread("
    assert (
        launch.index(set_limit)
        < launch.index(create)
        < launch.index(assign)
        < launch.index(root_process)
        < launch.index(resume)
    )
    assert "NativeMethods.CREATE_SUSPENDED" in launch
    assert "rootProcess.Handle" in launch
    assert "previousSuspendCount != 1" in launch
    assert "else if (!NativeMethods.TerminateProcess" in launch
    assert "NativeMethods.TerminateJobObject(jobHandle, 1)" in launch
    assert "Stopwatch cleanupStopwatch = Stopwatch.StartNew()" in launch
    assert "NativeMethods.WaitForSingleObject(" in launch
    assert "GetRemainingCleanupMilliseconds(" in launch
    assert "QueryActiveProcessCount(jobHandle)" in launch
    assert "cleanupFailures.Insert(0, launchFailure)" in launch
    assert "NativeMethods.CloseHandle(processInformation.hThread)" in launch
    assert "NativeMethods.CloseHandle(processInformation.hProcess)" in launch
    assert launch.index("hThread != IntPtr.Zero") < launch.index(
        "CloseHandle(processInformation.hThread)"
    )
    assert launch.index("hProcess != IntPtr.Zero") < launch.index(
        "CloseHandle(processInformation.hProcess)"
    )
    assert launch.index("rootProcess.Dispose()") < launch.index(
        "NativeMethods.WaitForSingleObject("
    )
    assert launch.index("CloseHandle(processInformation.hProcess)") < launch.index(
        "QueryActiveProcessCount(jobHandle)"
    )
    assert dispose.index("RootProcess.Dispose()") < dispose.index("job.Dispose()")
    assert "BREAKAWAY" not in job_wrapper


def test_desktop_cleanup_uses_one_deadline_for_root_and_job_accounting() -> None:
    termination = section(
        "function Stop-DesktopProbeProcess {",
        "\nfunction Invoke-DesktopWindowProbe {",
    )

    assert termination.count("[Diagnostics.Stopwatch]::StartNew()") == 1
    assert (
        "$timeoutMilliseconds = $desktopTerminationTimeoutSeconds * 1000" in termination
    )
    assert "$JobProcess.Terminate(1)" in termination
    assert "$terminateFailure = $_.Exception.ToString()" in termination
    assert "$JobProcess.CloseJobForKill()" in termination
    assert "$jobCloseFailure = $_.Exception.ToString()" in termination
    assert "$process.WaitForExit($remainingMilliseconds)" in termination
    assert "$process.Dispose()" in termination
    assert "$JobProcess.GetActiveProcessCount()" in termination
    assert "$activeProcesses -eq 0" in termination
    assert "$jobDrainedWithinDeadline" in termination
    assert "$rootReapedWithinDeadline" in termination
    assert "$null -ne $terminateFailure" in termination
    assert "$null -ne $jobCloseFailure" in termination
    assert "$JobProcess.GetActiveProcessIds()" in termination
    assert "job ActiveProcesses=$activeProcesses; residual PIDs" in termination
    assert "$JobProcess.Dispose()" in termination
    assert (
        termination.index("$terminateFailure = $_.Exception.ToString()")
        < termination.index("$JobProcess.CloseJobForKill()")
        < termination.index("$process.WaitForExit($remainingMilliseconds)")
        < termination.index("$process.Dispose()")
        < termination.index("while ($terminationStopwatch.ElapsedMilliseconds")
    )
    assert "shared $desktopTerminationTimeoutSeconds-second deadline" in termination
    assert "Get-CimInstance" not in SCRIPT
    assert "CreationCutoff" not in SCRIPT
    assert "$Process.Kill($true)" not in SCRIPT


def test_clean_install_proves_both_modes_and_upgrade_reuses_exact_binary() -> None:
    lifecycle = SCRIPT.split(
        "\ntry {\n    Invoke-CheckedProcess -FilePath $installerPath",
        maxsplit=1,
    )[1]

    assert lifecycle.count("= Invoke-StartupProbe") == 2
    clean_install = "$initialApplicationHash = Invoke-StartupProbe -ProbeDesktopWindow"
    upgraded_install = "$upgradedApplicationHash = Invoke-StartupProbe"
    assert lifecycle.count("-ProbeDesktopWindow") == 1
    assert clean_install in lifecycle
    assert upgraded_install in lifecycle
    assert "$upgradedApplicationHash -cne $initialApplicationHash" in lifecycle
    assert "application_sha256 = $initialApplicationHash" in lifecycle
    assert "desktop_window_probes = 1" in lifecycle
    assert "web_startup_probes = 2" in lifecycle
    assert lifecycle.index(clean_install) < lifecycle.index(upgraded_install)
