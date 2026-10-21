//! `--screens <каталог>`: снимки экранов без человека — дизайн проверяется по PNG,
//! а не нажатиями в окне (как `--screens` Postman).
//!
//! Сцены идут по очереди, как шаги замера: команды Core → ждём, пока листинги
//! панелей загружены, и ещё несколько кадров → снимок `PrintWindow` в фоновом
//! потоке → следующая сцена. На снимок — строка `screen <имя> <путь> <ш>x<в>` в
//! stdout, в конце — выход; ошибка (снимок не удался, одного цвета, сцена не
//! готова) — stderr и код 1. Сообщения — по-английски: это диагностика.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use urfin_core::{Command, ListingStatus, Side};

/// Кадров с готовой сценой до снимка: первые кадры после смены состояния ещё прежние.
const SETTLE_FRAMES: u32 = 3;
const SCENE_TIMEOUT: Duration = Duration::from_secs(20);

/// Сцена: имя файла и команды. `resize WxH` — размер окна, `wait prompt` —
/// снимок, когда открыт вопрос (job спрашивает асинхронно); это не команды Core.
struct Scene {
    name: &'static str,
    steps: &'static [&'static str],
}

const SCENES: &[Scene] = &[
    Scene {
        name: "files",
        steps: &[],
    },
    // Темы (D-01, №59): имена — как у снимков docs/design/screens/theme-*.png.
    Scene {
        name: "theme-system-light",
        steps: &["ui.theme.set id=system-light"],
    },
    Scene {
        name: "theme-system-dark",
        steps: &["ui.theme.set id=system-dark"],
    },
    Scene {
        name: "theme-blue",
        steps: &["ui.theme.set id=blue"],
    },
    Scene {
        name: "theme-turquoise",
        steps: &["ui.theme.set id=turquoise"],
    },
    Scene {
        name: "theme-grass",
        steps: &["ui.theme.set id=grass"],
    },
    Scene {
        name: "theme-sepia",
        steps: &["ui.theme.set id=sepia"],
    },
    Scene {
        name: "marked",
        steps: &[
            "ui.theme.follow_os",
            "sel.add mask=\"*.md;*.pdf;*.jpg\"",
            "panel.cursor.down n=3",
        ],
    },
    Scene {
        name: "brief",
        steps: &["panel.view_mode mode=brief"],
    },
    Scene {
        name: "medium",
        steps: &["panel.view_mode mode=medium"],
    },
    Scene {
        name: "wide",
        steps: &["panel.view_mode mode=wide"],
    },
    Scene {
        name: "menu",
        steps: &["panel.view_mode mode=full", "ui.menu"],
    },
    Scene {
        name: "draft-copy",
        steps: &["ui.cancel", "ops.copy"],
    },
    Scene {
        name: "draft-delete",
        steps: &["ui.cancel", "ops.delete"],
    },
    Scene {
        name: "draft-mkdir",
        steps: &["ui.cancel", "ops.mkdir"],
    },
    Scene {
        name: "rename",
        steps: &["ui.cancel", "sel.none", "ui.rename"],
    },
    Scene {
        name: "quick-search",
        steps: &["ui.cancel", "keymap.qs.start text=re"],
    },
    Scene {
        name: "toast",
        steps: &["keymap.qs.cancel", "panel.cursor.home", "ops.copy"],
    },
    Scene {
        name: "info",
        steps: &["panel.mode mode=info"],
    },
    Scene {
        name: "tree",
        steps: &["panel.mode mode=tree"],
    },
    Scene {
        name: "narrow",
        steps: &[
            "panel.mode mode=files",
            "ui.theme.set id=system-light",
            "resize 720x600",
        ],
    },
    Scene {
        name: "multi-rename",
        steps: &[
            "panel.focus side=left",
            "sel.add mask=\"*.md;*.pdf;*.jpg;*.txt\"",
            "ui.draft command=ops.multi_rename arg=pattern init=\"[N]_[C:1:1:2].[E]\"",
        ],
    },
    // Последней: вопрос о конфликте остаётся открытым (S5-07).
    Scene {
        name: "conflict",
        steps: &[
            "ui.cancel",
            "sel.none",
            "panel.focus side=left",
            "panel.cursor.to name=Cargo.toml",
            // Назначение относительно левой панели (корень демо-дерева).
            "ops.copy current=true dest=Projects",
            WAIT_PROMPT,
        ],
    },
];

const WAIT_PROMPT: &str = "wait prompt";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Enter,
    Wait,
    Shooting,
}

/// Ход снимков.
#[derive(Debug)]
pub struct Screens {
    dir: PathBuf,
    scene: usize,
    phase: Phase,
    settle: u32,
    since: Instant,
}

