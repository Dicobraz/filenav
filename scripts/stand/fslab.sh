#!/usr/bin/env bash
# Urfin: тестовые ФС на loop-образах. Использование: fslab.sh up|down|status
set -euo pipefail

LAB="${FSLAB_DIR:-$HOME/fslab}"
SIZE="${FSLAB_SIZE:-4G}"
FSES=(ext4 btrfs xfs ntfs3)

mkfs_for() {
  case "$1" in
    ext4)  sudo mkfs.ext4 -q -F "$2" ;;
    btrfs) sudo mkfs.btrfs -q -f "$2" ;;
    xfs)   sudo mkfs.xfs -q -f -m reflink=1 "$2" ;;
    ntfs3) sudo mkntfs -Q -F "$2" >/dev/null ;;
  esac
}

up() {
  mkdir -p "$LAB"
  for fs in "${FSES[@]}"; do
    img="$LAB/$fs.img"; mnt="/mnt/fl-$fs"
    if mountpoint -q "$mnt"; then echo "$mnt уже смонтирован"; continue; fi
    if [ ! -f "$img" ]; then truncate -s "$SIZE" "$img"; mkfs_for "$fs" "$img"; fi
    sudo mkdir -p "$mnt"
    if [ "$fs" = ntfs3 ]; then
      sudo mount -t ntfs3 -o loop,uid="$(id -u)",gid="$(id -g)" "$img" "$mnt"
    else
      sudo mount -o loop "$img" "$mnt"; sudo chown "$(id -u):$(id -g)" "$mnt"
    fi
    echo "$fs → $mnt"
  done
}

down() {
  for fs in "${FSES[@]}"; do
    mnt="/mnt/fl-$fs"
    if mountpoint -q "$mnt"; then sudo umount "$mnt"; echo "размонтирован $mnt"; fi
  done
}

status() { for fs in "${FSES[@]}"; do findmnt -n "/mnt/fl-$fs" || echo "/mnt/fl-$fs — нет"; done; }

case "${1:-}" in up) up ;; down) down ;; status) status ;; *) echo "usage: $0 up|down|status"; exit 1 ;; esac
