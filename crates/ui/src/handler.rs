//! Handler namespace `ui` (S2 §3): меняет только [`UiState`] и фокус, без I/O.
//! Оверлеи пока не рисуются — их виджеты появятся в S2-12…S2-17.

use filenav_core::{
    ArgSpec, Args, CmdError, Command, CommandMeta, Cx, Focus, Handler, Level, OverlayKind,
    PromptId, Row, Side, UiEvent, ValueKind,
};

use crate::state::{Draft, ThemeMode, UiState};

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
    meta("ui.menu", "Menu", &[arg("menu", ValueKind::Str, false)]),
    meta(
        "ui.qsearch",
        "Quick search",
        &[arg("text", ValueKind::Str, false)],
    ),
    meta("ui.rename", "Rename in place", &[]),
    meta("ui.path_edit", "Edit path", &[SIDE]),
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
];

/// Шаг `ui.zoom` и пределы, %.
const ZOOM_STEP: i64 = 10;
const ZOOM_RANGE: (i64, i64) = (50, 300);
/// Шаг `ui.split` и пределы доли левой панели.
const SPLIT_STEP: f32 = 0.05;
const SPLIT_RANGE: (f32, f32) = (0.1, 0.9);

/// Handler `ui`. [`UiState`] кладётся в `AppState.ext` в первом pump.
#[derive(Debug, Default)]
pub struct UiHandler;

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
        // Фокус ушёл из оверлея (клик в панель, вопрос core) — оверлей закрыт.
        let open = st.overlay;
        if open.is_some() && !matches!(focus, Focus::Overlay(_)) {
            st.close_overlay();
            cx.state.touch();
        }
    }

    fn handle(&mut self, cmd: &Command, cx: &mut Cx<'_>) -> Result<(), CmdError> {
        let a = &cmd.args;
        match cmd.id.as_str() {
            "ui.menu" => {
                let menu = a.str("menu").unwrap_or_default().into();
                open(cx, OverlayKind::Menu, |st| st.menu = Some(menu));
            }
            "ui.qsearch" => {
                let side = cx.state.active;
                let text = a.str("text").unwrap_or_default().into();
                open(cx, OverlayKind::QuickSearch, |st| {
                    st.qsearch = Some((side, text));
                });
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
            "ui.path_edit" => {
                let side = side_arg(cx, a);
                open(cx, OverlayKind::InlineEdit, |st| st.path_edit = Some(side));
            }
            "ui.draft" => {
                let draft = Draft {
                    cmd: a.str("command").unwrap_or_default().into(),
                    arg: a.str("arg").unwrap_or_default().into(),
                    text: a.str("init").unwrap_or_default().into(),
                };
                open(cx, OverlayKind::Draft, |st| st.draft = Some(draft));
            }
            "ui.cancel" => {
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
            "ui.columns" => {
                ui_state(cx).columns = Some(a.str("set").unwrap_or_default().into());
            }
            _ => return Err(CmdError::Unknown(cmd.id.as_str().into())),
        }
        Ok(())
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
    use filenav_core::SideRef as S;
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
    for w in filenav_i18n::set_language(lang) {
        if let filenav_i18n::I18nWarning::UnknownLanguage { requested, .. } = w {
            let mut args = filenav_i18n::FluentArgs::new();
            args.set("lang", requested);
            let text = filenav_i18n::current().fmt("ui-lang-unavailable", &args);
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
