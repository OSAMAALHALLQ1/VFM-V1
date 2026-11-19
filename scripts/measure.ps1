#requires -Version 5.1
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$Media,
    [ValidatePattern('^[1-9][0-9]*x[1-9][0-9]*$')]
    [string]$Size = '1920x1080',
    [string]$Output = '',
    [ValidateRange(1, 3600)]
    [int]$Seconds = 20
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function ConvertTo-NativeArgument {
    param([AllowEmptyString()][string]$Value)
    if ($Value.IndexOf('"') -ge 0 -or $Value.IndexOf([char]0) -ge 0 -or
        $Value.IndexOf("`r") -ge 0 -or $Value.IndexOf("`n") -ge 0) {
        throw 'Arguments must not contain double quotes, NUL, or newline characters.'
    }
    # Windows argv rules: double trailing backslashes before the closing quote.
    return '"' + [regex]::Replace($Value, '(\\+)$', '$1$1') + '"'
}

function Ensure-Directory {
    param([string]$Path)
    $ancestor = $Path
    while (-not (Test-Path -LiteralPath $ancestor)) {
        $parent = [IO.Path]::GetDirectoryName($ancestor)
        if ([string]::IsNullOrEmpty($parent) -or $parent -eq $ancestor) {
            throw "Cannot find an existing parent directory for: $Path"
        }
        $ancestor = $parent
    }
    if (-not (Test-Path -LiteralPath $ancestor -PathType Container)) {
        throw "A directory path is occupied by a file: $ancestor"
    }
    [void][IO.Directory]::CreateDirectory($Path)
}

function Get-MetricSummary {
    param([object[]]$Values)
    $valid = @($Values | Where-Object { $null -ne $_ })
    if ($valid.Count -eq 0) {
        return [pscustomobject]@{ count = 0; mean = $null; max = $null }
    }
    $statistics = $valid | Measure-Object -Average -Maximum
    return [pscustomobject]@{
        count = $valid.Count
        mean = [double]$statistics.Average
        max = [double]$statistics.Maximum
    }
}

