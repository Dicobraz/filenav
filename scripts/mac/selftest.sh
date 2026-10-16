#!/bin/zsh
# FileNav: автономная самопроверка на macOS (без участия пользователя).
#
#   zsh selftest.sh [путь к filenav]       # по умолчанию — filenav рядом со скриптом
#
# ~2 минуты; окно FileNav само открывается, разворачивается, уходит в полный
# экран и закрывается — клавиатуру и мышь не трогать. Итог — один архив
# ~/Desktop/filenav-selftest-<дата>.tgz: его и прислать. Ничего не ставит, ничего
# не отправляет в сеть, профиль пользователя не трогает (portable-каталог в /tmp).

set -u
zmodload zsh/datetime

HERE=${0:A:h}
SRC=${1:-$HERE/filenav}
STAMP=$(date +%Y%m%d-%H%M%S)
WORK=$(mktemp -d /tmp/filenav-selftest.XXXXXX)
OUT=$WORK/results
mkdir -p $OUT
APP=$WORK/filenav

log() { print -r -- "[$(date +%H:%M:%S)] $*" | tee -a $OUT/run.log }

# Запуск с таймаутом (в macOS нет `timeout`): код 124 — убит по таймауту.
run_t() {
  local secs=$1; shift
  "$@" &
  local pid=$! end=$(( EPOCHSECONDS + secs ))
  while kill -0 $pid 2>/dev/null; do
    if (( EPOCHSECONDS >= end )); then
      kill -9 $pid 2>/dev/null; wait $pid 2>/dev/null; return 124
    fi
    sleep 0.2
  done
  wait $pid
}

ms() { printf '%.1f' $(( (EPOCHREALTIME - $1) * 1000 )) }

if [[ ! -f $SRC ]]; then
  print -u2 "filenav не найден: $SRC"; exit 1
fi
cp "$SRC" $APP && chmod +x $APP
xattr -d com.apple.quarantine $APP 2>/dev/null
touch $WORK/filenav.portable          # всё пишется в $WORK/data
log "work dir: $WORK"

# 1. Система: ОС, железо, GPU и дисплеи (Retina, частота), архитектура бинаря.
{
  sw_vers; print; uname -a; print
  sysctl -n machdep.cpu.brand_string hw.model hw.memsize 2>/dev/null; print
  file $APP; lipo -info $APP 2>/dev/null; print
  system_profiler SPDisplaysDataType 2>/dev/null
} > $OUT/system.txt 2>&1
log "system info: done"

# 2. Без окна: Core, 1000 команд, выход.
t=$EPOCHREALTIME
run_t 60 $APP --smoke > $OUT/smoke.txt 2>&1; rc=$?
log "smoke: exit=$rc $(ms $t) ms"

# 3. Старт: 5 раз, JSON с этапами (первый видимый кадр и т.д.).
for i in 1 2 3 4 5; do
  run_t 30 $APP --bench-startup >> $OUT/startup.jsonl 2>> $OUT/startup.err; rc=$?
  log "bench-startup #$i: exit=$rc"
  sleep 1
done

# 4. Сценарий окна внутри приложения: разворот, полный экран, размеры,
#    свёртывание, простой; кадры, размеры, ошибки поверхности, снимки кадров.
t=$EPOCHREALTIME
run_t 150 $APP --selftest $OUT/selftest > $OUT/selftest.out 2>&1; rc=$?
log "selftest: exit=$rc $(ms $t) ms"

# 5. Обычный старт: память и CPU в простое, затем второй запуск (один экземпляр).
$APP > $OUT/primary.out 2>&1 &
primary=$!
sleep 5
footprint -p $primary > $OUT/footprint.txt 2>&1
vmmap --summary $primary > $OUT/vmmap.txt 2>&1
cpu0=$(ps -o time= -p $primary)
log "primary pid=$primary rss_kb=$(ps -o rss= -p $primary | tr -d ' ') cpu_time=$cpu0"
sleep 30
log "after 30 s idle: rss_kb=$(ps -o rss= -p $primary | tr -d ' ') cpu_time=$(ps -o time= -p $primary)"

mkdir -p $WORK/sub
front_before=$(lsappinfo info -only name $(lsappinfo front) 2>/dev/null)
t=$EPOCHREALTIME
run_t 10 $APP $WORK/sub > $OUT/second.out 2>&1; rc=$?
log "second start: exit=$rc $(ms $t) ms (должен сразу выйти, путь — новым табом в первом окне)"
sleep 1
front_after=$(lsappinfo info -only name $(lsappinfo front) 2>/dev/null)
log "front app: before=[$front_before] after=[$front_after] (после второго запуска впереди должно быть окно FileNav)"
log "primary alive after second start: $(kill -0 $primary 2>/dev/null && print yes || print no)"

kill -INT $primary 2>/dev/null
for i in {1..25}; do kill -0 $primary 2>/dev/null || break; sleep 0.2; done
kill -0 $primary 2>/dev/null && { kill -9 $primary; log "primary: killed"; } || log "primary: exited"

# 6. Логи, crash-файлы, сессия; архив на рабочий стол.
cp -R $WORK/data/logs $OUT/logs 2>/dev/null
cp $WORK/data/state/session.toml $OUT/ 2>/dev/null
ARCHIVE=~/Desktop/filenav-selftest-$STAMP.tgz
tar czf $ARCHIVE -C $WORK results
log "archive: $ARCHIVE"
print "\nГотово. Пришлите файл: $ARCHIVE"
