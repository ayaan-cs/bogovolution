# Free replay demo for rules-evolve on Windows. No key, no cost, no network.
# Run from the unzipped folder in PowerShell:
#   powershell -ExecutionPolicy Bypass -File .\demo.ps1
# Needs sh.exe on PATH (Git for Windows, see WINDOWS-SETUP.md).
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$exe = Join-Path $here 'rules-evolve.exe'
$demo = Join-Path $here 'examples\demo'
$work = Join-Path $here 'demo-run'
if (Test-Path $work) { throw "$work already exists. Delete it first to run the demo again." }
New-Item -ItemType Directory -Path $work | Out-Null
Copy-Item -Recurse -Path (Join-Path $demo 'fixture\*') -Destination $work
& $exe -C $work init --model demo-model
if ($LASTEXITCODE -ne 0) { throw 'init failed' }
Copy-Item (Join-Path $demo 'tasks.toml'), (Join-Path $demo 'mock.toml') (Join-Path $work '.rules-evolve')
& $exe -C $work evolve --generations 10 --seed 7 --provider replay --replay-file (Join-Path $demo 'replay.jsonl')
if ($LASTEXITCODE -ne 0) { throw 'evolve failed' }
& $exe -C $work status
& $exe -C $work verify-log
& $exe -C $work export --out (Join-Path $here 'demo-tree.json')
Write-Host "Done. Watch it with: .\rules-evolve.exe -C demo-run watch"
Write-Host "Or open viewer\index.html and load demo-tree.json."
