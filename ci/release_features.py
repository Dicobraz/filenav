#!/usr/bin/env python3
"""Запрет диагностических features в release-графе urfin-app (S0 v5 №11).

Падает, если в разрешённом графе `urfin-app` (normal + build зависимости,
без dev) у любого пакета включена feature `profile`, `dhat-heap` или
`count-alloc`. Ищет по именам в выводе `cargo tree`, а не в `Cargo.toml`,
поэтому работает и до появления самих features (S9-08, S9-10).

Запуск: python3 ci/release_features.py   (из корня репо). Вывод — ASCII.
"""

import re
import subprocess
import sys

FORBIDDEN = {"profile", "dhat-heap", "count-alloc"}
LINE = re.compile(r"^(?P<pkg>\S+ v\S+)(?: \(.*?\))?(?: \[(?P<features>[^\]]*)\])?$")


def main() -> int:
    out = subprocess.run(
        [
            "cargo", "tree", "--locked",
            "-p", "urfin-app",
            "-e", "normal,build",
            "--prefix", "none",
            "--no-dedupe",
            "-f", "{p} [{f}]",
        ],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    bad = {}
    for raw in out.splitlines():
        m = LINE.match(raw.strip())
        if not m or not m.group("features"):
            continue
        hit = FORBIDDEN & {f.strip() for f in m.group("features").split(",")}
        if hit:
            bad.setdefault(m.group("pkg"), set()).update(hit)
    if bad:
        for pkg, feats in sorted(bad.items()):
            print(f"::error::{pkg}: forbidden release features enabled: {', '.join(sorted(feats))}")
        return 1
    print(f"OK: none of {', '.join(sorted(FORBIDDEN))} in the urfin-app release graph")
    return 0


if __name__ == "__main__":
    sys.exit(main())
