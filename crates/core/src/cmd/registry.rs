use rustc_hash::FxHashMap;

use super::{CmdError, Command, CommandId, SideRef, Value, ValueKind};

/// Описание аргумента команды.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ArgSpec {
    pub name: &'static str,
    pub kind: ValueKind,
    pub required: bool,
}

/// Описание команды для палитры, keymap и валидации.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CommandMeta {
    pub id: &'static str,
    /// EN. Перевод — Fluent-ключ `cmd-<id>`, где точки id заменены дефисами
    /// (`cmd-ops-copy` для `ops.copy`); подпись keybar — `keybar-<id>` с фолбэком на
    /// `cmd-<id>` (S0 v3 №14). Маппинг id → ключ делает i18n.
    pub title: &'static str,
    pub category: &'static str,
    pub args: &'static [ArgSpec],
    /// Автоповтор при удержании клавиши (S3).
    pub repeatable: bool,
}

/// Реестр команд всех handlers. Порядок `iter` — порядок регистрации.
#[derive(Default, Debug)]
pub struct CommandRegistry {
    list: Vec<&'static CommandMeta>,
    by_id: FxHashMap<&'static str, usize>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Регистрирует описания (обычно `Handler::commands()`). Ошибка `BadId` на
    /// невалидном или повторном id; при ошибке реестр не меняется.
    pub fn register(&mut self, metas: &'static [CommandMeta]) -> Result<(), CmdError> {
        for (i, m) in metas.iter().enumerate() {
            CommandId::new(m.id)?;
            if self.by_id.contains_key(m.id) || metas[..i].iter().any(|p| p.id == m.id) {
                return Err(CmdError::BadId(m.id.into()));
            }
        }
        for m in metas {
            self.by_id.insert(m.id, self.list.len());
            self.list.push(m);
        }
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&'static CommandMeta> {
        self.by_id.get(id).map(|&i| self.list[i])
    }

    pub fn iter(&self) -> impl Iterator<Item = &'static CommandMeta> + '_ {
        self.list.iter().copied()
    }

    /// Команда известна, все аргументы описаны и нужного типа, обязательные есть.
    /// `ValueKind::Loc` принимает и `Str`; аргумент с именем `side` должен быть
    /// `left | right | active | passive` (без учёта регистра).
    pub fn validate(&self, c: &Command) -> Result<(), CmdError> {
        let meta = self
            .get(c.id.as_str())
            .ok_or_else(|| CmdError::Unknown(c.id.as_str().into()))?;
        for (k, v) in c.args.iter() {
            let spec = meta
                .args
                .iter()
                .find(|s| s.name == k)
                .ok_or_else(|| CmdError::arg(k, "unknown argument"))?;
            if !kind_accepts(spec.kind, v) {
                return Err(CmdError::arg(
                    k,
                    format!("expected {:?}, got {:?}", spec.kind, v.kind()),
                ));
            }
            if spec.name == "side" && !matches!(v, Value::Str(s) if SideRef::parse(s).is_some()) {
                return Err(CmdError::arg(k, "expected left | right | active | passive"));
            }
        }
        if let Some(missing) = meta
            .args
            .iter()
            .find(|s| s.required && c.args.get(s.name).is_none())
        {
            return Err(CmdError::arg(missing.name, "required"));
        }
        Ok(())
    }
}

fn kind_accepts(kind: ValueKind, v: &Value) -> bool {
    match (kind, v) {
        (ValueKind::Loc, Value::Str(_)) => true,
        (k, v) => k == v.kind(),
    }
}

#[cfg(test)]
mod tests {
    use super::{ArgSpec, CommandMeta, CommandRegistry};
    use crate::cmd::{CmdError, Command, Value, ValueKind};
    use crate::loc::Location;

    const fn arg(name: &'static str, kind: ValueKind, required: bool) -> ArgSpec {
        ArgSpec {
            name,
            kind,
            required,
        }
    }

