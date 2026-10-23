# Urfin: интеграция zsh (WSL) (S6 §5, S6-13). Подключается из нашего .zshrc после
# файлов пользователя: precmd (D, E, A, FnCwd; первым), preexec (C), конец PS1 (B;
# последним, после starship и тем). Хуки zle: Ctrl+Alt+Shift+F12 — тихий cd из файла
# и reset-prompt, Ctrl+Alt+Shift+F11 — очистить строку.
__urfin_nonce="$URFIN_NONCE"
unset URFIN_NONCE

__urfin_esc() {
    local s="${1//\\/\\\\}"
    s="${s//;/\\x3b}"
    s="${s//$'\n'/\\x0a}"
    s="${s//$'\r'/\\x0d}"
    printf '%s' "$s"
}

__urfin_ran=
__urfin_cmd=
__urfin_precmd() {
    local code=$?
    if [[ -n $__urfin_ran ]]; then
        printf '\033]633;D;%s\007\033]633;E;%s;%s\007' "$code" "$(__urfin_esc "$__urfin_cmd")" "$__urfin_nonce"
    else
        printf '\033]633;D\007'
    fi
    __urfin_ran=
    printf '\033]633;A\007\033]633;P;FnCwd=%s;%s\007' "$__urfin_nonce" "$(__urfin_esc "$PWD")"
    return $code
}
__urfin_preexec() {
    __urfin_ran=1
    __urfin_cmd=$1
    printf '\033]633;C\007'
}
__urfin_b() {
    [[ $PS1 == *'633;B'* ]] || PS1="$PS1"$'%{\033]633;B\007%}'
}
precmd_functions=(__urfin_precmd $precmd_functions __urfin_b)
preexec_functions+=(__urfin_preexec)

__urfin_cd() {
    local p
    p=$(<"$URFIN_CWD_FILE") 2>/dev/null || return
    builtin cd -- "$p" 2>/dev/null || return
    printf '\033]633;P;FnCwd=%s;%s\007' "$__urfin_nonce" "$(__urfin_esc "$PWD")"
    zle reset-prompt
}
__urfin_clear() {
    BUFFER=
    CURSOR=0
}
zle -N __urfin_cd
zle -N __urfin_clear
bindkey '^[[24;8~' __urfin_cd
bindkey '^[[23;8~' __urfin_clear
