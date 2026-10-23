"""Индекс ручных проверок (скилл pets-dev, R-03): docs/manual-tests/README.md.

python scripts/manual_tests_index.py          # переписать индекс
python scripts/manual_tests_index.py --check  # код 1, если индекс устарел

Файл проверки — docs/manual-tests/<ID>.md: первая строка `# <ID> — <название>`,
затем строка `Статус: ⏳ | ✅ | 🔁 | ❌ …` (дата, сборка, кратко). Индекс руками не правится.
"""

import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
DIR = ROOT / "docs" / "manual-tests"
SKIP = {"README.md", "ARCHIVE.md"}

HEAD = """# Ручные проверки

Генерируется `scripts/manual_tests_index.py` — руками не править.

Файл на задачу: `docs/manual-tests/<ID>.md` — первая строка `# <ID> — <название>`, затем
`Статус: ⏳ | ✅ | 🔁 | ❌` (дата, сборка), шаги и ожидания таблицей. Приёмка — пакетом по бете;
итог владельца — в строке статуса файла. До 2026-10-24 — общий реестр [ARCHIVE.md](ARCHIVE.md)
(разделы §1–§61 со статусами на тот день), больше не правится.

| Задача | Проверка | Статус |
|---|---|---|
"""


def entry(path: pathlib.Path) -> str:
    lines = path.read_text(encoding="utf-8").splitlines()
    title = lines[0].lstrip("# ").strip() if lines else path.stem
    task, _, name = title.partition(" — ")
    status = next(
        (l.split(":", 1)[1].strip() for l in lines if l.startswith("Статус:")), "?"
    )
    return f"| [{task or path.stem}]({path.name}) | {name or title} | {status} |"


def render() -> str:
    rows = [entry(p) for p in sorted(DIR.glob("*.md")) if p.name not in SKIP]
    return HEAD + "\n".join(rows) + ("\n" if rows else "")


def main() -> int:
    text = render()
    out = DIR / "README.md"
    if "--check" in sys.argv:
        if not out.exists() or out.read_text(encoding="utf-8") != text:
            print("docs/manual-tests/README.md устарел: python scripts/manual_tests_index.py")
            return 1
        return 0
    out.write_text(text, encoding="utf-8", newline="\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
