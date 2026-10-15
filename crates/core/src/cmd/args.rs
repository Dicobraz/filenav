use compact_str::CompactString;
use smallvec::SmallVec;

use super::Value;
use crate::loc::Location;

/// Аргументы команды: пары ключ → значение в порядке добавления, ключи уникальны.
/// Равенство не зависит от порядка (TOML-таблицы приходят отсортированными).
#[derive(Clone, Debug, Default)]
pub struct Args(SmallVec<[(CompactString, Value); 2]>);

impl PartialEq for Args {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().all(|(k, v)| other.get(k) == Some(v))
    }
}

/// Ссылка на панель в аргументе `side` (S0 v3 №5, v5 №6). Перевод `Active`/`Passive`
/// в [`Side`](crate::Side) делает вызывающий код относительно `AppState.active`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum SideRef {
    Left,
    Right,
    Active,
    Passive,
}

impl SideRef {
    /// `left | right | active | passive` без учёта регистра.
    pub(crate) fn parse(s: &str) -> Option<Self> {
        [
            ("left", SideRef::Left),
            ("right", SideRef::Right),
            ("active", SideRef::Active),
            ("passive", SideRef::Passive),
        ]
        .into_iter()
        .find_map(|(name, v)| s.eq_ignore_ascii_case(name).then_some(v))
    }
}

impl Args {
    /// Добавить или заменить значение ключа.
    #[must_use]
    pub fn with(mut self, k: &str, v: impl Into<Value>) -> Self {
        let v = v.into();
        match self.0.iter_mut().find(|(key, _)| key == k) {
            Some(slot) => slot.1 = v,
            None => self.0.push((k.into(), v)),
        }
        self
    }

    pub fn get(&self, k: &str) -> Option<&Value> {
        self.0.iter().find(|(key, _)| key == k).map(|(_, v)| v)
    }

    pub fn str(&self, k: &str) -> Option<&str> {
        match self.get(k)? {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn int(&self, k: &str) -> Option<i64> {
        match self.get(k)? {
            Value::Int(n) => Some(*n),
            _ => None,
        }
    }

    pub fn bool(&self, k: &str) -> Option<bool> {
        match self.get(k)? {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// `Loc` как есть; `Str` — через [`Location::parse_uri`], а если это не uri — как
    /// локальный путь ([`Location::local`]).
    pub fn loc(&self, k: &str) -> Option<Location> {
        value_loc(self.get(k)?)
    }

    pub fn list(&self, k: &str) -> Option<&[Value]> {
        match self.get(k)? {
            Value::List(l) => Some(l),
            _ => None,
        }
    }

    /// Список `Location` (S0 v2 №11): каждый элемент `List` — `Loc` или `Str`,
    /// разобранный как в [`Args::loc`]. `None`, если значение не список или в нём
    /// есть элемент другого типа.
    pub fn locs(&self, k: &str) -> Option<Vec<Location>> {
        self.list(k)?.iter().map(value_loc).collect()
    }

    /// Аргумент `side`: `left | right | active | passive` без учёта регистра.
    /// `None` при отсутствии ключа, не-строке или неизвестном значении.
    pub fn side(&self, k: &str) -> Option<SideRef> {
        SideRef::parse(self.str(k)?)
    }

    /// Пары в порядке добавления.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, &Value)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

fn value_loc(v: &Value) -> Option<Location> {
    match v {
        Value::Loc(l) => Some(l.clone()),
        Value::Str(s) => {
            Some(Location::parse_uri(s).unwrap_or_else(|_| Location::local(s.as_str())))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{Args, SideRef};
    use crate::cmd::Value;
    use crate::loc::Location;

    #[test]
    fn with_replaces_existing_key() {
        let a = Args::default().with("n", 1).with("s", "x").with("n", 2);
        assert_eq!(a.len(), 2);
        assert_eq!(a.int("n"), Some(2));
        assert_eq!(a.str("s"), Some("x"));
        assert_eq!(a.int("s"), None);
        assert_eq!(a.get("missing"), None);
        let keys: Vec<_> = a.iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["n", "s"]);
        assert_eq!(a, Args::default().with("s", "x").with("n", 2));
        assert_ne!(a, Args::default().with("s", "x").with("n", 3));
        assert_ne!(a, Args::default().with("s", "x"));
    }

    #[test]
    fn loc_from_str_uri_or_path() {
        let p = if cfg!(windows) { r"C:\x" } else { "/x" };
        let a = Args::default()
            .with("uri", format!("local:{p}"))
            .with("path", p)
            .with("loc", Location::local(p))
            .with("n", 1);
        for k in ["uri", "path", "loc"] {
            assert_eq!(a.loc(k), Some(Location::local(p)), "{k}");
        }
        assert_eq!(a.loc("n"), None);
    }

    #[test]
    fn locs_list() {
        let (p1, p2) = if cfg!(windows) {
            (r"C:\a", r"C:\b y")
        } else {
            ("/a", "/b y")
        };
        let a = Args::default()
            .with(
                "src",
                vec![Value::from(p1), Value::Loc(Location::local(p2))],
            )
            .with("bad", vec![Value::from(p1), Value::Int(1)])
            .with("one", p1);
        assert_eq!(
            a.locs("src"),
            Some(vec![Location::local(p1), Location::local(p2)])
        );
        assert_eq!(a.list("src").map(<[Value]>::len), Some(2));
        assert_eq!(a.locs("bad"), None);
        assert_eq!(a.locs("one"), None);
    }

    #[test]
    fn side_four_values_case_insensitive() {
        let cases = [
            ("left", Some(SideRef::Left)),
            ("RIGHT", Some(SideRef::Right)),
            ("Active", Some(SideRef::Active)),
            ("passivE", Some(SideRef::Passive)),
            ("middle", None),
            ("", None),
        ];
        for (s, want) in cases {
            assert_eq!(Args::default().with("side", s).side("side"), want, "{s:?}");
        }
        assert_eq!(Args::default().with("side", 1).side("side"), None);
        assert_eq!(Args::default().side("side"), None);
    }
}
