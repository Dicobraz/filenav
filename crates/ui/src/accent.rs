//! Акцентная палитра ОС (S2 §1.3): семь оттенков `light3…dark3`.
//! Windows — `AccentPalette` из реестра; без неё (Win10, macOS) оттенки считаются от
//! базового цвета в OKLCH: L ±0,08 / 0,16 / 0,24 при тех же C и h, хрома уменьшается,
//! пока цвет не влезет в sRGB.

use crate::color::Color32;

/// Акцент Windows по умолчанию.
pub const DEFAULT_ACCENT: Color32 = Color32::from_rgb(0x00, 0x78, 0xD4);

/// Шаги L для `light3, light2, light1, base, dark1, dark2, dark3`.
const STEPS: [f32; 7] = [0.24, 0.16, 0.08, 0.0, -0.08, -0.16, -0.24];

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AccentPalette {
    pub light3: Color32,
    pub light2: Color32,
    pub light1: Color32,
    pub base: Color32,
    pub dark1: Color32,
    pub dark2: Color32,
    pub dark3: Color32,
}

impl Default for AccentPalette {
    fn default() -> Self {
        Self::from_base(DEFAULT_ACCENT)
    }
}

impl AccentPalette {
    /// `AccentPalette` (`REG_BINARY`, 8 × RGBA: light3 … dark3, восьмой не используется).
    pub fn from_registry(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 28 {
            return None;
        }
        let c = |i: usize| Color32::from_rgb(bytes[i * 4], bytes[i * 4 + 1], bytes[i * 4 + 2]);
        Some(Self {
            light3: c(0),
            light2: c(1),
            light1: c(2),
            base: c(3),
            dark1: c(4),
            dark2: c(5),
            dark3: c(6),
        })
    }

    /// `DWM\AccentColor` (DWORD 0xAABBGGRR) → оттенки в OKLCH.
    pub fn from_abgr(abgr: u32) -> Self {
        let [r, g, b, _a] = abgr.to_le_bytes();
        Self::from_base(Color32::from_rgb(r, g, b))
    }

    /// Оттенки от базового цвета в OKLCH.
    pub fn from_base(base: Color32) -> Self {
        let lch = Oklch::from_srgb(base);
        let s = STEPS.map(|dl| {
            if dl == 0.0 {
                base
            } else {
                lch.with_l(lch.l + dl).to_srgb()
            }
        });
        Self {
            light3: s[0],
            light2: s[1],
            light1: s[2],
            base: s[3],
            dark1: s[4],
            dark2: s[5],
            dark3: s[6],
        }
    }

    /// Оттенки по порядку `light3 … dark3`.
    pub fn shades(&self) -> [Color32; 7] {
        [
            self.light3,
            self.light2,
            self.light1,
            self.base,
            self.dark1,
            self.dark2,
            self.dark3,
        ]
    }
}

/// Цвет в OKLCH (Björn Ottosson, `OKLab`).
#[derive(Copy, Clone, Debug, PartialEq)]
struct Oklch {
    l: f32,
    c: f32,
    h: f32,
}

impl Oklch {
    #[allow(clippy::many_single_char_names, reason = "формулы OKLab")]
    fn from_srgb(c: Color32) -> Self {
        let [r, g, b] = [c.r(), c.g(), c.b()].map(to_linear);
        let l = 0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b;
        let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
        let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;
        let [l, m, s] = [l, m, s].map(f32::cbrt);
        let lab_l = 0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s;
        let a = 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s;
        let bb = 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s;
        Self {
            l: lab_l,
            c: a.hypot(bb),
            h: bb.atan2(a),
        }
    }

    fn with_l(self, l: f32) -> Self {
        Self {
            l: l.clamp(0.0, 1.0),
            ..self
        }
    }

    /// Линейный sRGB (может выйти за 0..1).
    #[allow(clippy::many_single_char_names, reason = "формулы OKLab")]
    fn linear(self, c: f32) -> [f32; 3] {
        let (a, b) = (c * self.h.cos(), c * self.h.sin());
        let l = self.l + 0.396_337_78 * a + 0.215_803_76 * b;
        let m = self.l - 0.105_561_346 * a - 0.063_854_17 * b;
        let s = self.l - 0.089_484_18 * a - 1.291_485_5 * b;
        let [l, m, s] = [l, m, s].map(|x| x * x * x);
        [
            4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
            -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
            -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
        ]
    }

