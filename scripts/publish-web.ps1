[CmdletBinding()]
param(
    [string]$ProjectRoot = 'D:\GitProjects\shipontology.local\src\ShipOntology.WebCad'
)

$ErrorActionPreference = 'Stop'

function Invoke-Checked {
    param([string]$Command, [Parameter(ValueFromRemainingArguments)] [string[]]$Arguments)
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed ($LASTEXITCODE): $Command $($Arguments -join ' ')"
    }
}

$sourceRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$projectPath = [IO.Path]::GetFullPath($ProjectRoot)
if (-not (Test-Path -LiteralPath (Join-Path $projectPath 'ShipOntology.WebCad.csproj'))) {
    throw "ShipOntology.WebCad project was not found: $projectPath"
}
foreach ($tool in @('cargo', 'rustup', 'trunk', 'wasm-bindgen', 'robocopy')) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        throw "Required tool is missing: $tool"
    }
}
if ('wasm32-unknown-unknown' -notin (& rustup target list --installed)) {
    throw 'Install the WebAssembly target with: rustup target add wasm32-unknown-unknown'
}

# Only this generated asset directory may be mirrored. The host project and
# its wwwroot siblings must never be used as the robocopy destination.
$destination = [IO.Path]::GetFullPath((Join-Path $projectPath 'wwwroot\opencadstudio'))
$expected = $projectPath.TrimEnd('\') + '\wwwroot\opencadstudio'
if (-not $destination.Equals($expected, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Unexpected asset destination: $destination"
}
$buildOutput = Join-Path $sourceRoot 'dist\webcad-host'
# Trunk runs Cargo from the config directory. Keep it inside the checkout so
# .cargo/config.toml (wasm random backend, stack and target features) is used.
$temporaryConfig = Join-Path $sourceRoot ('.trunk-web-' + [guid]::NewGuid() + '.toml')
$previousNoColor = [Environment]::GetEnvironmentVariable('NO_COLOR', 'Process')
Push-Location $sourceRoot
try {
    Remove-Item Env:NO_COLOR -ErrorAction SilentlyContinue
    # Replace Trunk's POSIX-only hook with the same explicit Windows commands.
    [IO.File]::WriteAllText($temporaryConfig, '# Windows worker build is run below.')
    Invoke-Checked trunk build --config $temporaryConfig --release --dist $buildOutput --public-url /app/ --html-output index.html (Join-Path $sourceRoot 'web-app.html')
    Invoke-Checked cargo build --locked --release --target wasm32-unknown-unknown --package ocs_web_worker

    # Respect Cargo configuration and CARGO_TARGET_DIR rather than accidentally
    # publishing an old worker from a different target directory.
    $metadata = Invoke-Checked cargo metadata --locked --no-deps --format-version 1
    $targetDirectory = ($metadata | ConvertFrom-Json).target_directory
    $workerWasm = Join-Path $targetDirectory 'wasm32-unknown-unknown\release\ocs_web_worker.wasm'
    $workerOutput = Join-Path $buildOutput 'worker_pkg'
    New-Item -ItemType Directory -Force -Path $workerOutput | Out-Null
    Invoke-Checked wasm-bindgen --target web --out-dir $workerOutput --out-name ocs_web_worker $workerWasm

    foreach ($asset in @('index.html', 'ocs-parse-worker.js', 'worker_pkg\ocs_web_worker.js', 'worker_pkg\ocs_web_worker_bg.wasm')) {
        $assetPath = Join-Path $buildOutput $asset
        if (-not (Test-Path -LiteralPath $assetPath -PathType Leaf) -or (Get-Item -LiteralPath $assetPath).Length -eq 0) {
            throw "Build output is incomplete: $asset"
        }
    }
    if (-not (Get-ChildItem -LiteralPath $buildOutput -Filter 'OpenCADStudio*_bg.wasm')) {
        throw 'The main CAD WebAssembly module was not built.'
    }
    # Synchronize only after both modules and all their assets have built.
    & robocopy $buildOutput $destination /MIR /NFL /NDL /NJH /NJS /NP
    if ($LASTEXITCODE -ge 8) { throw "Asset synchronization failed: robocopy exit $LASTEXITCODE" }
    Write-Host "Published Web CAD to $destination"
    Write-Host 'Open /app/ on the ShipOntology.WebCad host; refresh the browser after publishing.'
}
finally {
    Pop-Location
    [Environment]::SetEnvironmentVariable('NO_COLOR', $previousNoColor, 'Process')
    Remove-Item -LiteralPath $temporaryConfig -Force -ErrorAction SilentlyContinue
}
