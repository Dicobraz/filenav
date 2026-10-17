//! Natural sort (S4 §4.2, решение владельца №10): без учёта регистра, цифровые
//! группы — по значению, `ё` сразу после `е`.
//!
//! Уровни: (1) символы со свёрткой регистра и `ё` = `е`, цифровые группы по
//! значению; (2) первая по позиции вторичная разница — ведущие нули (меньше
//! нулей — раньше) или `ё` после `е`. [`name_cmp`] добавляет (3) сырые байты —
//! полный порядок.

use std::cmp::Ordering;

/// Сравнение имён по уровням 1–2: `Equal` только для имён, различающихся лишь
/// регистром.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    cmp_with(a, b, true)
}

/// Полный порядок: [`natural_cmp`] (или без чисел при `natural = false`), затем
/// сырые байты — детерминизм для одинаковых без учёта регистра имён.
pub fn name_cmp(a: &str, b: &str, natural: bool) -> Ordering {
    cmp_with(a, b, natural).then_with(|| a.as_bytes().cmp(b.as_bytes()))
}

/// Уровни 1–2; `natural = false` — цифры обычными символами.
pub fn cmp_with(a: &str, b: &str, natural: bool) -> Ordering {
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);
    let mut secondary = Ordering::Equal;
    loop {
        let (Some(&ca), Some(&cb)) = (ab.get(i), bb.get(j)) else {
            // Короче — раньше; при равной длине — вторичная разница.
            return (ab.len() - i).cmp(&(bb.len() - j)).then(secondary);
        };
        if natural && ca.is_ascii_digit() && cb.is_ascii_digit() {
            let (ord, zeros, ni, nj) = digits(ab, i, bb, j);
            if ord != Ordering::Equal {
                return ord;
            }
            if secondary == Ordering::Equal {
                // Меньше ведущих нулей — раньше.
                secondary = zeros;
            }
            (i, j) = (ni, nj);
            continue;
        }
        if ca < 0x80 && cb < 0x80 {
            let ord = ca.to_ascii_lowercase().cmp(&cb.to_ascii_lowercase());
            if ord != Ordering::Equal {
                return ord;
            }
            i += 1;
            j += 1;
            continue;
        }
        let (cx, lx) = decode(a, i);
        let (cy, ly) = decode(b, j);
        let (fx, yx) = fold(cx);
        let (fy, yy) = fold(cy);
        let ord = fx.cmp(&fy);
        if ord != Ordering::Equal {
            return ord;
        }
        if secondary == Ordering::Equal {
            secondary = yx.cmp(&yy);
        }
        i += lx;
        j += ly;
    }
}

/// Цифровые группы с `i` и `j`: (порядок по значению, порядок по числу нулей,
/// конец группы в `a`, в `b`).
fn digits(a: &[u8], i: usize, b: &[u8], j: usize) -> (Ordering, Ordering, usize, usize) {
    let end = |s: &[u8], k: usize| k + s[k..].iter().take_while(|c| c.is_ascii_digit()).count();
    let (ea, eb) = (end(a, i), end(b, j));
    let zeros = |s: &[u8], k: usize, e: usize| s[k..e].iter().take_while(|&&c| c == b'0').count();
    let (za, zb) = (zeros(a, i, ea), zeros(b, j, eb));
    let (va, vb) = (&a[i + za..ea], &b[j + zb..eb]);
    // Значение: сначала длина без нулей, затем цифры — без переполнения.
    let ord = va.len().cmp(&vb.len()).then_with(|| va.cmp(vb));
    (ord, za.cmp(&zb), ea, eb)
}

/// Символ на байтовой позиции `i` и его длина.
fn decode(s: &str, i: usize) -> (char, usize) {
    let c = s[i..].chars().next().expect("index on char boundary");
    (c, c.len_utf8())
}

/// Свёртка: простой lowercase (если даёт ровно один не-ASCII символ), `ё` → `е`.
/// Второе значение — вторичный вес (`ё` после `е`).
fn fold(c: char) -> (char, u8) {
    let mut l = c.to_lowercase();
    let lower = match (l.next(), l.next()) {
        (Some(x), None) if !x.is_ascii() || c.is_ascii() => x,
        _ => c,
    };
    if lower == 'ё' { ('е', 1) } else { (lower, 0) }
}

