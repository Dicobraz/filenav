# Экраны S2 §2 на Windows 11 (S2-21)

Снимки настоящего окна (wgpu DX12, системные шрифты Segoe) со сценариями из `crates/ui/legacy/screens_fixtures.rs`: по паре светлая/тёмная на экран, 1280×768 точек (на стенде масштаб 125 % → 1600×998 пикселей). Те же сценарии сверяет insta-тест `screens_match_snapshots` (Windows).

| Файл | Раздел S2 |
|---|---|
| `2_1_main_*` | §2.1 Таблица слева, Список справа |
| `2_2_medium_wide_*` | §2.2 Medium и Wide |
| `2_4_tree_info_*` | §2.4 Дерево и «Сведения» |
| `2_5_quick_view_*` | §2.5 быстрый просмотр |
| `2_6_menu_*` | §2.6 меню F9 |
| `2_7_delete_*` | §2.7 удаление |
| `2_8_conflict_*` | §2.8 конфликт копирования |
| `2_9_two_jobs_*` | §2.9 две задачи |
| `2_10_quick_search_*`, `2_10_rename_*` | §2.10 быстрый поиск, rename по месту |

§2.3 (Custom columns) — M2+, §2.11 (терминал) — S6. Keybar (§2.12) — на каждом снимке.

Каталоги на снимках — фиктивные (backend `test`, отсюда `test:` в командной строке и в «Сведениях»).

## Как переснять

```
cargo build --release -p urfin-app --features screens
urfin.exe --screens %TEMP%\urfin-screens
```

Окно само проходит по экранам и закрывается; PNG без сжатия. Сжатие без потерь для репозитория — PowerShell:

```
Add-Type -AssemblyName System.Drawing
Get-ChildItem $env:TEMP\urfin-screens\*.png | % { $i=[Drawing.Image]::FromFile($_.FullName); $i.Save("docs\screens\s2-21\$($_.Name)", [Drawing.Imaging.ImageFormat]::Png); $i.Dispose() }
```

Копия `urfin.exe` — с файлом `urfin.portable` рядом, чтобы не трогать профиль.
