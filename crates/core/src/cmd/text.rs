//! Текстовая форма команды: разбор ([`Command::parse`]) и `Display`.

use std::fmt;

use super::{Args, CmdError, Command, CommandId, Value, valid_key};

pub(super) fn parse(s: &str) -> Result<Command, CmdError> {
    let mut p = Parser { s, pos: 0 };
    p.skip_ws();
    let id = p.take_while(|c| !c.is_whitespace());
    let mut cmd = Command::with_id(CommandId::new(id)?);
    let mut args = Args::default();
    loop {
        p.skip_ws();
        if p.eof() {
            break;
        }
        let key = p.take_while(|c| c != '=' && !c.is_whitespace());
        if !valid_key(key) {
            return Err(CmdError::arg(key, "expected key=value with key [a-z0-9_]+"));
        }
        if !p.eat('=') {
            return Err(CmdError::arg(key, "expected '='"));
        }
        let v = p.value(key, false)?;
        if args.get(key).is_some() {
            return Err(CmdError::arg(key, "duplicate argument"));
        }
        if !p.eof() && !p.peek().is_some_and(char::is_whitespace) {
            return Err(CmdError::arg(key, "expected space after value"));
        }
        args = args.with(key, v);
    }
    cmd.args = args;
    Ok(cmd)
}

struct Parser<'a> {
    s: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn eof(&self) -> bool {
        self.pos >= self.s.len()
    }

    fn peek(&self) -> Option<char> {
        self.s[self.pos..].chars().next()
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += c.len_utf8();
            true
        } else {
            false
        }
    }

    fn take_while(&mut self, f: impl Fn(char) -> bool) -> &'a str {
        let start = self.pos;
        while let Some(c) = self.peek().filter(|&c| f(c)) {
            self.pos += c.len_utf8();
        }
        &self.s[start..self.pos]
    }

    fn skip_ws(&mut self) {
        self.take_while(char::is_whitespace);
    }

    fn value(&mut self, key: &str, in_list: bool) -> Result<Value, CmdError> {
        match self.peek() {
            Some('"') => self.quoted(key).map(Value::from),
            Some('[') => self.list(key),
            _ => {
                let tok = self.take_while(|c| {
                    !c.is_whitespace() && c != '"' && !(in_list && (c == ',' || c == ']'))
                });
                bare(key, tok)
            }
        }
    }

    fn quoted(&mut self, key: &str) -> Result<String, CmdError> {
        self.eat('"');
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return Err(CmdError::arg(key, "unterminated string")),
                Some('"') => {
                    self.pos += 1;
                    if !self.eat('"') {
                        return Ok(out);
                    }
                    out.push('"');
                }
                Some(c) => {
                    out.push(c);
                    self.pos += c.len_utf8();
                }
            }
        }
    }

    fn list(&mut self, key: &str) -> Result<Value, CmdError> {
        self.eat('[');
        let mut items = Vec::new();
        self.skip_ws();
        if self.eat(']') {
            return Ok(items.into());
        }
        loop {
            self.skip_ws();
            items.push(self.value(key, true)?);
            self.skip_ws();
            if self.eat(']') {
                return Ok(items.into());
            }
            if !self.eat(',') {
                return Err(CmdError::arg(key, "expected ',' or ']' in list"));
            }
        }
    }
}

fn is_int(tok: &str) -> bool {
    let digits = tok.strip_prefix('-').unwrap_or(tok);
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

fn bare(key: &str, tok: &str) -> Result<Value, CmdError> {
    match tok {
        "" => Err(CmdError::arg(key, "missing value")),
        "true" => Ok(Value::Bool(true)),
        "false" => Ok(Value::Bool(false)),
        t if is_int(t) => t
            .parse()
            .map(Value::Int)
            .map_err(|_| CmdError::arg(key, "integer out of range")),
        t => Ok(Value::from(t)),
    }
}

/// Строка без кавычек читается обратно той же строкой.
fn bare_safe(s: &str) -> bool {
    !s.is_empty()
        && s != "true"
        && s != "false"
        && !is_int(s)
        && !s.starts_with('[')
        && !s
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '"' | ',' | ']'))
}

fn write_str(f: &mut fmt::Formatter<'_>, s: &str) -> fmt::Result {
    if bare_safe(s) {
        f.write_str(s)
    } else {
        write!(f, "\"{}\"", s.replace('"', "\"\""))
    }
}

fn write_value(f: &mut fmt::Formatter<'_>, v: &Value) -> fmt::Result {
    match v {
        Value::Bool(b) => write!(f, "{b}"),
        Value::Int(n) => write!(f, "{n}"),
        Value::Str(s) => write_str(f, s),
        Value::Loc(l) => write_str(f, &l.to_uri()),
        Value::List(items) => {
            f.write_str("[")?;
            for (i, it) in items.iter().enumerate() {
                if i > 0 {
                    f.write_str(",")?;
                }
                write_value(f, it)?;
            }
            f.write_str("]")
        }
    }
}

