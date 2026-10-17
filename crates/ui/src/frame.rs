//! Базовый слой кадра через [`Canvas`] (S2-07): регионы, подложки карточек, баннеров,
//! job-бара, cmdline. Виджеты (табы, строки, кнопки) рисуются поверх в своих регионах.
//! Порядок — слои S0 v5 №3: терминал → панели → cmdline → оверлеи.

use filenav_core::{AppState, Level, PromptDetail, Side};

use crate::canvas::Canvas;
use crate::fonts::Font;
use crate::layout::Regions;
use crate::state::UiState;
use crate::text::Tx;
use crate::tokens::{Metric, Role, Tokens};

/// Подложки регионов. `prompt` — строка приглашения cmdline (`C:\work>`).
pub fn paint(
    c: &mut dyn Canvas,
    r: &Regions,
    t: &Tokens,
    s: &AppState,
    ui: &UiState,
    prompt: &str,
) {
    let overlay = t.radius(Metric::RadiusOverlay);
    let control = t.radius(Metric::RadiusControl);
    let thin = t.m(Metric::StrokeThin);

    c.region("terminal", r.terminal);
    if ui.panels_hidden {
        // Вывод терминала — S6; пока пустая подложка под скрытыми панелями.
        c.rect(r.terminal, Role::LayerPanel, overlay);
    }
    c.region("titlebar", r.titlebar);

    let mut banners = r.banners.iter();
    if let Some(b) = &s.banner
        && let Some(rect) = banners.next()
    {
        c.region("banner", *rect);
        let bg = match b.level {
            Level::Error => Role::CriticalBg,
            Level::Warn => Role::CautionBg,
            Level::Info => Role::InfoBg,
        };
        c.rect(*rect, bg, overlay);
        c.stroke(*rect, Role::StrokeCard, overlay, thin);
    }
    if let Some(p) = s
        .prompts
        .active()
        .filter(|p| !ui.prompt_hidden.contains(&p.id))
        && let Some(rect) = banners.next()
    {
        c.region("prompt", *rect);
        let bg = match p.spec.detail {
            Some(PromptDetail::Conflict { .. }) => Role::CautionBg,
            None => Role::InfoBg,
        };
        c.rect(*rect, bg, overlay);
        c.stroke(*rect, Role::StrokeCard, overlay, thin);
    }

    for side in [Side::Left, Side::Right] {
        let Some(card) = r.panel(side) else {
            continue;
        };
        c.region(
            if side == Side::Left {
                "panel.left"
            } else {
                "panel.right"
            },
            card,
        );
        c.rect(card, Role::LayerPanel, overlay);
        c.stroke(card, Role::StrokeCard, overlay, thin);
        if s.active == side {
            // Акцентная полоса активной панели — верх карточки (`w.accent_bar`).
            let inset = t.m(Metric::RadiusOverlay);
            let bar = egui::Rect::from_min_size(
                card.min + egui::vec2(inset, 0.0),
                egui::vec2(
                    (card.width() - 2.0 * inset).max(0.0),
                    t.m(Metric::WAccentBar),
                ),
            );
            c.rect(bar, Role::AccentFill, egui::CornerRadius::same(1));
        }
    }
    if let Some(sp) = r.splitter {
        c.region("splitter", sp);
    }
    for job in &r.jobs {
        c.region("job", *job);
        c.rect(*job, Role::LayerPanel, control);
    }
    c.region("cmdline", r.cmdline);
    c.text(
        r.cmdline.left_center() + egui::vec2(t.m(Metric::PadRowX), 0.0),
        egui::Align2::LEFT_CENTER,
        &Tx::user(prompt),
        Font::Mono,
        t.m(Metric::TypeMono),
        Role::TextSecondary,
    );
    c.region("keybar", r.keybar);
}
