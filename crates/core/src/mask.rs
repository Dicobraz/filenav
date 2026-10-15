//! Маска имён в стиле Far: `*.rs,*.toml|*.bak`.
//!
//! Синтаксис:
//! - `include|exclude`: имя подходит, если совпало с любым шаблоном include
//!   (пустой include — «все») и ни с одним из exclude. Не больше одного `|`.
//! - Шаблоны в части разделяются `,` или `;`, пробелы по краям отбрасываются,
//!   пустые шаблоны пропускаются.
//! - `*` — любая последовательность символов (в том числе пустая), `?` — ровно один символ.
//! - `"…"` — кавычки защищают `,`, `;`, `|` и пробелы по краям; `*` и `?` внутри работают.
//! - Шаблон, оканчивающийся на `.*`, подходит и без расширения: `abc.*` ловит `abc`,
//!   `*.*` ловит всё (как в Far и DOS).
//!
//! Регистр: без учёта на Windows и macOS, с учётом на остальных ОС. Сравнение
//! посимвольное (`char::to_lowercase`, если даёт один символ), без полного case folding.

use std::fmt;

/// Маска Far-стиля. Регистронезависима на Windows/macOS.
#[derive(Clone, PartialEq, Eq)]
pub struct Mask {
    src: String,
    include: Vec<Pat>,
    exclude: Vec<Pat>,
    ci: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Pat {
    toks: Vec<Tok>,
    /// Шаблон оканчивается на `.*`: длина префикса без этих двух токенов.
    dot_star: Option<usize>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Tok {
    Star,
    Any,
    Lit(char),
}

/// Ошибка разбора маски.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MaskError {
    #[error("empty mask")]
    Empty,
    #[error("more than one '|' in mask")]
    ExtraPipe,
    #[error("unclosed quote in mask")]
    UnclosedQuote,
}

/// Регистр по ОС.
const OS_CASE_INSENSITIVE: bool = cfg!(any(windows, target_os = "macos"));

impl Mask {
    pub fn parse(s: &str) -> Result<Self, MaskError> {
        Self::parse_case(s, OS_CASE_INSENSITIVE)
    }

    /// Разбор с явным режимом регистра (для тестов обоих режимов на любой ОС).
    pub(crate) fn parse_case(s: &str, case_insensitive: bool) -> Result<Self, MaskError> {
        // Символы текущего шаблона с признаком «внутри кавычек».
        fn flush(cur: &mut Vec<(char, bool)>, out: &mut Vec<String>) {
            let unquoted_ws = |&(c, q): &(char, bool)| !q && c.is_whitespace();
            let start = cur.iter().position(|x| !unquoted_ws(x));
            let end = cur.iter().rposition(|x| !unquoted_ws(x));
            if let (Some(a), Some(b)) = (start, end) {
                out.push(cur[a..=b].iter().map(|&(c, _)| c).collect());
            }
            cur.clear();
        }

        let mut parts: [Vec<String>; 2] = [Vec::new(), Vec::new()];
        let mut part = 0;
        let mut cur: Vec<(char, bool)> = Vec::new();
        let mut in_quotes = false;
        for c in s.chars() {
            match c {
                '"' => in_quotes = !in_quotes,
                _ if in_quotes => cur.push((c, true)),
                ',' | ';' => flush(&mut cur, &mut parts[part]),
                '|' => {
                    if part == 1 {
                        return Err(MaskError::ExtraPipe);
                    }
                    flush(&mut cur, &mut parts[part]);
                    part = 1;
                }
                _ => cur.push((c, false)),
            }
        }
        if in_quotes {
            return Err(MaskError::UnclosedQuote);
        }
        flush(&mut cur, &mut parts[part]);
        let [inc, exc] = parts;
        if inc.is_empty() && exc.is_empty() {
            return Err(MaskError::Empty);
        }
        let compile = |v: Vec<String>| v.iter().map(|p| Pat::new(p, case_insensitive)).collect();
        Ok(Mask {
            src: s.to_owned(),
            include: compile(inc),
            exclude: compile(exc),
            ci: case_insensitive,
        })
    }