function Start-GpuCollector {
    $queue = [Collections.Concurrent.ConcurrentQueue[object]]::new()
    $ready = [Threading.ManualResetEventSlim]::new($false)
    $shell = [Management.Automation.PowerShell]::Create()
    try {
        # A separate in-process runspace keeps slow PDH initialization/queries
        # off the CPU sampler and the launched process's deadline watchdog.
        [void]$shell.AddScript({
            param($Queue, $Ready)
            try {
                while ($true) {
                    Get-Counter -Counter '\GPU Engine(*)\Utilization Percentage' `
                        -SampleInterval 1 -MaxSamples 1 -ErrorAction Stop | ForEach-Object {
                            $Queue.Enqueue([pscustomobject]@{ counter_set = $_; error = $null })
                            $Ready.Set()
                        }
                }
            } catch {
                $Queue.Enqueue([pscustomobject]@{
                    counter_set = $null
                    error = "GPU Engine counter unavailable: $($_.Exception.Message) (Counter names may be localized, unsupported, or inaccessible.)"
                })
                $Ready.Set()
            }
        }.ToString()).AddArgument($queue).AddArgument($ready)
        $invocation = $shell.BeginInvoke()
        return [pscustomobject]@{ shell = $shell; invocation = $invocation; queue = $queue; ready = $ready }
    } catch {
        $shell.Dispose()
        $ready.Dispose()
        throw
    }
}

function Stop-GpuCollector {
    param([object]$Collector)
    try {
        if (-not $Collector.invocation.IsCompleted) {
            $stop = $Collector.shell.BeginStop($null, $null)
            if (-not $stop.AsyncWaitHandle.WaitOne(2000)) {
                return 'GPU collector did not stop within 2 seconds; no unbounded wait was performed.'
            }
            $Collector.shell.EndStop($stop)
        }
        if (-not $Collector.invocation.AsyncWaitHandle.WaitOne(1000)) {
            return 'GPU pipeline completion timed out; no unbounded EndInvoke wait was performed.'
        }
        try { [void]$Collector.shell.EndInvoke($Collector.invocation) }
        catch [Management.Automation.PipelineStoppedException] { }
        $Collector.shell.Dispose()
        $Collector.ready.Dispose()
    } catch {
        return "GPU collector cleanup failed: $($_.Exception.Message)"
    }
}

function Get-ProcessGpuSample {
    param([int]$ProcessId, [AllowNull()][object]$CounterSet, [string]$UnavailableReason)
    $engines = @()
    $errors = [Collections.Generic.List[string]]::new()
    $timestamp = $null
    if ($null -eq $CounterSet) {
        $errors.Add($UnavailableReason)
    } else {
        $timestamp = $CounterSet.Timestamp.ToUniversalTime().ToString('o')
        foreach ($counter in $CounterSet.CounterSamples) {
            if ($counter.InstanceName -notlike ('pid_{0}_*' -f $ProcessId)) { continue }
            $value = $null
            if ($counter.Status -eq 0 -and
                -not [double]::IsNaN($counter.CookedValue) -and
                -not [double]::IsInfinity($counter.CookedValue) -and
                $counter.CookedValue -ge 0) {
                $value = [double]$counter.CookedValue
            } else {
                $errors.Add("Invalid GPU counter for $($counter.InstanceName): status=$($counter.Status), value=$($counter.CookedValue)")
            }
            $engines += [pscustomobject]@{
                instance = $counter.InstanceName
                utilization_percent = $value
                counter_status = $counter.Status
            }
        }
        if ($engines.Count -eq 0) {
            $errors.Add("No GPU Engine counter instances matched pid_${ProcessId}_*. The process may have no exposed GPU engine activity.")
        }
    }
    $maximum = Get-MetricSummary -Values @($engines | ForEach-Object { $_.utilization_percent })
    return [pscustomobject]@{
        timestamp_utc = $timestamp
        engines = @($engines)
        max_engine_percent = $maximum.max
        error = if ($errors.Count -gt 0) { $errors -join '; ' } else { $null }
    }
}

if ([string]::IsNullOrWhiteSpace($Output)) {
    $scriptRoot = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Path }
    $Output = Join-Path (Split-Path -Parent $scriptRoot) 'docs\bench'
}
foreach ($argument in @($Media, $Size, $Output)) { [void](ConvertTo-NativeArgument $argument) }
$root = if ($PSScriptRoot) { Split-Path -Parent $PSScriptRoot } else { Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path) }
$binary = Join-Path $root 'target\release\spike.exe'
$vlc = Join-Path $root 'third_party\vlc-3.0.24'
if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) { throw "Release binary not found: $binary" }
if (-not (Test-Path -LiteralPath (Join-Path $vlc 'libvlc.dll') -PathType Leaf)) {
    throw "libVLC runtime not found: $vlc"
}
$mediaPath = (Resolve-Path -LiteralPath $Media).ProviderPath
if (-not (Test-Path -LiteralPath $mediaPath -PathType Leaf)) { throw "Media must be a file: $mediaPath" }
$outputPath = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Output)
Ensure-Directory $outputPath
$runName = 'spike-{0}-{1}-{2}' -f ([DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ')), $Size, ([guid]::NewGuid().ToString('N').Substring(0, 8))
$jsonPath = Join-Path $outputPath ($runName + '.json')
$csvPath = Join-Path $outputPath ($runName + '.csv')
$appReportPath = Join-Path $outputPath ($runName + '.spike.json')
$stdoutPath = Join-Path $outputPath ($runName + '.stdout.log')
$stderrPath = Join-Path $outputPath ($runName + '.stderr.log')
$arguments = @($mediaPath, '--size', $Size, '--bench', $Seconds.ToString([Globalization.CultureInfo]::InvariantCulture), '--report', $appReportPath)
$argumentString = ($arguments | ForEach-Object { ConvertTo-NativeArgument $_ }) -join ' '

$hardwareErrors = [Collections.Generic.List[string]]::new()
$cpus = @()
$gpus = @()
$os = $null
try {
    $cpus = @(Get-CimInstance Win32_Processor | Select-Object Name, Manufacturer, NumberOfCores, NumberOfLogicalProcessors, MaxClockSpeed)
} catch { $hardwareErrors.Add("Win32_Processor: $($_.Exception.Message)") }
try {
    $gpus = @(Get-CimInstance Win32_VideoController | Select-Object Name, VideoProcessor, DriverVersion, DriverDate, AdapterRAM, PNPDeviceID)
} catch { $hardwareErrors.Add("Win32_VideoController: $($_.Exception.Message)") }
try {
    $os = Get-CimInstance Win32_OperatingSystem | Select-Object Caption, Version, BuildNumber, OSArchitecture, TotalVisibleMemorySize
} catch { $hardwareErrors.Add("Win32_OperatingSystem: $($_.Exception.Message)") }

$logicalProcessors = [Environment]::ProcessorCount
$warmupSamples = 3
$sampleLimit = $warmupSamples + $Seconds
$deadlineSeconds = $sampleLimit + 15
$samples = [Collections.Generic.List[object]]::new()
$runErrors = [Collections.Generic.List[string]]::new()
$process = [Diagnostics.Process]::new()
$process.StartInfo = [Diagnostics.ProcessStartInfo]::new()
$process.StartInfo.FileName = $binary
$process.StartInfo.WorkingDirectory = $root
$process.StartInfo.UseShellExecute = $false
$process.StartInfo.Arguments = $argumentString
$process.StartInfo.RedirectStandardOutput = $true
$process.StartInfo.RedirectStandardError = $true
$process.StartInfo.StandardOutputEncoding = [Text.UTF8Encoding]::new($false)
$process.StartInfo.StandardErrorEncoding = [Text.UTF8Encoding]::new($false)
$process.StartInfo.EnvironmentVariables['LIBVLC_PATH'] = $vlc
$process.StartInfo.EnvironmentVariables['SLINT_BACKEND'] = 'winit-femtovg'
$started = $false
$processId = $null
$exitCode = $null
$exited = $false
$timedOut = $false
$stdoutTask = $null
$stderrTask = $null
$gpuCollector = $null
$gpuCollectorError = $null
$startedUtc = $null
$clock = [Diagnostics.Stopwatch]::new()

try {
    try {
        $gpuCollector = Start-GpuCollector
        # Prime PDH outside the benchmark (bounded), then discard pre-launch data.
        [void]$gpuCollector.ready.Wait(120000)
        $entry = $null
        while ($gpuCollector.queue.TryDequeue([ref]$entry)) {
            if ($null -ne $entry.error) { $gpuCollectorError = $entry.error }
        }
    } catch { $gpuCollectorError = "GPU collector could not start: $($_.Exception.Message)" }
    $startedUtc = [DateTime]::UtcNow.ToString('o')
    if (-not $process.Start()) { throw 'Process.Start returned false.' }
    $started = $true
    $processId = $process.Id
    $clock.Start()
    # Drain both pipes concurrently. Waiting before draining can deadlock on full pipes.
    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    $process.Refresh()
    $previousCpuSeconds = $process.TotalProcessorTime.TotalSeconds
    $previousElapsed = $clock.Elapsed.TotalSeconds
    $nextSample = 1.0

    while (-not $process.HasExited) {
        if ($clock.Elapsed.TotalSeconds -ge $deadlineSeconds) {
            $timedOut = $true
            $runErrors.Add("Process exceeded the ${deadlineSeconds}s deadline; killing only launched PID $processId.")
            $process.Kill()
            break
        }
        if ($samples.Count -ge $sampleLimit) {
            # Bench owns its exit; do not kill it just because sampling is complete.
            [void]$process.WaitForExit(100)
            continue
        }

        $remaining = [math]::Min($nextSample - $clock.Elapsed.TotalSeconds,
            $deadlineSeconds - $clock.Elapsed.TotalSeconds)
        if ($remaining -gt 0) {
            [void]$process.WaitForExit([int][math]::Ceiling($remaining * 1000))
        }
        if ($process.HasExited) { break }
        if ($clock.Elapsed.TotalSeconds -ge $deadlineSeconds) { continue }
        try {
            $process.Refresh()
            $cpuSeconds = $process.TotalProcessorTime.TotalSeconds
            $workingSetBytes = $process.WorkingSet64
        } catch {
            if ($process.HasExited) { break }
            throw
        }
        $elapsed = $clock.Elapsed.TotalSeconds
        $sampleTimestamp = [DateTime]::UtcNow
        $freshCounterSet = $null
        if ($null -ne $gpuCollector) {
            $entry = $null
            while ($gpuCollector.queue.TryDequeue([ref]$entry)) {
                if ($null -ne $entry.error) { $gpuCollectorError = $entry.error }
                else { $freshCounterSet = $entry.counter_set }
            }
        }
        $unavailableReason = 'GPU collector has not returned a fresh counter sample for this tick; no previous value was reused.'
        if ($null -ne $gpuCollectorError) { $unavailableReason = $gpuCollectorError }
        $gpu = Get-ProcessGpuSample -ProcessId $processId -CounterSet $freshCounterSet -UnavailableReason $unavailableReason
        $interval = $elapsed - $previousElapsed
        $cpuDelta = $cpuSeconds - $previousCpuSeconds
        $cpuPercent = $null
        $sampleError = $null
        if ($interval -gt 0 -and $cpuDelta -ge 0) {
            $cpuPercent = 100.0 * $cpuDelta / $interval / $logicalProcessors
        } else {
            $sampleError = 'CPU delta or elapsed interval invalid; CPU utilization is unavailable.'
        }
        $index = $samples.Count + 1
        $samples.Add([pscustomobject]@{
            sample = $index
            timestamp_utc = $sampleTimestamp.ToString('o')
            elapsed_seconds = $elapsed
            interval_seconds = $interval
            included_in_summary = ($index -gt $warmupSamples)
            cpu_total_seconds = $cpuSeconds
            cpu_delta_seconds = $cpuDelta
            cpu_percent = $cpuPercent
            working_set_bytes = $workingSetBytes
            working_set_mib = $workingSetBytes / 1MB
            gpu_timestamp_utc = $gpu.timestamp_utc
            gpu_engines = @($gpu.engines)
            gpu_max_engine_percent = $gpu.max_engine_percent
            gpu_error = $gpu.error
            sample_error = $sampleError
        })
        $previousCpuSeconds = $cpuSeconds
        $previousElapsed = $elapsed
        $nextSample += 1.0
        if ($nextSample -lt $elapsed) { $nextSample = $elapsed + 1.0 }
    }
} catch {
    $runErrors.Add($_.Exception.Message)
} finally {
    if ($started) {
        try {
            if (-not $process.HasExited) {
                # Also clean up this process on harness errors; never kill by name.
                $process.Kill()
            }
            $exited = $process.WaitForExit(5000)
            if ($exited) { $exitCode = $process.ExitCode }
            else { $runErrors.Add('Launched process did not exit within 5 seconds after termination.') }
        } catch { $runErrors.Add("Process cleanup failed: $($_.Exception.Message)") }
    }
    $clock.Stop()
    if ($null -ne $gpuCollector) {
        $gpuCleanupError = Stop-GpuCollector -Collector $gpuCollector
        if ($null -ne $gpuCleanupError) { $runErrors.Add($gpuCleanupError) }
    }
    $utf8 = [Text.UTF8Encoding]::new($false)
    foreach ($stream in @(
        @{ task = $stdoutTask; path = $stdoutPath },
        @{ task = $stderrTask; path = $stderrPath }
    )) {
        try {
            if ($null -ne $stream.task -and $stream.task.Wait(5000)) {
                [IO.File]::WriteAllText($stream.path, $stream.task.Result, $utf8)
            } else {
                $runErrors.Add("Output stream was not fully drained: $($stream.path)")
            }
        } catch { $runErrors.Add("Output capture failed: $($_.Exception.Message)") }
    }
    $process.Dispose()
}

if ($null -ne $exitCode -and $exitCode -ne 0) { $runErrors.Add("spike.exe exited with code $exitCode.") }
$appReportExists = Test-Path -LiteralPath $appReportPath -PathType Leaf
if (-not $appReportExists) {
    $runErrors.Add("Application report missing: $appReportPath")
} else {
    try { [void](Get-Content -LiteralPath $appReportPath -Raw -Encoding UTF8 | ConvertFrom-Json) }
    catch { $runErrors.Add("Application report is not valid JSON: $($_.Exception.Message)") }
}
$measured = @($samples | Where-Object { $_.included_in_summary })
$gpuEngineSummary = @($measured | ForEach-Object { $_.gpu_engines } |
    Group-Object -Property instance | ForEach-Object {
        [pscustomobject]@{
            instance = $_.Name
            utilization_percent = Get-MetricSummary -Values @($_.Group | ForEach-Object { $_.utilization_percent })
        }
    })
$coverageWarnings = @()
if ($measured.Count -ne $Seconds) {
    $coverageWarnings += "Captured $($measured.Count) of $Seconds requested post-warmup samples. Automatic exit or timer jitter can truncate the final sample; no values were invented."
}

$report = [ordered]@{
    schema_version = 1
    started_utc = $startedUtc
    finished_utc = [DateTime]::UtcNow.ToString('o')
    binary = $binary
    media = $mediaPath
    size = $Size
    requested_measurement_seconds = $Seconds
    observed_process_seconds = $clock.Elapsed.TotalSeconds
    measurement_complete = ($measured.Count -eq $Seconds)
    process_id = $processId
    exit_code = $exitCode
    nonzero_exit = ($null -ne $exitCode -and $exitCode -ne 0)
    process_exited = $exited
    timed_out = $timedOut
    success = ($started -and $exited -and $exitCode -eq 0 -and $runErrors.Count -eq 0)
    errors = @($runErrors.ToArray())
    coverage_warnings = @($coverageWarnings)
    environment = @{ LIBVLC_PATH = $vlc; SLINT_BACKEND = 'winit-femtovg' }
    hardware = @{ logical_processor_count = $logicalProcessors; processors = $cpus; video_controllers = $gpus; query_errors = @($hardwareErrors.ToArray()) }
    os = $os
    sampling_method = @{
        target_interval_seconds = 1
        clock = 'System.Diagnostics.Stopwatch (actual elapsed deltas; no interpolation)'
        cpu = '100 * delta(Process.TotalProcessorTime.TotalSeconds) / delta(elapsed_seconds) / Environment.ProcessorCount'
        memory = 'Process.WorkingSet64 snapshot in bytes; MiB = bytes / 1048576 (not private bytes or GPU memory)'
        gpu = 'Get-Counter \GPU Engine(*)\Utilization Percentage -MaxSamples 1 -SampleInterval 1 repeatedly in a background runspace; refresh wildcard instances each query; pid_PID_* only; preserve each instance; maximum valid engine percentage, NEVER sum engines'
        gpu_timing = 'PDH primed up to 120 seconds before launch; pre-launch values discarded. Wildcard instances are refreshed per query so a pre-launch expansion cannot exclude the new process. Each CPU tick consumes latest newly queued GPU snapshot with its own timestamp; no old snapshot is reused. Slow counters never block CPU sampling or the process deadline.'
        gpu_unavailable = 'null with explanatory error; missing engines are not treated as zero; summaries use only valid observations'
        warmup = 'Exclude first 3 process samples from every summary; retain them in raw output'
        warmup_samples = $warmupSamples
        deadline_seconds = $deadlineSeconds
        deadline_policy = '3 + Seconds + 15 seconds; kill only the Process object launched by this script; wait up to 5 seconds for termination'
    }
    summary = @{
        measured_samples = $measured.Count
        measured_interval_seconds = ($measured | Measure-Object -Property interval_seconds -Sum).Sum
        cpu_percent = Get-MetricSummary -Values @($measured | ForEach-Object { $_.cpu_percent })
        working_set_bytes = Get-MetricSummary -Values @($measured | ForEach-Object { $_.working_set_bytes })
        working_set_mib = Get-MetricSummary -Values @($measured | ForEach-Object { $_.working_set_mib })
        gpu_max_engine_percent = Get-MetricSummary -Values @($measured | ForEach-Object { $_.gpu_max_engine_percent })
        gpu_engines = @($gpuEngineSummary)
        gpu_unavailable_samples = @($measured | Where-Object { $null -eq $_.gpu_max_engine_percent }).Count
        gpu_errors = @($samples | ForEach-Object { $_.gpu_error } | Where-Object { $null -ne $_ } | Select-Object -Unique)
    }
    artifacts = @{ json = $jsonPath; samples_csv = $csvPath; application_report = $appReportPath; application_report_exists = $appReportExists; stdout = $stdoutPath; stderr = $stderrPath }
    samples = @($samples.ToArray())
}
[IO.File]::WriteAllText($jsonPath, ($report | ConvertTo-Json -Depth 12), $utf8)
$columns = @('sample', 'timestamp_utc', 'elapsed_seconds', 'interval_seconds', 'included_in_summary',
    'cpu_total_seconds', 'cpu_delta_seconds', 'cpu_percent', 'working_set_bytes', 'working_set_mib',
    'gpu_timestamp_utc', 'gpu_max_engine_percent', 'gpu_engines_json', 'gpu_error', 'sample_error')
$csvProperties = @($columns[0..11]) + @(
    @{ Name = 'gpu_engines_json'; Expression = { ConvertTo-Json -InputObject @($_.gpu_engines) -Compress -Depth 5 } },
    'gpu_error', 'sample_error')
$csvRows = @($samples | Select-Object -Property $csvProperties)
if ($csvRows.Count -gt 0) {
    $csvLines = @($csvRows | ConvertTo-Csv -NoTypeInformation)
} else {
    # Header-only CSV is an honest empty result, not a synthetic measurement.
    $csvLines = @(($columns | ForEach-Object { '"' + $_ + '"' }) -join ',')
}
[IO.File]::WriteAllLines($csvPath, [string[]]$csvLines, $utf8)
Write-Output $jsonPath
Write-Output $csvPath
foreach ($warning in $coverageWarnings) { Write-Warning $warning }
if (-not $report.success) {
    Write-Error ($runErrors -join '; ') -ErrorAction Continue
    exit 1
}
