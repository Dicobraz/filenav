//! Handler namespace `ui` (S2 §3): меняет только [`UiState`] и фокус, сам без I/O.
//! Чтения ФС для breadcrumb (меню подкаталогов, проверка введённого пути) — задачами
//! на полосе Interactive (ADR-0014, `fs_tasks`).

use std::sync::Arc;

use urfin_vfs::VfsRegistry;

use urfin_core::{
    ArgSpec, Args, CancelToken, CmdError, Command, CommandMeta, Cx, Focus, Handler, Level,
    OverlayKind, PromptId, Row, Side, UiEvent, ValueKind,
};

use crate::fs_tasks;
use crate::state::{Draft, Subdirs, ThemeMode, UiState};

const fn arg(name: &'static str, kind: ValueKind, required: bool) -> ArgSpec {
    ArgSpec {
        name,
        kind,
        required,
    }
}

const fn meta(id: &'static str, title: &'static str, args: &'static [ArgSpec]) -> CommandMeta {
    CommandMeta {
        id,
        title,
        category: "ui",
        args,
        repeatable: false,
    }
}

const SIDE: ArgSpec = arg("side", ValueKind::Str, false);
const ID: ArgSpec = arg("id", ValueKind::Int, false);

static COMMANDS: &[CommandMeta] = &[
    // `menu=subdirs side path` и `menu=crumbs side` — меню breadcrumb (ADR-0014).
    meta(
        "ui.menu",
        "Menu",
        &[
            arg("menu", ValueKind::Str, false),
            SIDE,
            arg("path", ValueKind::Loc, false),
        ],
    ),
    meta(
        "ui.qsearch",
        "Quick search",
        &[arg("text", ValueKind::Str, false)],
    ),
    meta("ui.rename", "Rename in place", &[]),
    // С `path` — подтвердить ввод: проверка и `panel.go` или тост (ADR-0014).
    meta(
        "ui.path_edit",
        "Edit path",
        &[SIDE, arg("path", ValueKind::Str, false)],
    ),
    meta(
        "ui.draft",
        "Command argument",
        &[
            arg("command", ValueKind::Str, true),
            arg("arg", ValueKind::Str, true),
            arg("init", ValueKind::Str, false),
        ],
    ),
    meta("ui.cancel", "Close", &[]),
    meta("ui.prompt.hide", "Hide question", &[ID]),
    meta("ui.prompt.show", "Show question", &[ID]),
    meta("ui.jobs.toggle", "Expand tasks", &[]),
    meta("ui.theme", "Theme", &[arg("mode", ValueKind::Str, true)]),
    meta("ui.zoom", "Zoom", &[arg("delta", ValueKind::Int, false)]),
    meta(
        "ui.split",
        "Panel split",
        &[
            arg("delta", ValueKind::Int, false),
            arg("reset", ValueKind::Bool, false),
        ],
    ),
    meta("ui.lang", "Language", &[arg("lang", ValueKind::Str, true)]),
    meta("ui.columns", "Columns", &[arg("set", ValueKind::Str, true)]),
    // Имя — из S3 §3 (Ctrl+O), чтобы keymap не расходился с S2.
    meta("ui.panels.toggle_all", "Hide or show panels", &[]),
    // «О программе» (S0-decisions №49): имя Úrfionn и пасхалка — тостом, пока нет окна.
    meta("ui.about", "About", &[]),
];

/// Шаг `ui.zoom` и пределы, %.
const ZOOM_STEP: i64 = 10;
const ZOOM_RANGE: (i64, i64) = (50, 300);
/// Шаг `ui.split` и пределы доли левой панели.
const SPLIT_STEP: f32 = 0.05;
const SPLIT_RANGE: (f32, f32) = (0.1, 0.9);

/// Handler `ui`. [`UiState`] кладётся в `AppState.ext` в первом pump.
pub struct UiHandler {
    /// Чтения для меню и проверки пути. Свой реестр, пока архивы (S8) не общие.
    vfs: Arc<VfsRegistry>,
    /// Текущая фоновая задача меню или проверки пути — отменяется при закрытии.
    task: Option<CancelToken>,
}

impl Default for UiHandler {
    fn default() -> Self {
        Self {
            vfs: Arc::new(VfsRegistry::with_local()),
            task: None,
        }
    }
}

impl std::fmt::Debug for UiHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiHandler").finish_non_exhaustive()
    }
}

impl UiHandler {
    /// Отменить прежнюю задачу и выдать токен новой.
    fn new_task(&mut self, cx: &Cx<'_>) -> CancelToken {
        self.cancel_task();
        let t = cx.rt.token();
        self.task = Some(t.clone());
        t
    }

    fn cancel_task(&mut self) {
        if let Some(t) = self.task.take() {
            t.cancel();
        }
    }
}