/// Что окну сделать в этом кадре.
pub(super) enum Step {
    /// Команды Core (уже разобранные) и, может быть, размер окна.
    Enter(Vec<Command>, Option<(f32, f32)>),
    /// Ждать следующего кадра.
    Next,
    /// Снять сцену в этот файл.
    Shoot(PathBuf),
    /// Ничего: снимок в работе.
    Idle,
}

impl Screens {
    pub fn new(dir: PathBuf) -> Screens {
        Screens {
            dir,
            scene: 0,
            phase: Phase::Enter,
            settle: 0,
            since: Instant::now(),
        }
    }

    fn name(&self) -> &'static str {
        SCENES[self.scene].name
    }

    /// Кадр нарисован; `ready` — листинги обеих панелей загружены.
    pub(super) fn frame(&mut self, ready: bool) -> Step {
        match self.phase {
            Phase::Enter => {
                self.phase = Phase::Wait;
                self.settle = 0;
                self.since = Instant::now();
                let mut cmds = Vec::new();
                let mut size = None;
                for s in SCENES[self.scene].steps {
                    if *s == WAIT_PROMPT {
                        continue;
                    }
                    if let Some(wh) = s.strip_prefix("resize ") {
                        size = parse_size(wh);
                        continue;
                    }
                    match Command::parse(s) {
                        Ok(c) => cmds.push(c),
                        Err(e) => fail(self.name(), &format!("bad step {s}: {e}")),
                    }
                }
                Step::Enter(cmds, size)
            }
            Phase::Wait => {
                if self.since.elapsed() > SCENE_TIMEOUT {
                    fail(self.name(), "scene not ready");
                }
                self.settle = if ready { self.settle + 1 } else { 0 };
                if self.settle < SETTLE_FRAMES {
                    return Step::Next;
                }
                self.phase = Phase::Shooting;
                Step::Shoot(self.dir.join(format!("{}.png", self.name())))
            }
            Phase::Shooting => Step::Idle,
        }
    }

    /// Снимок готов: строка в stdout, следующая сцена. `true` — сцены кончились.
    pub(super) fn shot(&mut self, result: Result<(PathBuf, u32, u32), String>) -> bool {
        match result {
            Ok((path, w, h)) => println!("screen {} {} {w}x{h}", self.name(), path.display()),
            Err(e) => fail(self.name(), &e),
        }
        self.scene += 1;
        self.phase = Phase::Enter;
        self.scene >= SCENES.len()
    }
}

impl Screens {
    /// Сцена готова к снимку: обе панели показывают загруженный листинг (и, если
    /// сцена ждёт, открыт вопрос).
    pub(super) fn ready(&self, s: &urfin_core::AppState) -> bool {
        let prompt = !SCENES[self.scene.min(SCENES.len() - 1)]
            .steps
            .contains(&WAIT_PROMPT)
            || s.prompts.active().is_some();
        prompt
            && [Side::Left, Side::Right]
                .into_iter()
                .all(|side| matches!(s.panel(side).tab().listing.status, ListingStatus::Complete))
    }
}

fn parse_size(wh: &str) -> Option<(f32, f32)> {
    let (w, h) = wh.split_once('x')?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

/// Снять окно и записать PNG (фоновый поток).
pub(super) fn shoot(hwnd: isize, path: &Path) -> Result<(PathBuf, u32, u32), String> {
    #[cfg(windows)]
    {
        let img = crate::sys::capture::capture(hwnd)?;
        if img
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[..] == img.rgba[..4])
        {
            return Err(format!(
                "capture {}x{} is a single color: the window is not drawn",
                img.width, img.height
            ));
        }
        write_png(path, img.width, img.height, &img.rgba)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok((path.to_path_buf(), img.width, img.height))
    }
    #[cfg(not(windows))]
    {
        let _ = (hwnd, path);
        Err("screens: Windows only".into())
    }
}

fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> Result<(), png::EncodingError> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut enc = png::Encoder::new(file, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header()?;
    wr.write_image_data(rgba)?;
    wr.finish()
}

fn fail(scene: &str, e: &str) -> ! {
    eprintln!("screens: {scene}: {e}");
    std::process::exit(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Шаги всех сцен — разбираемые команды (опечатка ловится тестом, а не на стенде).
    #[test]
    fn scene_steps_parse() {
        for sc in SCENES {
            for s in sc.steps {
                if *s == WAIT_PROMPT {
                    continue;
                }
                if let Some(wh) = s.strip_prefix("resize ") {
                    assert!(parse_size(wh).is_some(), "{}: {s}", sc.name);
                } else {
                    Command::parse(s).unwrap_or_else(|e| panic!("{}: {s}: {e}", sc.name));
                }
            }
        }
    }
}
