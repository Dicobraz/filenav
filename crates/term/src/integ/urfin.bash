# Urfin: интеграция bash (Git Bash, WSL) (S6 §5, S6-13).
# Запуск: bash --rcfile <этот файл> -i. Подключает файлы пользователя как login-shell
# (/etc/profile, ~/.bash_profile | ~/.bash_login | ~/.profile), затем маркеры 633:
# PROMPT_COMMAND (D, E, A, FnCwd), PS0 (C), конец PS1 (B). Хуки readline:
# Ctrl+Alt+Shift+F12 — тихий cd из файла, Ctrl+Alt+Shift+F11 — очистить строку.
if [ -n "$__urfin_loaded" ]; then return 0; fi
__urfin_loaded=1
__urfin_nonce="$URFIN_NONCE"
unset URFIN_NONCE

[ -r /etc/profile ] && . /etc/profile
if [ -r ~/.bash_profile ]; then
    . ~/.bash_profile
elif [ -r ~/.bash_login ]; then
    . ~/.bash_login
elif [ -r ~/.profile ]; then
    . ~/.profile
elif [ -r ~/.bashrc ]; then
    . ~/.bashrc
fi

__urfin_esc() {
    local s="${1//\\/\\\\}"
    s="${s//;/\\x3b}"
    s="${s//$'\n'/\\x0a}"
    s="${s//$'\r'/\\x0d}"
    printf '%s' "$s"
}

# Каталог для Urfin: в Git Bash (MSYS) — путь Windows (`/tmp` и точки монтирования
# иначе не перевести), в WSL — как есть (переводит Urfin).
__urfin_pwd() {
    if [ -n "$MSYSTEM" ]; then builtin pwd -W; else printf '%s' "$PWD"; fi
}

__urfin_first=1
__urfin_last=
__urfin_pre() {
    local code=$?
    local h
    h=$(HISTTIMEFORMAT= builtin history 1)
    if [ -n "$__urfin_first" ]; then
        __urfin_first=
        __urfin_last=$h
    elif [ -n "$h" ] && [ "$h" != "$__urfin_last" ]; then
        __urfin_last=$h
        local cmd=
        if [[ $h =~ ^[[:space:]]*[0-9]+[[:space:]]+(.*)$ ]]; then
            cmd=${BASH_REMATCH[1]}
        fi
        printf '\033]633;D;%s\007\033]633;E;%s;%s\007' "$code" "$(__urfin_esc "$cmd")" "$__urfin_nonce"
    else
        printf '\033]633;D\007'
    fi
    printf '\033]633;A\007\033]633;P;FnCwd=%s;%s\007' "$__urfin_nonce" "$(__urfin_esc "$(__urfin_pwd)")"
    return $code
}

__urfin_post() {
    case "$PS1" in
        *'633;B'*) ;;
        *) PS1="$PS1"'\[\033]633;B\007\]' ;;
    esac
}

PROMPT_COMMAND="__urfin_pre${PROMPT_COMMAND:+;$PROMPT_COMMAND};__urfin_post"
PS0="${PS0}"$'\033]633;C\007'

__urfin_cd() {
    local p
    IFS= read -r p < "$URFIN_CWD_FILE" 2>/dev/null || [ -n "$p" ] || return
    builtin cd -- "$p" 2>/dev/null || return
    printf '\033]633;P;FnCwd=%s;%s\007' "$__urfin_nonce" "$(__urfin_esc "$(__urfin_pwd)")"
}
__urfin_clear() {
    READLINE_LINE=
    READLINE_POINT=0
}
bind -x '"\e[24;8~": __urfin_cd' 2>/dev/null
bind -x '"\e[23;8~": __urfin_clear' 2>/dev/null