    pub fn matches(&self, name: &str) -> bool {
        (self.include.is_empty() || self.include.iter().any(|p| p.matches(name, self.ci)))
            && !self.exclude.iter().any(|p| p.matches(name, self.ci))
    }

    /// Исходная строка маски.
    pub fn as_str(&self) -> &str {
        &self.src
    }
}

impl fmt::Debug for Mask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Mask").field(&self.src).finish()
    }
}

impl Pat {
    fn new(p: &str, ci: bool) -> Self {
        let toks: Vec<Tok> = p
            .chars()
            .map(|c| match c {
                '*' => Tok::Star,
                '?' => Tok::Any,
                c => Tok::Lit(if ci { fold(c) } else { c }),
            })
            .collect();
        let n = toks.len();
        let dot_star = if toks.ends_with(&[Tok::Lit('.'), Tok::Star]) {
            Some(n - 2)
        } else {
            None
        };
        Pat { toks, dot_star }
    }

    fn matches(&self, name: &str, ci: bool) -> bool {
        glob(&self.toks, name, ci)
            || self
                .dot_star
                .is_some_and(|n| glob(&self.toks[..n], name, ci))
    }
}

fn fold(c: char) -> char {
    if c.is_ascii() {
        return c.to_ascii_lowercase();
    }
    let mut l = c.to_lowercase();
    match (l.next(), l.next()) {
        (Some(x), None) => x,
        _ => c,
    }
}

/// Сопоставление с одним откатом к последней `*`: O(len(p) · len(name)) в худшем
/// случае, без аллокаций.
fn glob(p: &[Tok], name: &str, ci: bool) -> bool {
    let (mut pi, mut ni) = (0usize, 0usize);
    // (позиция в шаблоне после `*`, позиция в имени, с которой `*` сейчас начинает поглощать)
    let mut star: Option<(usize, usize)> = None;
    loop {
        if pi < p.len() {
            if p[pi] == Tok::Star {
                star = Some((pi + 1, ni));
                pi += 1;
                continue;
            }
            if let Some(c) = name[ni..].chars().next() {
                let ok = match p[pi] {
                    Tok::Any => true,
                    Tok::Lit(l) => l == if ci { fold(c) } else { c },
                    Tok::Star => unreachable!(),
                };
                if ok {
                    pi += 1;
                    ni += c.len_utf8();
                    continue;
                }
            }
        } else if ni == name.len() {
            return true;
        }
        // Несовпадение: `*` поглощает ещё один символ.
        let Some((sp, sn)) = star else { return false };
        let Some(c) = name[sn..].chars().next() else {
            return false;
        };
        let sn = sn + c.len_utf8();
        star = Some((sp, sn));
        pi = sp;
        ni = sn;
    }
}

#[cfg(test)]
mod tests {
    use super::{Mask, MaskError};
    use proptest::prelude::*;

    fn m(s: &str, ci: bool) -> Mask {
        Mask::parse_case(s, ci).unwrap()
    }

    #[test]
    fn include_exclude() {
        let mask = m("*.rs,*.toml|*.bak", false);
        assert!(mask.matches("main.rs"));
        assert!(mask.matches("Cargo.toml"));
        assert!(!mask.matches("main.rs.bak"));
        assert!(!mask.matches("x.bak"));
        assert!(!mask.matches("readme.md"));
        assert_eq!(mask.as_str(), "*.rs,*.toml|*.bak");
    }

    #[test]
    fn exclude_only_means_everything_else() {
        let mask = m("|*.bak;*.tmp", false);
        assert!(mask.matches("a.rs"));
        assert!(!mask.matches("a.bak"));
        assert!(!mask.matches("a.tmp"));
    }

    #[test]
    fn question_mark_is_one_char() {
        let mask = m("a?c", false);
        assert!(mask.matches("abc"));
        assert!(mask.matches("aжc"));
        assert!(!mask.matches("ac"));
        assert!(!mask.matches("abbc"));
        assert!(m("????", false).matches("файл"));
    }

