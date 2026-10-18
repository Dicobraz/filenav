//! Urfin — UI-фронтенд на egui (S2).
//!
//! [`UrfinUi`] — две карточки панелей, баннер, вопрос, тосты и keybar (каркас S2-02);
//! вид — из токенов Fluent ([`tokens`], S2-05) по теме и акценту ОС ([`sys`], S2-03).
//! Handler `ui` и [`UiState`] в `AppState.ext` — [`register`]. Ввод панели — в app
//! (`raw_input_hook`, до keymap S3); здесь — мышь. Тексты — только через [`text`]
//! (`clippy.toml`: `disallowed-methods`).

pub mod accent;
pub mod breadcrumb;
pub mod cache;
pub mod canvas;
pub mod chrome;
pub mod commands_doc;
pub mod config;
pub(crate) mod file_list;
pub mod fmt_cell;
pub mod fonts;
mod frame;
mod fs_tasks;
mod handler;
pub mod highlight;
pub mod icons;
pub mod keybar;
pub mod layout;
pub mod menu;
mod panel;
mod prompt_bar;
pub mod state;
pub mod sys;
mod tabs;
pub mod text;
mod titlebar;
pub mod tokens;

use std::collections::VecDeque;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use egui::{Align2, Sense};
use urfin_core::{AppState, Command, CommandRegistry, ConfigWarning, Core, Level, Side, UiEvent};
use urfin_i18n::I18n;

pub use handler::UiHandler;
pub use prompt_bar::prompt_takes_keys;
pub use state::{Draft, ThemeMode, UiState};

use crate::config::UiCfg;
use crate::fonts::{Font, FontSet};
use crate::highlight::Highlighter;
use crate::panel::CardMemo;
use crate::text::Tx;
use crate::tokens::{Metric, Role, Tokens};

/// Сколько показывается тост.
const TOAST_TTL: Duration = Duration::from_secs(4);
/// Не больше стольких тостов сразу (S2 §3).
const TOASTS_MAX: usize = 3;

/// Регистрирует handler `ui`; `UiState` появится в `ext` в первом pump.
pub fn register(core: &mut Core) {
    core.register(Box::new(UiHandler::default()));
}

/// Фронтенд окна: только читает `AppState`, действия — командами.
#[derive(Debug)]
pub struct UrfinUi {
    toasts: VecDeque<(String, Instant)>,
    cards: [CardMemo; 2],
    keybar: keybar::Keybar,
    /// Подсказки клавиш: заглушка Far до keymap S3 (S3-04).
    hints: Box<dyn keybar::KeyHints>,
    registered: Vec<&'static str>,
    /// Реестр команд (копия) — проверка `menu.toml` из каталога конфига.
    commands: CommandRegistry,
    /// Тема, акцент и контрастность ОС (probe, затем `sys::watch`).
    look: sys::Look,
    look_rx: Option<mpsc::Receiver<sys::Look>>,
    cfg: UiCfg,
    /// Тема из `[ui] theme`; команда `ui.theme` её перекрывает.
    cfg_theme: ThemeMode,
    tokens: Tokens,
    highlighter: Highlighter,
    fonts: FontSet,
    /// Файлы fallback-шрифтов из probe (читаются в фоне после первого кадра).
    font_files: sys::FontFiles,
    fallback_rx: Option<mpsc::Receiver<Vec<fonts::FallbackFont>>>,
    frames: u64,
    /// Ширины колонок табов (`FileList`).
    widths: file_list::Widths,
    /// Наборы табов в заголовке: прокрутка и перетаскивание (S2-13).
    tabs: [tabs::TabsMemo; 2],
    /// Разметка ячеек списка (S2 §6.2), общая для обеих панелей.
    rows: cache::RowCache,
    /// Эпоха стиля: тема, акцент, шрифты, масштаб, DPI — сброс `RowCache`.
    style_epoch: u64,
    last_ppp: f32,
    /// Заполненность атласа шрифтов в конце прошлого кадра: упала — egui пересоздал
    /// атлас, galley в `RowCache` ссылаются на старые места текстуры.
    atlas_fill: f32,
    /// Языковые шаблоны ячеек и эпоха языка, для которой собраны.
    fmt: (u64, fmt_cell::FmtCfg),
    /// Применённые настройки вида: тема, масштаб, `look` ОС.
    applied: Option<(ThemeMode, u16, sys::Look)>,
    /// Своя рамка окна (Windows); `None` — системная.
    chrome: Option<chrome::ChromeHandle>,
    /// Меню F9 (S2-14): из `menu.toml`, текущий пункт.
    menus: Vec<menu::MenuDef>,
    menu_memo: menu::MenuMemo,
    /// Баннер-вопрос и черновик (S2-15).
    bar: prompt_bar::BarMemo,
}

