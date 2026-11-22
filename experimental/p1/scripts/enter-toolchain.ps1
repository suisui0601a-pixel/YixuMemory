# Process-local setup only; does not edit global PATH or install system components.
param(
    [Parameter(Mandatory = $true)][string]$BuildRoot,
    [string]$ToolRoot
)
$ErrorActionPreference = 'Stop'
if (-not $ToolRoot) { $ToolRoot = Join-Path $PSScriptRoot '..\..\p1-tooling' }
$protoToolRoot = [IO.Path]::GetFullPath($ToolRoot)
$protoRustBin = Join-Path $protoToolRoot 'rustup\toolchains\1.99.1-x86_64-pc-windows-gnu\bin'
$protoCcBin = Join-Path $protoToolRoot 'devkit\w64devkit\bin'
$env:RUSTUP_HOME = Join-Path $protoToolRoot 'rustup'
$env:CARGO_HOME = Join-Path $BuildRoot 'cargo'
$env:CARGO_TARGET_DIR = Join-Path $BuildRoot 'target'
$env:RUSTC = Join-Path $protoRustBin 'rustc.exe'
$env:RUSTDOC = Join-Path $protoRustBin 'rustdoc.exe'
$env:RUSTFMT = Join-Path $protoRustBin 'rustfmt.exe'
$env:CC = Join-Path $protoCcBin 'gcc.exe'
$env:AR = Join-Path $protoCcBin 'ar.exe'
$env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = $env:CC
$env:RUSTFLAGS = '-C link-self-contained=yes'
$env:PATH = "$protoRustBin;$protoCcBin;$env:PATH"
$protoCompiler = & $env:RUSTC --version
if ($LASTEXITCODE -ne 0 -or $protoCompiler -notmatch '^rustc 1\.99\.1 ') { throw 'Pinned compiler is unavailable' }
