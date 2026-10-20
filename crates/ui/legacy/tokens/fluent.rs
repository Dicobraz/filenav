//! Значения ролей: нейтральные и статусные — WinUI 3 Common theme resources
//! (`#AARRGGBB`), раскраска по типам — S0-decisions №32. Акцент подставляет `palette`.

use egui::{Color32, Theme};

use super::{Palette, Role, argb};
use crate::legacy::sys::HcColors;

/// (роль, light, dark). Акцентные роли здесь — заглушки, их задаёт палитра акцента.
const TABLE: &[(Role, u32, u32)] = &[
    (Role::AccentFill, 0xFF00_67C0, 0xFF4C_C2FF),
    (Role::AccentFillHover, 0xE600_67C0, 0xE64C_C2FF),
    (Role::AccentFillPressed, 0xCC00_67C0, 0xCC4C_C2FF),
    (Role::AccentText, 0xFF00_3E92, 0xFF99_EBFF),
    (Role::AccentOn, 0xFFFF_FFFF, 0xFF00_0000),
    (Role::AccentSubtle, 0x1F00_78D4, 0x294C_C2FF),
    // TextFillColorPrimary / Secondary / Tertiary / Disabled.
    (Role::TextPrimary, 0xE400_0000, 0xFFFF_FFFF),
    (Role::TextSecondary, 0x9E00_0000, 0xC5FF_FFFF),
    (Role::TextTertiary, 0x7200_0000, 0x8BFF_FFFF),
    (Role::TextDisabled, 0x5C00_0000, 0x5DFF_FFFF),
    // SolidBackgroundFillColorBase / Secondary.
    (Role::BgBase, 0xFFF3_F3F3, 0xFF20_2020),
    (Role::BgBaseAlt, 0xFFEE_EEEE, 0xFF1C_1C1C),
    // Карточки панелей непрозрачные (S0-decisions №33).
    (Role::LayerPanel, 0xFFFF_FFFF, 0xFF27_2727),
    (Role::LayerOverlay, 0xFFF9_F9F9, 0xFF2C_2C2C),
    // SubtleFillColorSecondary / Tertiary.
    (Role::FillSubtleHover, 0x0900_0000, 0x0FFF_FFFF),
    (Role::FillSubtlePressed, 0x0600_0000, 0x0AFF_FFFF),
    (Role::FillCursor, 0x0F00_0000, 0x15FF_FFFF),
    (Role::FillCursorPassive, 0x0600_0000, 0x0AFF_FFFF),
    // ControlFillColorDefault / InputActive.
    (Role::FillControl, 0xB3FF_FFFF, 0x0FFF_FFFF),
    (Role::FillControlInput, 0xFFFF_FFFF, 0xFF1E_1E1E),
    // CardStrokeColorDefault, ControlStrokeColorDefault, ControlStrongStrokeColorDefault,
    // DividerStrokeColorDefault.
    (Role::StrokeCard, 0x0F00_0000, 0x1900_0000),
    (Role::StrokeControl, 0x0F00_0000, 0x12FF_FFFF),
    (Role::StrokeControlBottom, 0x7200_0000, 0x8BFF_FFFF),
    (Role::StrokeStrong, 0x7200_0000, 0x8BFF_FFFF),
    (Role::StrokeDivider, 0x1400_0000, 0x15FF_FFFF),
    (Role::ShadowOverlay, 0x2400_0000, 0x4200_0000),
    // SystemFillColor{Critical,Caution,Success}{,Background}, SystemFillColorAttentionBackground.
    (Role::CriticalFg, 0xFFC4_2B1C, 0xFFFF_99A4),
    (Role::CriticalBg, 0xFFFD_E7E9, 0xFF44_2726),
    (Role::CautionFg, 0xFF9D_5D00, 0xFFFC_E100),
    (Role::CautionBg, 0xFFFF_F4CE, 0xFF43_3519),
    (Role::SuccessFg, 0xFF0F_7B0F, 0xFF6C_CB5F),
    (Role::SuccessBg, 0xFFDF_F6DD, 0xFF39_3D1B),
    (Role::InfoBg, 0x80F6_F6F6, 0x08FF_FFFF),
    // Мягкие цвета по типам (S0-decisions №32).
    (Role::TypeArchive, 0xFF87_64B8, 0xFFC6_B1E8),
    (Role::TypeExec, 0xFF0F_7B0F, 0xFF6C_CB5F),
    (Role::TypeTemp, 0xFF9D_5D00, 0xFFFC_E100),
    (Role::TypeMuted, 0x7200_0000, 0x8BFF_FFFF),
];

pub(super) fn neutral(theme: Theme) -> Palette {
    let mut p = Palette([[0; 4]; Role::COUNT]);
    for &(role, light, dark) in TABLE {
        p.set(role, argb(if theme == Theme::Dark { dark } else { light }));
    }
    p
}

/// Высокая контрастность: только системные цвета (`GetSysColor`), без полупрозрачности.
pub(super) fn high_contrast(hc: &HcColors) -> Palette {
    let raw = |c: Color32| c.to_srgba_unmultiplied();
    let mut p = Palette([raw(hc.window_text); Role::COUNT]);
    for role in [
        Role::BgBase,
        Role::BgBaseAlt,
        Role::LayerPanel,
        Role::LayerOverlay,
        Role::FillControl,
        Role::FillControlInput,
        Role::CriticalBg,
        Role::CautionBg,
        Role::SuccessBg,
        Role::InfoBg,
    ] {
        p.set(role, raw(hc.window));
    }
    for role in [
        Role::FillSubtleHover,
        Role::FillSubtlePressed,
        Role::FillCursorPassive,
        Role::ShadowOverlay,
    ] {
        p.set(role, [0; 4]);
    }
    for role in [
        Role::AccentFill,
        Role::AccentFillHover,
        Role::AccentFillPressed,
        Role::AccentSubtle,
        Role::FillCursor,
    ] {
        p.set(role, raw(hc.highlight));
    }
    p.set(Role::AccentOn, raw(hc.highlight_text));
    p.set(Role::AccentText, raw(hc.highlight_text));
    p.set(Role::TextDisabled, raw(hc.gray_text));
    p.set(Role::TypeMuted, raw(hc.gray_text));
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_role_has_a_value() {
        let roles: Vec<Role> = TABLE.iter().map(|(r, _, _)| *r).collect();
        assert_eq!(roles, Role::ALL, "TABLE — все роли по порядку enum");
    }
}