/// Текстовая форма, которую читает [`Command::parse`]. `Loc` пишется uri-строкой.
impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id.as_str())?;
        for (k, v) in self.args.iter() {
            write!(f, " {k}=")?;
            write_value(f, v)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::cmd::{CmdError, Command, Value};
    use crate::loc::Location;
    use proptest::prelude::*;

    #[test]
    fn panel_go_quoted_path() {
        let c = Command::parse("panel.go path=\"C:\\x y\"").unwrap();
        assert_eq!(c.id.as_str(), "panel.go");
        assert_eq!(c.args.str("path"), Some(r"C:\x y"));
        if cfg!(windows) {
            assert_eq!(c.args.loc("path"), Some(Location::local(r"C:\x y")));
        }
    }

    #[test]
    fn ops_copy_list_and_trailing_backslash() {
        let c = Command::parse("ops.copy src=[\"C:\\a\",\"C:\\b y\"] dest=\"D:\\\"").unwrap();
        assert_eq!(
            c.args.list("src"),
            Some(&[Value::from(r"C:\a"), Value::from(r"C:\b y")][..])
        );
        assert_eq!(c.args.str("dest"), Some(r"D:\"));
        assert_eq!(Command::parse(&c.to_string()), Ok(c));
    }

    #[test]
    fn value_kinds() {
        let c = Command::parse(
            "  panel.sort key=name desc=true n=-12 s=\"a\"\"b\" e=[] l=[1, x ,[true]]  ",
        )
        .unwrap();
        assert_eq!(c.args.str("key"), Some("name"));
        assert_eq!(c.args.bool("desc"), Some(true));
        assert_eq!(c.args.int("n"), Some(-12));
        assert_eq!(c.args.str("s"), Some("a\"b"));
        assert_eq!(c.args.list("e"), Some(&[][..]));
        assert_eq!(
            c.args.list("l"),
            Some(
                &[
                    Value::Int(1),
                    Value::from("x"),
                    Value::from(vec![Value::Bool(true)])
                ][..]
            )
        );
        let keys: Vec<_> = c.args.iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["key", "desc", "n", "s", "e", "l"]);
    }

    #[test]
    fn errors() {
        let bad_arg = |s: &str| matches!(Command::parse(s), Err(CmdError::Arg { .. }));
        assert_eq!(Command::parse(""), Err(CmdError::BadId("".into())));
        assert_eq!(
            Command::parse("Panel.go"),
            Err(CmdError::BadId("Panel.go".into()))
        );
        assert!(bad_arg("panel.go path"));
        assert!(bad_arg("panel.go path="));
        assert!(bad_arg("panel.go Path=x"));
        assert!(bad_arg("panel.go cmd=x"));
        assert!(bad_arg("panel.go p=\"x"));
        assert!(bad_arg("panel.go p=\"x\"y"));
        assert!(bad_arg("panel.go p=[1 2]"));
        assert!(bad_arg("panel.go p=[1,"));
        assert!(bad_arg("panel.go p=1 p=2"));
        assert!(bad_arg("panel.go n=99999999999999999999"));
    }

    #[test]
    fn display_quotes_only_when_needed() {
        let c = Command::new("panel.go")
            .arg("a", "plain")
            .arg("b", "with space")
            .arg("c", "12")
            .arg("d", "true")
            .arg("e", "q\"q")
            .arg("f", 7)
            .arg("g", vec![Value::from("x,y"), Value::Bool(false)]);
        assert_eq!(
            c.to_string(),
            r#"panel.go a=plain b="with space" c="12" d="true" e="q""q" f=7 g=["x,y",false]"#
        );
        assert_eq!(Command::parse(&c.to_string()), Ok(c));
    }

    fn value() -> impl Strategy<Value = Value> {
        let leaf = prop_oneof![
            any::<bool>().prop_map(Value::Bool),
            any::<i64>().prop_map(Value::Int),
            "[a-zA-Z0-9 \"\\\\,\\[\\]:.=_-]{0,8}".prop_map(Value::from),
            "\\PC{0,6}".prop_map(Value::from),
        ];
        leaf.prop_recursive(2, 8, 4, |inner| {
            prop::collection::vec(inner, 0..4).prop_map(Value::from)
        })
    }

    proptest! {
        #[test]
        fn text_roundtrip(args in prop::collection::btree_map("[a-z_][a-z0-9_]{0,5}", value(), 0..5)) {
            let mut c = Command::new("panel.go");
            for (k, v) in args {
                if k != "cmd" {
                    c = c.arg(&k, v);
                }
            }
            let text = c.to_string();
            prop_assert_eq!(Command::parse(&text), Ok(c), "text = {}", text);
        }
    }
}
