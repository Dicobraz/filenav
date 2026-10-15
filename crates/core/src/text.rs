//! Тексты core для UI. Вызовы идут по Fluent-ключам `core-*` (S2 §7); пока
//! `filenav-i18n` не подключён (S2-01/S2-02), возвращается английский фолбэк.
//! При подключении меняется только тело [`tr`].

/// Текст по ключу с подстановкой `{$name}`.
pub(crate) fn tr(key: &'static str, args: &[(&str, &str)]) -> String {
    let template = match key {
        "core-unknown-command" => "Unknown command: {$id}",
        "core-command-failed" => "{$id}: {$error}",
        "core-task-panicked" => "Task {$task} failed unexpectedly",
        "core-not-found" => "Not found: {$name}",
        "core-no-match" => "Nothing matches {$mask}",
        "core-no-bookmark" => "Bookmark {$n} is empty",
        "core-last-tab" => "Cannot close the last tab",
        "core-no-tab" => "No tab {$n}",
        "core-config-warnings" => "Settings file has problems ({$count}): {$first}",
        _ => {
            debug_assert!(false, "no fallback for {key}");
            key
        }
    };
    let mut s = template.to_owned();
    for (name, value) in args {
        s = s.replace(&format!("{{${name}}}"), value);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::tr;

    #[test]
    fn substitutes_args() {
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