impl Handler for UiHandler {
    fn namespace(&self) -> &'static str {
        "ui"
    }

    fn commands(&self) -> &'static [CommandMeta] {
        COMMANDS
    }

    fn on_pump(&mut self, cx: &mut Cx<'_>) {
        let focus = cx.state.focus;
        let st = ui_state(cx);
        // Фокус ушёл из оверлея (клик в панель, вопрос core, быстрый поиск keymap) —
        // оверлей закрыт.
        let open = st.overlay;
        if open.is_some_and(|k| focus != Focus::Overlay(k)) {
            st.close_overlay();
            cx.state.touch();
            self.cancel_task();
        }
    }

    fn handle(&mut self, cmd: &Command, cx: &mut Cx<'_>) -> Result<(), CmdError> {
        let a = &cmd.args;
        match cmd.id.as_str() {
            "ui.menu" => self.menu(cx, a),
            // Быстрый поиск ведёт keymap (ADR-0016); здесь — синоним для меню.
            "ui.qsearch" => {
                let mut c = Command::new("keymap.qs.start");
                if let Some(t) = a.str("text") {
                    c = c.arg("text", t);
                }
                cx.dispatch(c);
                cx.keep_rev();
            }
            "ui.rename" => {
                let t = cx.state.active_tab();
                let Some(Row::Entry { entry, .. }) = t.row(t.cursor.row) else {
                    // На `..` и в пустом каталоге переименовывать нечего.
                    cx.keep_rev();
                    return Ok(());
                };
                let target = (t.id, entry.name.clone());
                open(cx, OverlayKind::InlineEdit, |st| st.rename = Some(target));
            }
            "ui.path_edit" => self.path_edit(cx, a),
            "ui.draft" => {
                let draft = Draft {
                    cmd: a.str("command").unwrap_or_default().into(),
                    arg: a.str("arg").unwrap_or_default().into(),
                    text: a.str("init").unwrap_or_default().into(),
                };
                open(cx, OverlayKind::Draft, |st| st.draft = Some(draft));
            }
            "ui.cancel" => {
                self.cancel_task();
                ui_state(cx).close_overlay();
                if matches!(cx.state.focus, Focus::Overlay(_)) {
                    set_focus(cx, Focus::Panel);
                }
            }
            "ui.prompt.hide" | "ui.prompt.show" => {
                let Some(id) = prompt_id(cx, a) else {
                    cx.keep_rev();
                    return Ok(());
                };
                let hidden = &mut ui_state(cx).prompt_hidden;
                hidden.retain(|h| *h != id);
                if cmd.id.as_str() == "ui.prompt.hide" {
                    hidden.push(id);
                }
            }
            "ui.jobs.toggle" => {
                let st = ui_state(cx);
                st.jobs_expanded = !st.jobs_expanded;
            }
            "ui.theme" => {
                let mode = match a.str("mode").unwrap_or_default() {
                    m if m.eq_ignore_ascii_case("system") => ThemeMode::System,
                    m if m.eq_ignore_ascii_case("light") => ThemeMode::Light,
                    m if m.eq_ignore_ascii_case("dark") => ThemeMode::Dark,
                    _ => {
                        return Err(CmdError::Arg {
                            name: "mode".into(),
                            msg: "expected system | light | dark".into(),
                        });
                    }
                };
                ui_state(cx).theme = Some(mode);
            }
            "ui.zoom" => zoom(ui_state(cx), a.int("delta")),
            "ui.split" => split(ui_state(cx), a.bool("reset"), a.int("delta")),
            "ui.lang" => lang(cx, a.str("lang").unwrap_or_default()),
            "ui.panels.toggle_all" => {
                let st = ui_state(cx);
                st.panels_hidden = !st.panels_hidden;
            }
            "ui.about" => {
                let i18n = urfin_i18n::current();
                let text = format!(
                    "{} {} — {}",
                    i18n.get("ui-about-name"),
                    env!("CARGO_PKG_VERSION"),
                    i18n.get("ui-about-aka")
                );
                cx.toast(Level::Info, text);
            }
            "ui.columns" => {
                ui_state(cx).columns = Some(a.str("set").unwrap_or_default().into());
            }
            _ => return Err(CmdError::Unknown(cmd.id.as_str().into())),
        }
        Ok(())
    }
}

impl UiHandler {
    /// `ui.menu`: F9 (S2-14) или меню breadcrumb — подкаталоги (`›`) и свёрнутые
    /// сегменты (`…`).
    fn menu(&mut self, cx: &mut Cx<'_>, a: &Args) {
        let menu = a.str("menu").unwrap_or_default();
        let side = side_arg(cx, a);
        if menu == "subdirs" {
            let loc = a
                .loc("path")
                .unwrap_or_else(|| cx.state.panel(side).tab().loc.clone());
            let cancel = self.new_task(cx);
            let generation = ui_state(cx).bump();
            fs_tasks::spawn_subdirs(
                cx.rt,
                Arc::clone(&self.vfs),
                loc.clone(),
                generation,
                cancel,
            );
            open(cx, OverlayKind::Menu, |st| {
                st.menu = Some("subdirs".into());
                st.menu_side = Some(side);
                st.subdirs = Some(Subdirs {
                    side,
                    loc,
                    items: None,
                    generation,
                });
            });
            return;
        }
        self.cancel_task();
        let crumbs = menu == "crumbs";
        // F9 без аргумента — меню активной панели (как Far).
        let menu = match menu {
            "" if cx.state.active == Side::Left => "left".into(),
            "" => "right".into(),
            m => m.into(),
        };
        open(cx, OverlayKind::Menu, |st| {
            st.menu = Some(menu);
            st.menu_side = crumbs.then_some(side);
        });
    }