    #[test]
    fn case_modes() {
        assert!(m("*.RS", true).matches("main.rs"));
        assert!(m("ФАЙЛ.*", true).matches("файл.txt"));
        assert!(!m("*.RS", false).matches("main.rs"));
        assert!(m("*.RS", false).matches("main.RS"));
    }

    #[test]
    fn case_follows_os() {
        let ci = cfg!(any(windows, target_os = "macos"));
        assert_eq!(Mask::parse("*.RS").unwrap().matches("main.rs"), ci);
    }

    #[test]
    fn dot_star_matches_names_without_extension() {
        assert!(m("*.*", false).matches("Makefile"));
        assert!(m("*.*", false).matches("a.b"));
        assert!(m("abc.*", false).matches("abc"));
        assert!(m("abc.*", false).matches("abc.d"));
        assert!(!m("abc.*", false).matches("abcd"));
    }

    #[test]
    fn quotes_and_spaces() {
        let mask = m(r#" "a,b.txt" , *.md "#, false);
        assert!(mask.matches("a,b.txt"));
        assert!(mask.matches("x.md"));
        assert!(!mask.matches("a"));
        assert!(m(r#"" x ""#, false).matches(" x "));
        assert!(m(r#""a|b*""#, false).matches("a|bc"));
    }

    #[test]
    fn errors() {
        assert_eq!(Mask::parse(""), Err(MaskError::Empty));
        assert_eq!(Mask::parse(" , ;|"), Err(MaskError::Empty));
        assert_eq!(Mask::parse("a|b|c"), Err(MaskError::ExtraPipe));
        assert_eq!(Mask::parse("\"abc"), Err(MaskError::UnclosedQuote));
    }

    #[test]
    fn pathological_stars_are_fast() {
        let mask = m(&"*a".repeat(30), false);
        assert!(!mask.matches(&"a".repeat(29)));
        assert!(mask.matches(&"a".repeat(60)));
    }

    /// Эталон: рекурсивное сопоставление и разбор без кавычек.
    fn naive_glob(p: &[char], n: &[char], ci: bool) -> bool {
        let eq = |a: char, b: char| {
            if ci {
                super::fold(a) == super::fold(b)
            } else {
                a == b
            }
        };
        match p.split_first() {
            None => n.is_empty(),
            Some(('*', rest)) => (0..=n.len()).any(|i| naive_glob(rest, &n[i..], ci)),
            Some(('?', rest)) => !n.is_empty() && naive_glob(rest, &n[1..], ci),
            Some((&c, rest)) => !n.is_empty() && eq(c, n[0]) && naive_glob(rest, &n[1..], ci),
        }
    }

    fn naive_pat(p: &str, n: &str, ci: bool) -> bool {
        let pc: Vec<char> = p.chars().collect();
        let nc: Vec<char> = n.chars().collect();
        naive_glob(&pc, &nc, ci) || (p.ends_with(".*") && naive_glob(&pc[..pc.len() - 2], &nc, ci))
    }

    /// `None` — маска невалидна.
    fn naive(mask: &str, name: &str, ci: bool) -> Option<bool> {
        let halves: Vec<&str> = mask.split('|').collect();
        if halves.len() > 2 {
            return None;
        }
        let pats = |h: &str| -> Vec<String> {
            h.split([',', ';'])
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_owned)
                .collect()
        };
        let inc = pats(halves[0]);
        let exc = halves.get(1).map(|h| pats(h)).unwrap_or_default();
        if inc.is_empty() && exc.is_empty() {
            return None;
        }
        let any = |v: &[String]| v.iter().any(|p| naive_pat(p, name, ci));
        Some((inc.is_empty() || any(&inc)) && !any(&exc))
    }

    proptest! {
        #[test]
        fn matches_naive(
            mask in "[aBé.*? ,;|]{0,12}",
            name in "[abAEé.]{0,10}",
            ci in any::<bool>(),
        ) {
            let got = Mask::parse_case(&mask, ci).ok().map(|m| m.matches(&name));
            prop_assert_eq!(got, naive(&mask, &name, ci));
        }
    }
}
