# Prepares the current PowerShell window for building Wealthfolio (Rust + Tauri).
# Safe to run more than once: the VS 2022 environment is loaded only if it isn't already.
# Usage:  & ~\scripts\prepeare_shell.ps1

$buildTools = 'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools'
$devShell = Join-Path $buildTools 'Common7\Tools\Launch-VsDevShell.ps1'

if ($env:VSINSTALLDIR -and $env:VSINSTALLDIR.TrimEnd('\') -ne $buildTools) {
    Write-Warning "This window already has a different Visual Studio environment ($env:VSINSTALLDIR)."
    Write-Warning 'Open a fresh PowerShell window and run this script there.'
    return
}

if ($env:VSINSTALLDIR) {
    Write-Host 'VS 2022 Build Tools environment already loaded; skipping.'
} else {
    # Launch-VsDevShell prints a harmless "vswhere.exe is not recognized" line.
    & $devShell -Arch amd64 -HostArch amd64 -SkipAutomaticLocation 2>$null | Out-Null
    Write-Host 'Loaded VS 2022 Build Tools (x64) environment.'
}

# Vendored OpenSSL must be built with Strawberry Perl, not Git's perl.
$env:OPENSSL_SRC_PERL = 'C:\Strawberry\perl\bin\perl.exe'
# bindgen (HiGHS solver in crates/optimization-engine) needs libclang.
$env:LIBCLANG_PATH = Join-Path $env:LOCALAPPDATA 'libclang'

foreach ($tool in 'cargo', 'pnpm', 'link', 'cmake') {
    $found = Get-Command "$tool.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $found) { Write-Warning "$tool.exe not found on PATH" }
}
$link = (Get-Command link.exe -ErrorAction SilentlyContinue | Select-Object -First 1).Source
if ($link -and $link -notlike "$buildTools*") {
    Write-Warning "link.exe resolves to $link instead of the VS 2022 linker."
}
if ($env:Path.Length -gt 7000) {
    Write-Warning "PATH is $($env:Path.Length) characters; above ~8191 cmd.exe (and pnpm scripts) break. Use a fresh window."
}

Write-Host "Ready. PATH length $($env:Path.Length). Now: cd ~\scripts\wealthfolio; pnpm tauri dev"
