//! Тексты core для UI: Fluent-ключи `core-*` (S2 §7) из `filenav-i18n`, язык — активный.

/// Текст по ключу с подстановкой строковых аргументов `{ $name }`.
pub(crate) fn tr(key: &'static str, args: &[(&str, &str)]) -> String {
    let mut fluent = filenav_i18n::FluentArgs::new();
    for (name, value) in args {
        fluent.set(*name, *value);
    }
    filenav_i18n::current().fmt(key, &fluent)
}

#[cfg(test)]
mod tests {
    use super::tr;

    #[test]
    fn substitutes_args() {
        // Язык по умолчанию до `init` — en.
        assert_eq!(
            tr("core-unknown-command", &[("id", "x.y")]),
            "Unknown command: x.y"
        );
        assert_eq!(
            tr("core-command-failed", &[("id", "a.b"), ("error", "bad")]),
            "a.b: bad"
        );
    }
}
