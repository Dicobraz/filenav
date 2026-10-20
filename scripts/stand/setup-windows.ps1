# Urfin: подготовка стенда Windows 11. Идемпотентно.
# Запуск из корня репо: powershell -ExecutionPolicy Bypass -File scripts/stand/setup-windows.ps1
# Build Tools ставятся с принятием лицензии Microsoft Visual Studio Build Tools.
$ErrorActionPreference = 'Stop'
$repo = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'

function Have($name) { [bool](Get-Command $name -ErrorAction SilentlyContinue) }

function WingetInstall($id, $extra) {
    $wargs = @('install', '--id', $id, '-e', '--source', 'winget', '--silent',
              '--accept-package-agreements', '--accept-source-agreements', '--disable-interactivity') + $extra
    & winget @wargs
    # 0 — установлено; -1978335189 — уже установлено, обновлений нет.
    if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne -1978335189) { throw "winget ${id}: код $LASTEXITCODE" }
}

# 1. Visual Studio Build Tools (C++ + Windows SDK) — линковщик для таргета msvc.
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$vc = $null
if (Test-Path $vswhere) {
    $vc = & $vswhere -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
}
if ($vc) {
    Write-Host "Build Tools: $vc"
} else {
    Write-Host 'Ставлю Visual Studio 2022 Build Tools (C++)...'
    WingetInstall 'Microsoft.VisualStudio.2022.BuildTools' @('--override',
        '--wait --quiet --norestart --nocache --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended')
    Write-Host 'Build Tools установлены; установщик может попросить перезагрузку.'
}

# 2. rustup и тулчейн из rust-toolchain.toml.
if (-not (Have 'rustup') -and -not (Test-Path (Join-Path $cargoBin 'rustup.exe'))) {
    Write-Host 'Ставлю rustup...'
    WingetInstall 'Rustlang.Rustup' @()
}
if ($env:Path -notlike "*$cargoBin*") { $env:Path = "$cargoBin;$env:Path" }
Push-Location $repo
try {
    rustup toolchain install   # без аргументов — версия из rust-toolchain.toml
} finally { Pop-Location }

# 3. cargo-nextest и cargo-deny — готовые бинари с GitHub Releases (нужен gh auth).
function GhTool($exe, $repoName, $tagPrefix, $pattern) {
    if (Test-Path (Join-Path $cargoBin $exe)) { Write-Host "$exe уже есть"; return }
    $tag = gh release list -R $repoName --limit 30 --exclude-pre-releases --json tagName --jq '.[].tagName' |
        Where-Object { $_ -like "$tagPrefix*" } | Select-Object -First 1
    if (-not $tag) { throw "${repoName}: релиз не найден" }
    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("urfin-" + [guid]::NewGuid())
    New-Item -ItemType Directory $tmp | Out-Null
    try {
        gh release download $tag -R $repoName -p $pattern -D $tmp
        if ($LASTEXITCODE -ne 0) { throw "gh release download: код $LASTEXITCODE" }
        $arc = Get-ChildItem $tmp -File | Select-Object -First 1
        if ($arc.Name -like '*.zip') {
            Expand-Archive $arc.FullName (Join-Path $tmp 'x')
        } else {
            New-Item -ItemType Directory (Join-Path $tmp 'x') | Out-Null
            # Системный bsdtar: GNU tar из Git for Windows принимает `C:` за удалённый хост.
            & (Join-Path $env:SystemRoot 'System32\tar.exe') -xzf $arc.FullName -C (Join-Path $tmp 'x')
            if ($LASTEXITCODE -ne 0) { throw "tar: код $LASTEXITCODE" }
        }
        $bin = Get-ChildItem (Join-Path $tmp 'x') -Recurse -Filter $exe | Select-Object -First 1
        if (-not $bin) { throw "$exe не найден в архиве $($arc.Name)" }
        Copy-Item $bin.FullName $cargoBin
        Write-Host "$exe $tag установлен"
    } finally { Remove-Item $tmp -Recurse -Force }
}
GhTool 'cargo-nextest.exe' 'nextest-rs/nextest' 'cargo-nextest-' '*x86_64-pc-windows-msvc.zip'
GhTool 'cargo-deny.exe' 'EmbarkStudios/cargo-deny' '' '*x86_64-pc-windows-msvc.tar.gz'

# 4. Общий каталог сборки на Dev Drive (H-01): Urfin, Postman и pets делят зависимости
# (gpui и др. собираются один раз). Только если D: — Dev Drive (ReFS), как в
# pets/scripts/stand/setup-windows.ps1; иначе не трогаем.
$vol = Get-Volume -DriveLetter D -ErrorAction SilentlyContinue
if ($vol -and $vol.FileSystemType -eq 'ReFS') {
    $current = [Environment]::GetEnvironmentVariable('CARGO_TARGET_DIR', 'User')
    if ($current -ne 'D:\cargo-target') {
        New-Item -ItemType Directory -Force 'D:\cargo-target' | Out-Null
        setx CARGO_TARGET_DIR 'D:\cargo-target' | Out-Null
        Write-Host 'CARGO_TARGET_DIR=D:\cargo-target (новые окна терминала)'
    } else {
        Write-Host 'CARGO_TARGET_DIR уже D:\cargo-target'
    }
} else {
    Write-Host 'Dev Drive D: не найден — CARGO_TARGET_DIR не меняется (target\ в каждом репо)'
}

Write-Host ''
Write-Host 'Готово. Проверка: cargo build --workspace; cargo nextest run --workspace'
Write-Host 'Чистка диска: scripts/stand/clean.ps1 (список), с -Apply — удалить'