    const fn meta(id: &'static str, args: &'static [ArgSpec]) -> CommandMeta {
        CommandMeta {
            id,
            title: id,
            category: "test",
            args,
            repeatable: false,
        }
    }

    static PANEL: [CommandMeta; 3] = [
        meta(
            "panel.go",
            &[
                arg("path", ValueKind::Loc, true),
                arg("side", ValueKind::Str, false),
            ],
        ),
        meta("panel.cursor.down", &[arg("n", ValueKind::Int, false)]),
        meta(
            "ops.copy",
            &[
                arg("src", ValueKind::List, true),
                arg("dest", ValueKind::Loc, true),
            ],
        ),
    ];

    fn reg() -> CommandRegistry {
        let mut r = CommandRegistry::new();
        r.register(&PANEL).unwrap();
        r
    }

    #[test]
    fn lookup_and_order() {
        let r = reg();
        assert_eq!(r.get("panel.go").map(|m| m.id), Some("panel.go"));
        assert!(r.get("panel.nope").is_none());
        let ids: Vec<_> = r.iter().map(|m| m.id).collect();
        assert_eq!(ids, ["panel.go", "panel.cursor.down", "ops.copy"]);
    }

    #[test]
    fn register_rejects_duplicates_and_bad_ids() {
        static DUP: [CommandMeta; 1] = [meta("panel.go", &[])];
        static SELF_DUP: [CommandMeta; 2] = [meta("tab.new", &[]), meta("tab.new", &[])];
        static BAD: [CommandMeta; 1] = [meta("Bad", &[])];
        let mut r = reg();
        assert_eq!(r.register(&DUP), Err(CmdError::BadId("panel.go".into())));
        assert_eq!(
            r.register(&SELF_DUP),
            Err(CmdError::BadId("tab.new".into()))
        );
        assert_eq!(r.register(&BAD), Err(CmdError::BadId("Bad".into())));
        assert_eq!(r.iter().count(), 3);
    }

    #[test]
    fn validate_ok() {
        let r = reg();
        let p = if cfg!(windows) { r"C:\x" } else { "/x" };
        for c in [
            Command::new("panel.go").arg("path", p),
            Command::new("panel.go")
                .arg("path", Location::local(p))
                .arg("side", "Passive"),
            Command::new("panel.cursor.down"),
            Command::new("panel.cursor.down").arg("n", 3),
            Command::parse(r#"ops.copy src=["C:\a","C:\b y"] dest="D:\""#).unwrap(),
        ] {
            assert_eq!(r.validate(&c), Ok(()), "{c}");
        }
        for side in ["left", "RIGHT", "active", "passive"] {
            let c = Command::new("panel.go").arg("path", p).arg("side", side);
            assert_eq!(r.validate(&c), Ok(()), "{side}");
        }
    }

    #[test]
    fn validate_errors() {
        let r = reg();
        let arg_err = |c: Command, name: &str| match r.validate(&c) {
            Err(CmdError::Arg { name: n, .. }) => assert_eq!(n, name, "{c}"),
            other => panic!("{c}: {other:?}"),
        };
        arg_err(Command::new("panel.go"), "path");
        arg_err(Command::new("panel.go").arg("path", 1), "path");
        arg_err(
            Command::new("panel.go")
                .arg("path", "x")
                .arg("side", "middle"),
            "side",
        );
        arg_err(
            Command::new("panel.go").arg("path", "x").arg("side", true),
            "side",
        );
        arg_err(
            Command::new("panel.go").arg("path", "x").arg("extra", 1),
            "extra",
        );
        arg_err(Command::new("panel.cursor.down").arg("n", "3"), "n");
        arg_err(
            Command::new("ops.copy").arg("src", "a").arg("dest", "b"),
            "src",
        );
        arg_err(
            Command::new("ops.copy").arg("src", Vec::<Value>::new()),
            "dest",
        );
        assert_eq!(
            r.validate(&Command::new("tab.new")),
            Err(CmdError::Unknown("tab.new".into()))
        );
    }
}
