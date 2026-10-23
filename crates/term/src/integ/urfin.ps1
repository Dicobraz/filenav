# Urfin: интеграция PowerShell 7 и Windows PowerShell 5.1 (S6 §5, S6-11).
# Приходит через -EncodedCommand после профиля пользователя: ExecutionPolicy и
# AllSigned её не останавливают. Обёртка над prompt (совместима с oh-my-posh и
# starship): 633;D;<код> → 633;E;<строка>;<nonce> → 633;A → 633;P;FnCwd → промпт
# пользователя → 633;B. Хуки PSReadLine: Ctrl+Alt+Shift+F12 — тихий cd из файла,
# Ctrl+Alt+Shift+F11 — очистить строку. CLM или нет PSReadLine — 633;P;FnHook=0.
$global:__UrfinNonce = $env:URFIN_NONCE
Remove-Item Env:URFIN_NONCE -ErrorAction SilentlyContinue
$global:__UrfinCwdFile = $env:URFIN_CWD_FILE
$global:__UrfinLastId = -1
$global:__UrfinFirst = $true
$global:__UrfinOrig = $function:prompt
if (-not $global:__UrfinOrig) {
    $global:__UrfinOrig = { "PS $($executionContext.SessionState.Path.CurrentLocation)$('>' * ($nestedPromptLevel + 1)) " }
}

function global:__UrfinEsc([string]$s) {
    $s = $s -replace '\\', '\\'
    $s = $s -replace ';', '\x3b'
    $s = $s -replace "`n", '\x0a'
    $s = $s -replace "`r", '\x0d'
    $s -replace "`t", '\x09'
}

function global:prompt {
    $ok = $global:?
    $code = $global:LASTEXITCODE
    $e = [char]27
    $bel = [char]7
    $out = ''
    $h = Get-History -Count 1
    if (-not $global:__UrfinFirst) {
        if ($h -and $h.Id -ne $global:__UrfinLastId) {
            $ec = 0
            if (-not $ok) {
                $ec = 1
                if ($code -is [int] -and $code -ne 0) { $ec = $code }
            }
            $out += "$e]633;D;$ec$bel"
            $out += "$e]633;E;" + (__UrfinEsc $h.CommandLine) + ";$global:__UrfinNonce$bel"
        } else {
            $out += "$e]633;D$bel"
        }
    }
    if ($h) { $global:__UrfinLastId = $h.Id }
    $global:__UrfinFirst = $false
    $out += "$e]633;A$bel"
    $loc = $executionContext.SessionState.Path.CurrentLocation
    if ($loc.Provider.Name -eq 'FileSystem') {
        $out += "$e]633;P;FnCwd=$global:__UrfinNonce;" + (__UrfinEsc $loc.ProviderPath) + $bel
    }
    $p = & $global:__UrfinOrig
    $global:LASTEXITCODE = $code
    $out + $p + "$e]633;B$bel"
}

if ($ExecutionContext.SessionState.LanguageMode -ne 'FullLanguage' -or -not (Get-Module PSReadLine)) {
    Write-Host -NoNewline "$([char]27)]633;P;FnHook=0$([char]7)"
} else {
    Set-PSReadLineKeyHandler -Chord 'Ctrl+Alt+Shift+F12' -ScriptBlock {
        $p = Get-Content -LiteralPath $global:__UrfinCwdFile -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
        if ($p) { Set-Location -LiteralPath $p.Trim() -ErrorAction SilentlyContinue }
        [Microsoft.PowerShell.PSConsoleReadLine]::InvokePrompt()
    }
    Set-PSReadLineKeyHandler -Chord 'Ctrl+Alt+Shift+F11' -ScriptBlock {
        [Microsoft.PowerShell.PSConsoleReadLine]::RevertLine()
    }
}
