# Urfin: запуск shell пользователя в WSL с интеграцией (S6-13):
# wsl.exe -d <D> -e sh -c '<этот текст>'. Путь к скриптам — URFIN_SHELL_DIR (WSL
# переводит его по WSLENV …/p). Корень автомонтирования — 633;P;FnMountRoot.
d=$URFIN_SHELL_DIR
r=$(wslpath -u 'C:\' 2>/dev/null)
printf '\033]633;P;FnMountRoot=%s\007' "${r%c/}"
s=${SHELL:-}
[ -n "$s" ] || s=$(getent passwd "$(id -un)" | cut -d: -f7)
[ -n "$s" ] || s=/bin/sh
case ${s##*/} in
    bash) exec "$s" --rcfile "$d/urfin.bash" -i ;;
    zsh) URFIN_USER_ZDOTDIR=${ZDOTDIR:-$HOME} ZDOTDIR=$d/zsh exec "$s" -i -l ;;
    fish) exec "$s" -l --init-command "source '$d/urfin.fish'" ;;
    *) exec "$s" -l ;;
esac
