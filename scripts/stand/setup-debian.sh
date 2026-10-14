#!/usr/bin/env bash
# FileNav: подготовка стенда Debian 13. Идемпотентно. Запуск: bash scripts/stand/setup-debian.sh
set -euo pipefail

sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  build-essential clang lld mold pkg-config git curl ca-certificates zip unzip jq python3 \
  libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev \
  libgl1-mesa-dev libegl1-mesa-dev mesa-utils libfontconfig-dev \
  btrfs-progs xfsprogs ntfs-3g dosfstools \
  hyperfine linux-perf heaptrack gh

if ! command -v rustup >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal -c clippy,rustfmt
fi
# shellcheck disable=SC1091
source "$HOME/.cargo/env"
rustup update stable
rustup target add x86_64-pc-windows-msvc

for c in cargo-nextest sccache cargo-xwin cargo-deny; do
  command -v "$c" >/dev/null 2>&1 || cargo install --locked "$c"
done

CFG="$HOME/.cargo/config.toml"
if [ ! -f "$CFG" ]; then
  cat >"$CFG" <<'EOF'
[build]
rustc-wrapper = "sccache"

[target.x86_64-unknown-linux-gnu]
linker = "clang"
rustflags = ["-C", "link-arg=-fuse-ld=mold"]
EOF
  echo "создан $CFG"
else
  echo "$CFG уже есть — не трогаю (добавь sccache и mold вручную при необходимости)"
fi

echo
echo "Готово. Дальше: gh auth login; установить Claude Code по официальной документации;"
echo "git clone <repo> ~/src/filenav; при необходимости: bash scripts/stand/fslab.sh up"
glxinfo -B 2>/dev/null | grep -E 'OpenGL (renderer|version)' || echo "glxinfo: нет GL-контекста (запусти из графической сессии)"
