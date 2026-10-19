# Pets stand: periodic disk cleanup (H-01). Shows sizes; deletes only with -Apply.
#  - per repo: target dir (from `cargo metadata`, honours CARGO_TARGET_DIR): debug\incremental, criterion,
#    foreign target triples (cross builds belong to CI), extra dirs from -Extra (e.g. manual, tmp);
#  - test datasets: %LOCALAPPDATA%\Pets\testdata\<app> for -Apps;
#  - %TEMP% items with the app prefixes (see clean-temp.ps1).
# Copy of pets/scripts/stand/clean.ps1 with Urfin defaults (datasets: urfin-testdata, `--example testdata -- clean`).
# Usage: powershell -File scripts\stand\clean.ps1 -Repos <path>,<path> [-Apps urfin,fn] [-Extra manual,tmp]
#        [-Testdata] [-Apply]
param(
  [string[]]$Repos = @("."),
  [string[]]$Apps = @("urfin", "fn"),
  [string[]]$Extra = @("manual", "tmp"),
  [switch]$Testdata,
  [switch]$Apply
)

# `-File` passes "a,b" as one string: split lists here.
$Repos = $Repos | ForEach-Object { $_ -split ',' } | Where-Object { $_ }
$Apps = $Apps | ForEach-Object { $_ -split ',' } | Where-Object { $_ }
$Extra = $Extra | ForEach-Object { $_ -split ',' } | Where-Object { $_ }

function SizeOf($path) {
  (Get-ChildItem $path -Recurse -File -Force -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum
}

$candidates = @()
foreach ($repo in $Repos) {
  Push-Location $repo
  $meta = cargo metadata --format-version 1 --no-deps 2>$null | ConvertFrom-Json
  Pop-Location
  if (-not $meta) { "${repo}: not a cargo workspace, skipped"; continue }
  $t = $meta.target_directory
  $candidates += Join-Path $t "debug\incremental"
  $candidates += Join-Path $t "release\incremental"
  $candidates += Join-Path $t "criterion"
  foreach ($e in $Extra) { $candidates += Join-Path $t $e }
  # Cross builds: target\<triple> (aarch64-apple-darwin, x86_64-unknown-linux-gnu, ...) - CI's job.
  Get-ChildItem $t -Directory -ErrorAction SilentlyContinue | Where-Object { $_.Name -match '^[a-z0-9_]+-[a-z0-9_]+-[a-z0-9_]+' } | ForEach-Object { $candidates += $_.FullName }
}
if ($Testdata) { foreach ($a in $Apps) { $candidates += Join-Path $env:LOCALAPPDATA "Pets\testdata\$a" } }
Get-ChildItem $env:TEMP -Force -ErrorAction SilentlyContinue | Where-Object { $n = $_.Name; $Apps | Where-Object { $n -like "$_-*" } } | ForEach-Object { $candidates += $_.FullName }

$total = 0
$found = @()
foreach ($c in ($candidates | Select-Object -Unique)) {
  if (-not (Test-Path $c)) { continue }
  $s = SizeOf $c
  $total += $s
  $found += $c
  "{0,10:N2} GB  {1}" -f ($s / 1GB), $c
}
"Total: {0:N2} GB" -f ($total / 1GB)
if (-not $Apply) { "Run with -Apply to delete."; exit 0 }
foreach ($c in $found) { Remove-Item $c -Recurse -Force -ErrorAction SilentlyContinue }
"Deleted."
