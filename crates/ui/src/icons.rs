//! Глифы иконок (S2 §1.6): кодовые точки Segoe Fluent Icons; в Segoe MDL2 Assets
//! (Win10) те же точки для всего набора — проверено тестом `has_glyph`.

use urfin_vfs::icons::IconClass;

/// Глиф класса элемента.
pub fn glyph(class: IconClass) -> char {
    match class {
        IconClass::Dir | IconClass::DirLink => FOLDER,
        IconClass::File | IconClass::Text => '\u{E7C3}',
        IconClass::Doc => '\u{E8A5}',
        IconClass::Image => '\u{E8B9}',
        IconClass::Video => '\u{E714}',
        IconClass::Audio => '\u{E8D6}',
        IconClass::Archive => '\u{F012}',
        IconClass::Exec => '\u{E7AC}',
        IconClass::Code => '\u{E943}',
        IconClass::Disk => '\u{EDA2}',
        IconClass::Link => LINK,
    }
}

pub const FOLDER: char = '\u{E8B7}';
/// Оверлей ссылки на каталог (8 px) и класс `Link`.
pub const LINK: char = '\u{E71B}';
/// Строка `..`.
pub const PARENT: char = '\u{E74A}';
/// Оверлей облачного файла.
pub const CLOUD: char = '\u{E753}';

pub const CHEVRON_RIGHT: char = '\u{E76C}';
pub const CHEVRON_LEFT: char = '\u{E76B}';
pub const CHEVRON_DOWN: char = '\u{E70D}';
pub const SEARCH: char = '\u{E721}';
/// `+` нового таба.
pub const ADD: char = '\u{E710}';
pub const CANCEL: char = '\u{E711}';
pub const PAUSE: char = '\u{E769}';
pub const PLAY: char = '\u{E768}';
pub const WARNING: char = '\u{E7BA}';
pub const ERROR_BADGE: char = '\u{EA39}';
pub const ACCEPT: char = '\u{E8FB}';
pub const DELETE: char = '\u{E74D}';
pub const COPY: char = '\u{E8C8}';
pub const RENAME: char = '\u{E8AC}';
pub const NEW_FOLDER: char = '\u{E8F4}';
pub const MORE: char = '\u{E712}';
pub const SETTINGS: char = '\u{E713}';
pub const SORT: char = '\u{E8CB}';

/// Кнопки caption (Segoe Fluent Icons / MDL2).
pub const CHROME_MINIMIZE: char = '\u{E921}';
pub const CHROME_MAXIMIZE: char = '\u{E922}';
pub const CHROME_RESTORE: char = '\u{E923}';
pub const CHROME_CLOSE: char = '\u{E8BB}';

/// Все используемые глифы: классы, оверлеи, UI.
pub const ALL: [char; 35] = [
    FOLDER,
    '\u{E7C3}',
    '\u{E8A5}',
    '\u{E8B9}',
    '\u{E714}',
    '\u{E8D6}',
    '\u{F012}',
    '\u{E7AC}',
    '\u{E943}',
    '\u{EDA2}',
    LINK,
    PARENT,
    CLOUD,
    CHEVRON_RIGHT,
    CHEVRON_LEFT,
    CHEVRON_DOWN,
    SEARCH,
    ADD,
    CANCEL,
    PAUSE,
    PLAY,
    WARNING,
    ERROR_BADGE,
    ACCEPT,
    DELETE,
    COPY,
    RENAME,
    NEW_FOLDER,
    MORE,
    SETTINGS,
    SORT,
    CHROME_MINIMIZE,
    CHROME_MAXIMIZE,
    CHROME_RESTORE,
    CHROME_CLOSE,
];

/// Строка всех глифов (прогрев атласа).
pub fn all_text() -> String {
    ALL.iter().collect()
}