    /// В sRGB; вне охвата хрома уменьшается делением пополам (L и h сохраняются).
    fn to_srgb(self) -> Color32 {
        let inside = |rgb: [f32; 3]| rgb.iter().all(|v| (-1e-5..=1.0 + 1e-5).contains(v));
        let mut c = self.c;
        if !inside(self.linear(c)) {
            let (mut lo, mut hi) = (0.0, self.c);
            for _ in 0..32 {
                let mid = f32::midpoint(lo, hi);
                if inside(self.linear(mid)) {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            c = lo;
        }
        let [r, g, b] = self.linear(c).map(from_linear);
        Color32::from_rgb(r, g, b)
    }
}

fn to_linear(c: u8) -> f32 {
    let c = f32::from(c) / 255.0;
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[allow(clippy::cast_sign_loss, reason = "значение зажато в 0..=255")]
fn from_linear(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(c: Color32) -> String {
        format!("{:02X}{:02X}{:02X}", c.r(), c.g(), c.b())
    }

    #[test]
    fn registry_palette() {
        // Синий по умолчанию с этого стенда (Win11 26300).
        let bytes = [
            0x99, 0xEB, 0xFF, 0x00, 0x4C, 0xC2, 0xFF, 0x00, 0x00, 0x91, 0xF8, 0x00, 0x00, 0x78,
            0xD4, 0x00, 0x00, 0x67, 0xC0, 0x00, 0x00, 0x3E, 0x92, 0x00, 0x00, 0x1A, 0x68, 0x00,
            0xF7, 0x63, 0x0C, 0x00,
        ];
        let p = AccentPalette::from_registry(&bytes).unwrap();
        let got: Vec<_> = p.shades().into_iter().map(hex).collect();
        assert_eq!(
            got,
            [
                "99EBFF", "4CC2FF", "0091F8", "0078D4", "0067C0", "003E92", "001A68"
            ]
        );
        assert_eq!(AccentPalette::from_registry(&bytes[..20]), None);
    }

    /// Эталон — независимый расчёт `OKLab` на Python (f64, та же подгонка хромы); f32 даёт
    /// расхождение не больше 1 в канале.
    #[test]
    fn oklch_shades_match_reference() {
        let cases = [
            (
                0xFFD4_7800_u32,
                [
                    "90C5FF", "5BABFF", "2E91EF", "0078D4", "0061AD", "004B87", "003564",
                ],
            ),
            (
                0xFF23_11E8,
                [
                    "FFB0A7", "FF847A", "FF4B47", "E81123", "C10018", "990010", "730009",
                ],
            ),
            (
                0xFF10_7C10,
                [
                    "68C863", "4EAE4A", "339530", "107C10", "006202", "004901", "003100",
                ],
            ),
            (
                0xFF80_8080,
                [
                    "CACACA", "B1B1B1", "989898", "808080", "696969", "525252", "3D3D3D",
                ],
            ),
        ];
        for (abgr, want) in cases {
            let got = AccentPalette::from_abgr(abgr).shades();
            for (g, w) in got.iter().zip(want) {
                let w = u32::from_str_radix(w, 16).unwrap().to_be_bytes();
                let diff = [g.r(), g.g(), g.b()]
                    .iter()
                    .zip(&w[1..])
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap();
                assert!(diff <= 1, "{abgr:08X}: {} vs {w:02X?}", hex(*g));
            }
        }
    }

    #[test]
    fn shades_get_lighter_to_darker() {
        for base in [
            DEFAULT_ACCENT,
            Color32::from_rgb(255, 255, 0),
            Color32::BLACK,
        ] {
            let l: Vec<f32> = AccentPalette::from_base(base)
                .shades()
                .iter()
                .map(|c| Oklch::from_srgb(*c).l)
                .collect();
            assert!(l.windows(2).all(|w| w[0] >= w[1] - 1e-3), "{base:?}: {l:?}");
        }
    }
}
