"""Индекс решений владельца (скилл pets-dev, R-04): docs/decisions/README.md.

python scripts/decisions_index.py          # переписать индекс
python scripts/decisions_index.py --check  # код 1, если индекс устарел

Решение — docs/decisions/<YYYY-MM-DD>-<slug>.md: первая строка `# <заголовок>`, дальше —
суть, что заменяет, следствия. Сквозных номеров нет; индекс руками не правится.
"""

import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
DIR = ROOT / "docs" / "decisions"

HEAD = """# Решения владельца

Генерируется `scripts/decisions_index.py` — руками не править.

Приоритетнее всего остального в репозитории. Решение — файл `<YYYY-MM-DD>-<slug>.md`,
без сквозных номеров. До 2026-10-24 — архив [S0-decisions.md](../specs/S0-decisions.md)
(«№1…№93»; ссылки «S0 №NN» в коде и документах ведут туда), он больше не правится.

| Дата | Решение |
|---|---|
"""


def entry(path: pathlib.Path) -> str:
    lines = path.read_text(encoding="utf-8").splitlines()
    title = lines[0].lstrip("# ").strip() if lines else path.stem
    return f"| {path.name[:10]} | [{title}]({path.name}) |"


def render() -> str:
    files = sorted(
        (p for p in DIR.glob("*.md") if p.name != "README.md"),
        key=lambda p: p.name,
        reverse=True,
    )
    rows = [entry(p) for p in files]
    return HEAD + "\n".join(rows) + ("\n" if rows else "")


def main() -> int:
    text = render()
    out = DIR / "README.md"
    if "--check" in sys.argv:
        if not out.exists() or out.read_text(encoding="utf-8") != text:
            print("docs/decisions/README.md устарел: python scripts/decisions_index.py")
            return 1
        return 0
    out.write_text(text, encoding="utf-8", newline="\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
