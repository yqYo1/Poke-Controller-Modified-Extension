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
    Invoke-CheckedProcess `
        -FilePath $application `
        -ArgumentList @('--ui', 'web', '--exit-after-startup')
}

try {
    Invoke-CheckedProcess -FilePath $installerPath -ArgumentList @('/S')
    Invoke-StartupProbe

    New-Item -ItemType Directory -Force -Path $dataRoot | Out-Null
    Set-Content -LiteralPath $sentinel -Value 'preserve-user-data' -NoNewline

    Invoke-CheckedProcess -FilePath $installerPath -ArgumentList @('/S')
    if (-not (Test-Path -LiteralPath $sentinel -PathType Leaf)) {
        throw 'Upgrade removed the user data sentinel'
    }
    Invoke-StartupProbe

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
        profile_preserved = $true
        startup_probes = 2
        user_data_preserved = $true
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