impl UrfinUi {
    /// `registry` — команды всех handlers (keybar гасит незарегистрированные);
    /// `probe` — [`sys::probe_system`]; `cfg` — секция `[ui]`; `fonts` — [`fonts::install`]
    /// (без него — шрифты egui). Предупреждения probe и конфига показываются тостами.
    pub fn new(
        registry: &CommandRegistry,
        probe: &sys::SysProbe,
        cfg: &UiCfg,
        fonts: FontSet,
    ) -> Self {
        let (tokens, mut warnings) = Tokens::build(probe, cfg);
        let (highlighter, more) = Highlighter::build(&cfg.colors);
        warnings.extend(more);
        let cfg_theme = match cfg.theme.trim().to_ascii_lowercase().as_str() {
            "" | "system" => ThemeMode::System,
            "light" => ThemeMode::Light,
            "dark" => ThemeMode::Dark,
            other => {
                warnings.push(ConfigWarning {
                    key: "ui.theme".to_owned(),
                    msg: format!("expected system | light | dark, got {other:?}"),
                });
                ThemeMode::System
            }
        };
        let registered = keybar::registered(registry);
        let (menus, menu_warnings) =
            menu::parse(menu::MENU_TOML, registry).expect("built-in menu.toml parses (menu_tests)");
        let far = Instant::now() + TOAST_TTL;
        let toasts = probe
            .warnings
            .iter()
            .chain(&warnings)
            .map(|w| (format!("{}: {}", w.key, w.msg), far))
            .chain(menu_warnings.into_iter().map(|w| (w, far)))
            .chain(
                menu::letter_conflicts(&menus, &urfin_i18n::current())
                    .into_iter()
                    .map(|w| (w, far)),
            )
            .collect();
        Self {
            menus,
            menu_memo: menu::MenuMemo::default(),
            bar: prompt_bar::BarMemo::default(),
            toasts,
            cards: [CardMemo::default(), CardMemo::default()],
            keybar: keybar::Keybar::default(),
            hints: Box::new(keybar::FarStub),
            registered,
            commands: menu::copy_registry(registry),
            look: probe.look,
            look_rx: None,
            cfg: cfg.clone(),
            cfg_theme,
            tokens,
            highlighter,
            fonts,
            font_files: probe.fonts.clone(),
            fallback_rx: None,
            chrome: None,
            frames: 0,
            widths: file_list::Widths::default(),
            tabs: Default::default(),
            rows: cache::RowCache::new(),
            style_epoch: 0,
            last_ppp: 0.0,
            atlas_fill: 0.0,
            fmt: (
                urfin_i18n::epoch(),
                fmt_cell::FmtCfg::new(
                    &urfin_i18n::current(),
                    cfg.date_format.eq_ignore_ascii_case("iso"),
                    sys::Zone::system(),
                ),
            ),
            applied: None,
        }
    }

    /// Меню из `menu.toml` каталога конфига вместо вшитого; ошибки — тостами, при
    /// ошибке разбора остаётся вшитое.
    pub fn set_menu(&mut self, text: &str) {
        let far = Instant::now() + TOAST_TTL;
        match menu::parse(text, &self.commands) {
            Ok((menus, warnings)) => {
                let letters = menu::letter_conflicts(&menus, &urfin_i18n::current());
                self.menus = menus;
                self.toasts
                    .extend(warnings.into_iter().chain(letters).map(|w| (w, far)));
            }
            Err(e) => self.toasts.push_back((format!("menu.toml: {e}"), far)),
        }
    }