/// 8 байт ключа (big-endian → сравнение как `u64`), монотонного относительно
/// [`cmp_with`]: `prefix_key(a) < prefix_key(b)` ⇒ `a < b`; равенство — «смотри
/// дальше». Сложенные ASCII-байты до первого спецслучая: цифра в natural-режиме
/// → `b'0'` и стоп, не-ASCII → `0x80` и стоп; остаток — нули.
pub fn prefix_key(name: &str, natural: bool) -> u64 {
    let mut k = [0u8; 8];
    for (slot, &b) in k.iter_mut().zip(name.as_bytes()) {
        if natural && b.is_ascii_digit() {
            *slot = b'0';
            break;
        }
        if b >= 0x80 {
            *slot = 0x80;
            break;
        }
        *slot = b.to_ascii_lowercase();
    }
    u64::from_be_bytes(k)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn sorted(mut v: Vec<&str>) -> Vec<&str> {
        v.sort_by(|a, b| name_cmp(a, b, true));
        v
    }

    #[test]
    fn known_cases() {
        assert_eq!(
            sorted(vec!["file10", "file2", "file1", "File3"]),
            ["file1", "file2", "File3", "file10"]
        );
        // Равные значения: меньше нулей — раньше.
        assert_eq!(sorted(vec!["a01", "a1", "a001"]), ["a1", "a01", "a001"]);
        assert_eq!(natural_cmp("a01", "a1"), Ordering::Greater);
        // ё сразу после е, раньше ж; регистр не влияет на уровень 1.
        assert_eq!(
            sorted(vec!["ж", "ё", "е", "Ё", "Е"]),
            ["Е", "е", "Ё", "ё", "ж"]
        );
        assert_eq!(natural_cmp("ёлка", "елка"), Ordering::Greater);
        assert_eq!(natural_cmp("ёлка", "елки"), Ordering::Less);
        // Короче — раньше; цифры раньше букв; пунктуация до цифр — раньше.
        assert_eq!(sorted(vec!["ab", "a", "abc"]), ["a", "ab", "abc"]);
        assert_eq!(sorted(vec!["b", "1", "-"]), ["-", "1", "b"]);
        // Длинные числа — без переполнения.
        assert_eq!(
            natural_cmp("x99999999999999999999999", "x100000000000000000000000"),
            Ordering::Less
        );
        // Эмодзи и прочее — по кодпоинтам после ASCII.
        assert_eq!(sorted(vec!["😀", "я", "z"]), ["z", "я", "😀"]);
        // Не-natural: цифры — символы.
        assert_eq!(name_cmp("file10", "file2", false), Ordering::Less);
        // Кельвин не сворачивается в ASCII k.
        assert_ne!(natural_cmp("\u{212A}", "k"), Ordering::Equal);
    }

    fn name() -> impl Strategy<Value = String> {
        "[a-cA-C0-2ёЁеЕж😀._ -]{0,8}"
    }

    proptest! {
        #[test]
        fn name_cmp_is_total_order(a in name(), b in name(), c in name(), nat in any::<bool>()) {
            let ab = name_cmp(&a, &b, nat);
            prop_assert_eq!(ab, name_cmp(&b, &a, nat).reverse());
            prop_assert_eq!(ab == Ordering::Equal, a == b);
            if ab != Ordering::Greater && name_cmp(&b, &c, nat) != Ordering::Greater {
                prop_assert_ne!(name_cmp(&a, &c, nat), Ordering::Greater, "{} {} {}", a, b, c);
            }
        }

        #[test]
        fn prefix_key_is_monotonic(a in name(), b in name(), nat in any::<bool>()) {
            let (ka, kb) = (prefix_key(&a, nat), prefix_key(&b, nat));
            if ka < kb {
                prop_assert_eq!(cmp_with(&a, &b, nat), Ordering::Less, "{} {}", a, b);
            }
        }

        #[test]
        fn prefix_key_monotonic_any_unicode(a in "\\PC{0,10}", b in "\\PC{0,10}") {
            if prefix_key(&a, true) < prefix_key(&b, true) {
                prop_assert_eq!(natural_cmp(&a, &b), Ordering::Less);
            }
        }
    }
}
