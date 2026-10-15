use std::fmt;

use compact_str::CompactString;

use super::CmdError;

/// Идентификатор команды.
///
/// Формат: `ns.verb[.qual]` — 2 или 3 сегмента через `.`, символы `[a-z0-9_]`,
/// сегмент начинается с буквы, всего ≤24 байт (хранится inline в `CompactString`).
#[derive(Clone, Eq, PartialEq, Hash)]
pub struct CommandId(CompactString);

impl CommandId {
    /// Для литералов. Невалидный id в `const`-контексте — ошибка компиляции,
    /// иначе паника.
    pub const fn from_static(s: &'static str) -> Self {
        match check(s) {
            Ok(()) => CommandId(CompactString::const_new(s)),
            Err(()) => panic!("invalid CommandId: expected ns.verb[.qual], [a-z0-9_], <=24 bytes"),
        }
    }

    pub fn new(s: &str) -> Result<Self, CmdError> {
        match check(s) {
            Ok(()) => Ok(CommandId(s.into())),
            Err(()) => Err(CmdError::BadId(s.into())),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Часть до первой точки: по ней выбирается handler.
    pub fn namespace(&self) -> &str {
        self.0.split_once('.').map_or(self.as_str(), |(ns, _)| ns)
    }
}

impl fmt::Debug for CommandId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CommandId({})", self.0)
    }
}

impl fmt::Display for CommandId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

const MAX_LEN: usize = 24;

const fn check(s: &str) -> Result<(), ()> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > MAX_LEN {
        return Err(());
    }
    let mut segments = 1;
    let mut seg_start = true;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'.' {
            if seg_start {
                return Err(());
            }
            segments += 1;
            seg_start = true;
        } else if seg_start {
            if !c.is_ascii_lowercase() {
                return Err(());
            }
            seg_start = false;
        } else if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_') {
            return Err(());
        }
        i += 1;
    }
    if seg_start || segments < 2 || segments > 3 {
        return Err(());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::CommandId;
    use crate::cmd::CmdError;

    const GO: CommandId = CommandId::from_static("panel.go");

    #[test]
    fn valid_ids() {
        for s in [
            "panel.go",
            "panel.cursor.page_up",
            "tab.goto",
            "ops.copy",
            "a.b",
            "panel.cursor.page_dow2",
            "panel.cursor.page_down_x",
        ] {
            assert_eq!(CommandId::new(s).unwrap().as_str(), s);
        }
        assert_eq!(GO.namespace(), "panel");
        assert_eq!(
            CommandId::from_static("panel.cursor.up").namespace(),
            "panel"
        );
    }

    #[test]
    fn invalid_ids_rejected() {
        for s in [
            "",
            "panel",
            ".go",
            "panel.",
            "panel..go",
            "Panel.go",
            "panel.Go",
            "panel.go-x",
            "panel.1go",
            "_p.go",
            "panel go",
            "a.b.c.d",
            "panel.cursor.page_down_xy",
            "панель.go",
        ] {
            assert_eq!(CommandId::new(s), Err(CmdError::BadId(s.into())), "{s:?}");
        }
    }

    #[test]
    #[should_panic(expected = "invalid CommandId")]
    fn from_static_panics_on_invalid() {
        let _ = CommandId::from_static("bad");
    }
}