    /// Первый кадр — прогрев атласа и проверка цифр; второй — фоновое чтение
    /// fallback-шрифтов; дальше — их установка, когда прочитаны.
    fn fonts_step(&mut self, ctx: &egui::Context) {
        self.frames += 1;
        if self.frames == 1 && self.fonts.installed() {
            let t = &self.tokens;
            self.fonts.warm_up(
                ctx,
                &[
                    (Font::Ui, t.m(Metric::TypeBody)),
                    (Font::Strong, t.m(Metric::TypeBody)),
                    (Font::Caption, t.m(Metric::TypeCaption)),
                    (Font::Subtitle, t.m(Metric::TypeSubtitle)),
                    (Font::Mono, t.m(Metric::TypeMono)),
                    (Font::Icons, t.m(Metric::WIcon)),
                ],
                &icons::all_text(),
            );
        }
        if self.frames == 2 && self.fonts.installed() {
            let (tx, rx) = mpsc::channel();
            let files = self.font_files.clone();
            let cfg = self.cfg.clone();
            let ctx = ctx.clone();
            let spawned = std::thread::Builder::new()
                .name("font-fallback".into())
                .spawn(move || {
                    let (list, warnings) = fonts::load_fallbacks(&files, &cfg);
                    for w in warnings {
                        tracing::warn!(key = %w.key, msg = %w.msg, "fallback font");
                    }
                    if tx.send(list).is_ok() {
                        ctx.request_repaint();
                    }
                });
            if spawned.is_ok() {
                self.fallback_rx = Some(rx);
            }
        }
        if let Some(list) = self.fallback_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            fonts::add_fallbacks(ctx, &list);
            self.style_epoch += 1;
            self.fallback_rx = None;
        }
    }

    /// Токены текущего вида (тема — та, что у egui в этом кадре).
    pub fn tokens(&self) -> &Tokens {
        &self.tokens
    }

    /// Своя рамка окна ([`sys::install_window_chrome`]): кнопки caption в заголовке,
    /// при Mica — прозрачный `bg.base`.
    pub fn set_chrome(&mut self, chrome: Option<chrome::ChromeHandle>) {
        let mica = chrome.as_ref().is_some_and(|c| c.mica);
        self.chrome = chrome;
        self.tokens.set_mica(mica);
        // Стиль пересоберётся со следующим кадром.
        self.applied = None;
    }

    /// Следить за темой и акцентом ОС ([`sys::watch`]); без этого — только probe.
    pub fn watch_system(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        sys::watch(ctx, tx);
        self.look_rx = Some(rx);
    }

    /// Кадр (ADR-0011): `root` — корневой `Ui` окна от eframe.
    #[allow(clippy::too_many_lines, reason = "кадр: слои окна по порядку")]
    pub fn show(
        &mut self,
        root: &mut egui::Ui,
        s: &AppState,
        ev: &[UiEvent],
        out: &mut Vec<Command>,
    ) {
        let ctx = root.ctx().clone();
        let i18n = urfin_i18n::current();
        let fallback = UiState::new();
        let st = s.ext.get::<UiState>().unwrap_or(&fallback);
        self.apply_look(&ctx, st);
        self.fonts_step(&ctx);
        let ppp = ctx.pixels_per_point();
        if (ppp - self.last_ppp).abs() > f32::EPSILON {
            // Другой монитор или масштаб ОС: разметка ячеек — заново (S2 §5).
            self.last_ppp = ppp;
            self.style_epoch += 1;
        }
        let fill = atlas_fill(&ctx);
        if fill + 1e-6 < self.atlas_fill {
            self.style_epoch += 1;
        }
        self.rows.sync_epochs(urfin_i18n::epoch(), self.style_epoch);
        self.tokens.set_theme(ctx.theme());
        self.take_toasts(&ctx, ev);
        self.take_drafts(ev, out);
        let epoch = urfin_i18n::epoch();
        self.keybar
            .refresh(&i18n, epoch, &self.registered, self.hints.as_ref());
        if self.fmt.0 != epoch {
            let iso = self.cfg.date_format.eq_ignore_ascii_case("iso");
            self.fmt = (
                epoch,
                fmt_cell::FmtCfg::new(&i18n, iso, sys::Zone::system()),
            );
        }

        if let Some(c) = &self.chrome {
            c.sync_dark(ctx.theme() == egui::Theme::Dark);
        }
        let t = &self.tokens;
        let gap = t.m(Metric::GapPanels);
        // Регионы окна (S2-07) и базовый слой через Canvas; виджеты — в регионах.
        let screen = root.max_rect();
        let regions = layout::Layout::compute(screen, t, s, st);
        root.painter().rect_filled(screen, 0.0, t.c(Role::BgBase));
        let prompt = format!("{}>", panel::path_text(s.active_tab()));
        frame::paint(
            &mut canvas::EguiCanvas {
                painter: root.painter().clone(),
                tokens: t,
                fonts: &self.fonts,
                i18n: &i18n,
            },
            &regions,
            t,
            s,
            st,
            &prompt,
        );
        let mut child = |r: egui::Rect| root.new_child(egui::UiBuilder::new().max_rect(r));
        let mut title = titlebar::TitleBar {
            i18n: &i18n,
            tokens: t,
            fonts: &self.fonts,
            chrome: self.chrome.as_ref(),
            columns: regions.panels,
            memo: &mut self.tabs,
            menu: menu::open_id(st).map(|open| titlebar::MenuBar {
                menus: &self.menus,
                open,
                memo: &mut self.menu_memo,
            }),
        };
        title.show(&mut child(regions.titlebar), s, out);
        self.paint_keybar(&mut child(regions.keybar), &i18n, out);
        if let (Some(first), Some(last)) = (regions.banners.first(), regions.banners.last()) {
            let mut ui = child(first.union(*last).shrink(gap));
            top_bars(&mut ui, &i18n, &self.tokens, &self.fonts, s, st, out);
        }
        self.paint_jobs(root, &regions, &i18n, s);
        if let (Some(r), Some(b)) = (regions.prompt, prompt_bar::bar(s, st)) {
            let mut c = canvas::EguiCanvas {
                painter: root.painter().clone(),
                tokens: &self.tokens,
                fonts: &self.fonts,
                i18n: &i18n,
            };
            let mut ui = root.new_child(egui::UiBuilder::new().max_rect(r));
            prompt_bar::show(
                &mut c,
                &mut prompt_bar::BarCx {
                    ui: &mut ui,
                    i18n: &i18n,
                    tokens: &self.tokens,
                    fonts: &self.fonts,
                    fmt: &self.fmt.1,
                    memo: &mut self.bar,
                },
                r,
                b,
                s,
                out,
            );
        } else if st.draft.is_none() {
            self.bar.ops = None;
        }
        // Меню breadcrumb — до карточек: клик по другому `›` закрывает его и открывает новое.
        breadcrumb::menu(
            &ctx,
            &i18n,
            &self.tokens,
            st,
            [
                (&self.cards[0].crumbs, Side::Left),
                (&self.cards[1].crumbs, Side::Right),
            ],
            out,
        );
        for side in [Side::Left, Side::Right] {
            if let Some(r) = regions.panel(side) {
                self.card(root, r, &i18n, s, st, side, out);
            }
        }
        if let Some(sp) = regions.splitter {
            let resp = root.interact(sp, root.scope_id().with("splitter"), Sense::click());
            if resp.hovered() {
                root.ctx()
                    .set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if resp.double_clicked() {
                out.push(Command::new("ui.split").arg("reset", true));
            }
        }
        self.menu_flyout(&ctx, &i18n, st, regions.titlebar, out);
        self.paint_toasts(&ctx, &i18n);
        self.rows.end_frame();
        self.atlas_fill = atlas_fill(&ctx);
    }

    /// Счётчики `RowCache` (тесты, перф-лог).
    pub fn row_cache_stats(&self) -> (cache::CacheStats, usize) {
        (self.rows.stats, self.rows.len())
    }

    fn apply_look(&mut self, ctx: &egui::Context, st: &UiState) {
        if let Some(rx) = &self.look_rx {
            while let Ok(look) = rx.try_recv() {
                self.look = look;
            }
        }
        let mode = st.theme.unwrap_or(self.cfg_theme);
        let want = (mode, st.zoom, self.look);
        if self.applied == Some(want) {
            return;
        }
        if self.applied.is_none_or(|(_, _, look)| look != self.look) {
            let probe = sys::SysProbe {
                look: self.look,
                ..sys::SysProbe::default()
            };
            // Предупреждения конфига уже показаны при создании.
            self.tokens = Tokens::build(&probe, &self.cfg).0;
            self.tokens
                .set_mica(self.chrome.as_ref().is_some_and(|c| c.mica));
            tokens::style::apply(ctx, &self.tokens, &self.fonts);
        }
        self.applied = Some(want);
        self.style_epoch += 1;
        ctx.set_theme(match (mode, self.look.theme) {
            (ThemeMode::Light, _) | (ThemeMode::System, Some(egui::Theme::Light)) => {
                egui::ThemePreference::Light
            }
            (ThemeMode::Dark, _) | (ThemeMode::System, Some(egui::Theme::Dark)) => {
                egui::ThemePreference::Dark
            }
            (ThemeMode::System, None) => egui::ThemePreference::System,
        });
        ctx.set_zoom_factor(f32::from(st.zoom) / 100.0);
    }

    /// `ops.draft` (S0 v3 №15): операция без подтверждения — черновик внизу.
    fn take_drafts(&mut self, ev: &[UiEvent], out: &mut Vec<Command>) {
        for e in ev {
            let UiEvent::Ext { id, args } = e else {
                continue;
            };
            if id.as_str() != "ops.draft" {
                continue;
            }
            let command = args.str("command").unwrap_or_default().to_owned();
            let arg = match command.as_str() {
                "ops.copy" | "ops.move" => "dest",
                "ops.mkdir" => "name",
                _ => "confirmed",
            };
            out.push(
                Command::new("ui.draft")
                    .arg("command", command.as_str())
                    .arg("arg", arg),
            );
            self.bar.ops = Some(prompt_bar::OpsDraft {
                command,
                permanent: args.bool("permanent").unwrap_or(false),
            });
        }
    }

    fn take_toasts(&mut self, ctx: &egui::Context, ev: &[UiEvent]) {
        let now = Instant::now();
        for e in ev {
            if let UiEvent::Toast { text, .. } = e {
                self.toasts.push_back((text.clone(), now + TOAST_TTL));
            }
        }
        while self.toasts.len() > TOASTS_MAX {
            self.toasts.pop_front();
        }
        self.toasts.retain(|(_, until)| *until > now);
        if let Some((_, until)) = self.toasts.front() {
            ctx.request_repaint_after(until.saturating_duration_since(now));
        }
    }

    /// Карточка панели: `FileList` (S2-10) в прямоугольнике раскладки.
    #[allow(
        clippy::too_many_arguments,
        reason = "карточка: состояние, UI и регион"
    )]
    fn card(
        &mut self,
        root: &mut egui::Ui,
        r: egui::Rect,
        i18n: &I18n,
        s: &AppState,
        st: &UiState,
        side: Side,
        out: &mut Vec<Command>,
    ) {
        let p = s.panel(side);
        if !p.visible {
            return;
        }
        let mut c = canvas::EguiCanvas {
            painter: root.painter().clone(),
            tokens: &self.tokens,
            fonts: &self.fonts,
            i18n,
        };
        let mut ui = root.new_child(egui::UiBuilder::new().max_rect(r));
        let mut cx = file_list::ListCx {
            ui: &mut ui,
            i18n,
            tokens: &self.tokens,
            fonts: &self.fonts,
            hl: &self.highlighter,
            fmt: &self.fmt.1,
            cache: &mut self.rows,
            memo: &mut self.cards[side.idx()],
            widths: &mut self.widths,
            st,
            keys: s.ext.get::<urfin_keymap::KeymapView>(),
            conflict: prompt_bar::conflict_dst(s, st)
                .filter(|(dst_side, _)| *dst_side == side)
                .map(|(_, name)| name),
        };
        file_list::show(&mut c, r, p.tab(), side, s.active == side, &mut cx, out);
    }

    fn paint_jobs(&self, root: &egui::Ui, regions: &layout::Regions, i18n: &I18n, s: &AppState) {
        let t = &self.tokens;
        let font = self.fonts.id(Font::Caption, t.m(Metric::TypeCaption));
        let active = s.jobs.iter().filter(|j| j.state.is_active());
        for (job, row) in active.zip(&regions.jobs) {
            text::paint_clipped(
                root.painter(),
                i18n,
                row.left_center() + egui::vec2(t.m(Metric::PadRowX), 0.0),
                row.width() - 2.0 * t.m(Metric::PadRowX),
                &Tx::user(job.title.as_str()),
                font.clone(),
                t.c(Role::TextPrimary),
            );
        }
    }

    /// Выпадающий список меню F9 и щит под ним: клик мимо закрывает меню и не уходит в
    /// панель (S2 §2.6). ← → — соседнее меню.
    fn menu_flyout(
        &mut self,
        ctx: &egui::Context,
        i18n: &I18n,
        st: &UiState,
        titlebar: egui::Rect,
        out: &mut Vec<Command>,
    ) {
        let Some(open) = menu::open_id(st) else {
            return;
        };
        let Some(def) = self.menus.iter().find(|m| m.id == open) else {
            return;
        };
        let (left, right) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
            )
        });
        for (pressed, dir) in [(left, -1), (right, 1)] {
            if pressed && let Some(next) = menu::neighbor(&self.menus, open, dir) {
                out.push(Command::new("ui.menu").arg("menu", next.as_str()));
            }
        }
        let screen = ctx.content_rect();
        let shield = screen.with_min_y(titlebar.bottom());
        egui::Area::new(egui::Id::unique("menu-shield"))
            .order(egui::Order::Foreground)
            .fixed_pos(shield.min)
            .show(ctx, |ui| {
                let r = ui.allocate_rect(shield, Sense::click());
                if r.clicked() || r.secondary_clicked() || r.middle_clicked() {
                    out.push(Command::new("ui.cancel"));
                }
            });
        let at = self
            .menu_memo
            .anchors
            .iter()
            .find(|(id, _)| id == open)
            .map_or(titlebar.left_bottom(), |(_, p)| *p);
        egui::Area::new(egui::Id::unique("menu-flyout"))
            .order(egui::Order::Tooltip)
            .fixed_pos(at)
            .show(ctx, |ui| {
                let mut c = canvas::EguiCanvas {
                    painter: ui.painter().clone(),
                    tokens: &self.tokens,
                    fonts: &self.fonts,
                    i18n,
                };
                let r = menu::flyout(
                    &mut c,
                    ui,
                    i18n,
                    &self.tokens,
                    &self.fonts,
                    self.hints.as_ref(),
                    def,
                    &mut self.menu_memo,
                    at,
                    out,
                );
                ui.allocate_rect(r, Sense::hover());
            });
    }

    fn paint_keybar(&self, ui: &mut egui::Ui, i18n: &I18n, out: &mut Vec<Command>) {
        let mods = keybar::Mods::from_egui(ui.input(|i| i.modifiers));
        let mut c = canvas::EguiCanvas {
            painter: ui.painter().clone(),
            tokens: &self.tokens,
            fonts: &self.fonts,
            i18n,
        };
        keybar::show(&mut c, ui, &self.keybar, mods, &self.tokens, out);
    }

    /// Уведомление от приложения (например, откат рендера на WARP, S0 №50): текст
    /// уже переведён. Полные тосты №51 — S2-17.
    pub fn toast(&mut self, text: String) {
        self.toasts.push_back((text, Instant::now() + TOAST_TTL));
    }

    fn paint_toasts(&self, ctx: &egui::Context, i18n: &I18n) {
        if self.toasts.is_empty() {
            return;
        }
        egui::Area::new(egui::Id::unique("toasts"))
            .anchor(
                Align2::RIGHT_BOTTOM,
                egui::vec2(
                    -self.tokens.m(Metric::SpaceM),
                    -(self.tokens.m(Metric::HKeybar) + self.tokens.m(Metric::SpaceL)),
                ),
            )
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                for (msg, _) in &self.toasts {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        text::label(ui, i18n, &Tx::user(msg.as_str()));
                    });
                }
            });
    }
}