    /// `ui.path_edit`: без `path` — открыть поле; с `path` — проверить и перейти.
    fn path_edit(&mut self, cx: &mut Cx<'_>, a: &Args) {
        let side = side_arg(cx, a);
        let Some(text) = a.str("path") else {
            self.cancel_task();
            open(cx, OverlayKind::InlineEdit, |st| st.path_edit = Some(side));
            return;
        };
        let base = cx.state.panel(side).tab().loc.clone();
        let Some(loc) = fs_tasks::resolve_input(text, &base) else {
            let mut args = urfin_i18n::FluentArgs::new();
            args.set("path", text.to_owned());
            args.set("error", "-");
            let msg = urfin_i18n::current().fmt("ui-path-not-found", &args);
            cx.toast(Level::Warn, msg);
            return;
        };
        let cancel = self.new_task(cx);
        let st = ui_state(cx);
        let generation = st.bump();
        st.path_check = Some(generation);
        if st.path_edit != Some(side) {
            st.close_overlay();
            st.path_check = Some(generation);
            st.overlay = Some(OverlayKind::InlineEdit);
            st.path_edit = Some(side);
            set_focus(cx, Focus::Overlay(OverlayKind::InlineEdit));
        }
        fs_tasks::spawn_path_check(cx.rt, Arc::clone(&self.vfs), side, loc, generation, cancel);
    }
}

/// [`UiState`] из `ext`; нет — создаётся.
pub(crate) fn ui_state<'c>(cx: &'c mut Cx<'_>) -> &'c mut UiState {
    if cx.state.ext.get::<UiState>().is_none() {
        cx.state.ext.insert(UiState::new());
    }
    cx.state.ext.get_mut::<UiState>().expect("inserted above")
}

/// Открыть оверлей: прежний закрывается, фокус — `Overlay(kind)`.
fn open(cx: &mut Cx<'_>, kind: OverlayKind, fill: impl FnOnce(&mut UiState)) {
    let st = ui_state(cx);
    st.close_overlay();
    st.overlay = Some(kind);
    fill(st);
    set_focus(cx, Focus::Overlay(kind));
}

fn set_focus(cx: &mut Cx<'_>, focus: Focus) {
    if cx.state.focus != focus {
        cx.state.focus = focus;
        cx.ui(UiEvent::FocusChanged(focus));
    }
}

fn side_arg(cx: &Cx<'_>, a: &Args) -> Side {
    use urfin_core::SideRef as S;
    let active = cx.state.active;
    match a.side("side") {
        None | Some(S::Active) => active,
        Some(S::Passive) => active.other(),
        Some(S::Left) => Side::Left,
        Some(S::Right) => Side::Right,
    }
}

fn zoom(st: &mut UiState, delta: Option<i64>) {
    st.zoom = match delta {
        Some(d) => {
            let z = (i64::from(st.zoom) + d.signum() * ZOOM_STEP).clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
            u16::try_from(z).unwrap_or(100)
        }
        None => 100,
    };
}

fn split(st: &mut UiState, reset: Option<bool>, delta: Option<i64>) {
    st.split = match (reset, delta) {
        (Some(true), _) | (_, None) => 0.5,
        (_, Some(d)) => {
            let step = if d < 0 { -SPLIT_STEP } else { SPLIT_STEP };
            (st.split + step).clamp(SPLIT_RANGE.0, SPLIT_RANGE.1)
        }
    };
}

/// Язык на лету: `epoch` растёт, кэши текста сбрасываются (S2 §7).
fn lang(cx: &mut Cx<'_>, lang: &str) {
    for w in urfin_i18n::set_language(lang) {
        if let urfin_i18n::I18nWarning::UnknownLanguage { requested, .. } = w {
            let mut args = urfin_i18n::FluentArgs::new();
            args.set("lang", requested);
            let text = urfin_i18n::current().fmt("ui-lang-unavailable", &args);
            cx.toast(Level::Warn, text);
        }
    }
}

/// `id` или активный вопрос.
fn prompt_id(cx: &Cx<'_>, a: &Args) -> Option<PromptId> {
    match a.int("id") {
        Some(id) => u64::try_from(id).ok().map(PromptId),
        None => cx.state.prompts.active().map(|p| p.id),
    }
}
