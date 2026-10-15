//! serde: `CommandId` и `Value` — скаляры, `Command` — `"id"` или `{ cmd = "id", key = value… }`.

use std::fmt;

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{Args, Command, CommandId, Value, valid_key};

impl Serialize for CommandId {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for CommandId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = <std::borrow::Cow<'de, str>>::deserialize(d)?;
        CommandId::new(&s).map_err(de::Error::custom)
    }
}

/// `Loc` пишется uri-строкой и читается обратно как `Str`.
impl Serialize for Value {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::Bool(b) => s.serialize_bool(*b),
            Value::Int(n) => s.serialize_i64(*n),
            Value::Str(v) => s.serialize_str(v),
            Value::Loc(l) => s.serialize_str(&l.to_uri()),
            Value::List(items) => {
                let mut seq = s.serialize_seq(Some(items.len()))?;
                for it in items.iter() {
                    seq.serialize_element(it)?;
                }
                seq.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(ValueVisitor)
    }
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("bool, integer, string or array")
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Int(v))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Value, E> {
        i64::try_from(v)
            .map(Value::Int)
            .map_err(|_| E::custom("integer out of range"))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Value, E> {
        Ok(Value::from(v))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut items = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(v) = seq.next_element()? {
            items.push(v);
        }
        Ok(items.into())
    }
}

impl Serialize for Command {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if self.args.is_empty() {
            return s.serialize_str(self.id.as_str());
        }
        let mut map = s.serialize_map(Some(self.args.len() + 1))?;
        map.serialize_entry("cmd", &self.id)?;
        for (k, v) in self.args.iter() {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Command {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(CommandVisitor)
    }
}

struct CommandVisitor;

impl<'de> Visitor<'de> for CommandVisitor {
    type Value = Command;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(r#"command id string or table { cmd = "id", … }"#)
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Command, E> {
        CommandId::new(v).map(Command::with_id).map_err(E::custom)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Command, A::Error> {
        let mut id = None;
        let mut args = Args::default();
        while let Some(k) = map.next_key::<std::borrow::Cow<'de, str>>()? {
            if k == "cmd" {
                if id.is_some() {
                    return Err(de::Error::duplicate_field("cmd"));
                }
                id = Some(map.next_value::<CommandId>()?);
            } else {
                if !valid_key(&k) {
                    return Err(de::Error::custom(format_args!(
                        "invalid argument name '{k}': expected [a-z0-9_]+"
                    )));
                }
                if args.get(&k).is_some() {
                    return Err(de::Error::custom(format_args!("duplicate argument '{k}'")));
                }
                args = args.with(&k, map.next_value::<Value>()?);
            }
        }
        let id = id.ok_or_else(|| de::Error::missing_field("cmd"))?;
        let mut c = Command::with_id(id);
        c.args = args;
        Ok(c)
    }
}

#[cfg(test)]
mod tests {
    use crate::cmd::{Command, Value};
    use crate::loc::Location;

    #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
    struct Keymap {
        bind: Vec<Command>,
    }

    fn sample() -> Keymap {
        Keymap {
            bind: vec![
                Command::new("panel.up"),
                Command::new("panel.go").arg("path", r"C:\x y"),
                Command::new("panel.sort")
                    .arg("key", "name")
                    .arg("desc", true),
                Command::new("panel.cursor.down").arg("n", 5),
                Command::new("ops.copy")
                    .arg("src", vec![Value::from(r"C:\a"), Value::from(r"C:\b y")])
                    .arg("dest", r"D:\"),
                Command::new("panel.switch").arg("side", "passive"),
            ],
        }
    }

    #[test]
    fn toml_snapshot_and_roundtrip() {
        let k = sample();
        let text = toml::to_string_pretty(&k).unwrap();
        insta::assert_snapshot!(text);
        assert_eq!(toml::from_str::<Keymap>(&text).unwrap(), k);
    }

    #[test]
    fn toml_hand_written_forms() {
        let text = r#"
            bind = [
                "panel.up",
                { cmd = "panel.go", path = 'C:\x y' },
                { cmd = "ops.copy", src = ['C:\a', 'C:\b y'], dest = 'D:\' },
                { cmd = "tab.goto", n = 3 },
            ]
        "#;
        let k: Keymap = toml::from_str(text).unwrap();
        assert_eq!(k.bind[0], Command::new("panel.up"));
        assert_eq!(k.bind[1], Command::new("panel.go").arg("path", r"C:\x y"));
        assert_eq!(
            k.bind[2],
            Command::new("ops.copy")
                .arg("src", vec![Value::from(r"C:\a"), Value::from(r"C:\b y")])
                .arg("dest", r"D:\")
        );
        assert_eq!(k.bind[2].args.list("src").map(<[Value]>::len), Some(2));
        assert_eq!(k.bind[3].args.int("n"), Some(3));
        insta::assert_snapshot!(
            "hand_written_reserialized",
            toml::to_string_pretty(&k).unwrap()
        );
    }

    #[test]
    fn loc_serializes_as_uri_string() {
        let p = if cfg!(windows) { r"C:\x" } else { "/x" };
        let k = Keymap {
            bind: vec![Command::new("panel.go").arg("path", Location::local(p))],
        };
        let back: Keymap = toml::from_str(&toml::to_string(&k).unwrap()).unwrap();
        let want = format!("local:{p}");
        assert_eq!(back.bind[0].args.str("path"), Some(want.as_str()));
        assert_eq!(back.bind[0].args.loc("path"), Some(Location::local(p)));
    }

    #[test]
    fn toml_errors() {
        for text in [
            r#"bind = ["Panel.up"]"#,
            r#"bind = [{ path = "x" }]"#,
            r#"bind = [{ cmd = "panel.go", Path = "x" }]"#,
            r#"bind = [{ cmd = "panel.go", n = 1.5 }]"#,
            r#"bind = [{ cmd = "panel.go", t = { a = 1 } }]"#,
            r"bind = [1]",
        ] {
            assert!(toml::from_str::<Keymap>(text).is_err(), "{text}");
        }
    }
}
