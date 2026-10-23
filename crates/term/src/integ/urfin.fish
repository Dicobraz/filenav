# Urfin: интеграция fish (WSL) (S6 §5, S6-13). fish --init-command 'source …' —
# после config.fish пользователя: fish_postexec (D, E), fish_preexec (C),
# fish_prompt (A, FnCwd), обёртка над fish_prompt (B). Хуки: Ctrl+Alt+Shift+F12 —
# тихий cd из файла, Ctrl+Alt+Shift+F11 — очистить строку.
set -g __urfin_nonce $URFIN_NONCE
set -e URFIN_NONCE

function __urfin_esc
    string replace -a '\\' '\\\\' -- $argv[1] | string replace -a ';' '\\x3b'
end

function __urfin_postexec --on-event fish_postexec
    printf '\e]633;D;%s\a\e]633;E;%s;%s\a' $status (__urfin_esc "$argv[1]") $__urfin_nonce
end

function __urfin_preexec --on-event fish_preexec
    printf '\e]633;C\a'
end

function __urfin_a --on-event fish_prompt
    printf '\e]633;A\a\e]633;P;FnCwd=%s;%s\a' $__urfin_nonce (__urfin_esc "$PWD")
end

if functions -q fish_prompt
    functions -c fish_prompt __urfin_user_prompt
else
    function __urfin_user_prompt
        printf '%s> ' (prompt_pwd)
    end
end
function fish_prompt
    __urfin_user_prompt
    printf '\e]633;B\a'
end

function __urfin_cd
    set -l p (cat "$URFIN_CWD_FILE" 2>/dev/null)
    test -n "$p"; and builtin cd -- "$p" 2>/dev/null; or return
    printf '\e]633;P;FnCwd=%s;%s\a' $__urfin_nonce (__urfin_esc "$PWD")
    commandline -f repaint
end
function __urfin_clear
    commandline ''
end
bind \e\[24\;8~ __urfin_cd
bind \e\[23\;8~ __urfin_clear