/// Заполненность атласа шрифтов egui.
#[allow(
    clippy::redundant_closure_for_method_calls,
    reason = "путь метода не выводит время жизни FontsView"
)]
fn atlas_fill(ctx: &egui::Context) -> f32 {
    ctx.fonts(|f| f.font_atlas_fill_ratio())
}

/// Баннер core над карточками (вопросы — `prompt_bar` внизу). Тексты уже переведены core.
fn top_bars(
    ui: &mut egui::Ui,
    i18n: &I18n,
    t: &Tokens,
    fonts: &FontSet,
    s: &AppState,
    st: &UiState,
    out: &mut Vec<Command>,
) {
    let strong = fonts.id(Font::Strong, t.m(Metric::TypeBody));
    if let Some(b) = &s.banner {
        let fg = match b.level {
            Level::Error => Role::CriticalFg,
            Level::Warn => Role::CautionFg,
            Level::Info => Role::TextPrimary,
        };
        ui.horizontal_wrapped(|ui| {
            text::label_styled(ui, i18n, &Tx::user(b.text.as_str()), Some(t.c(fg)), None);
            for (label, cmd) in &b.actions {
                if text::button(ui, i18n, &Tx::user(label.as_str())).clicked() {
                    out.push(cmd.clone());
                }
            }
        });
        ui.add_space(4.0);
    }
    let _ = (strong, st);
}

/// Версия крейта из `Cargo.toml`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests;
