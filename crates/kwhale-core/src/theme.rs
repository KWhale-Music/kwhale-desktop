use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    System,
    #[default]
    Dark,
    Light,
    Amoled,
}

impl ThemeMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "Системная",
            Self::Dark => "Тёмная",
            Self::Light => "Светлая",
            Self::Amoled => "AMOLED",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Dark => "dark",
            Self::Light => "light",
            Self::Amoled => "amoled",
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "system" => Self::System,
            "light" => Self::Light,
            "amoled" => Self::Amoled,
            _ => Self::Dark,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum UiBackend {
    #[default]
    Auto,
    Slint,
    Adw,
    Qt,
}

impl UiBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Slint => "slint",
            Self::Adw => "adw",
            Self::Qt => "qt",
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "slint" | "ui" | "material" => Self::Slint,
            "adw" | "adwaita" | "gtk" | "gtk4" => Self::Adw,
            "qt" | "qt6" | "kde" | "kirigami" => Self::Qt,
            _ => Self::Auto,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PlatformStyle {
    #[default]
    Auto,
    Gnome,
    Kde,
    Windows,
}

impl PlatformStyle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Gnome => "gnome",
            Self::Kde => "kde",
            Self::Windows => "windows",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Авто",
            Self::Gnome => "GNOME",
            Self::Kde => "KDE",
            Self::Windows => "Windows",
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "gnome" | "adw" | "gtk" => Self::Gnome,
            "kde" | "plasma" | "lxqt" | "qt" => Self::Kde,
            "windows" | "win" | "win11" | "fluent" => Self::Windows,
            _ => Self::Auto,
        }
    }

    pub fn resolve(self, xdg_desktop: Option<&str>) -> Self {
        if self != Self::Auto {
            return self;
        }
        if cfg!(target_os = "windows") {
            return Self::Windows;
        }
        let d = xdg_desktop.unwrap_or_default().to_ascii_uppercase();
        if d.contains("KDE") || d.contains("PLASMA") || d.contains("LXQT") {
            Self::Kde
        } else {
            Self::Gnome
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn from_u32(rgb: u32) -> Self {
        Self {
            r: ((rgb >> 16) & 0xFF) as u8,
            g: ((rgb >> 8) & 0xFF) as u8,
            b: (rgb & 0xFF) as u8,
        }
    }

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    pub fn to_rgba_css(self, alpha: f32) -> String {
        format!("rgba({}, {}, {}, {:.3})", self.r, self.g, self.b, alpha.clamp(0.0, 1.0))
    }

    pub fn luminance(self) -> f32 {
        fn lin(c: u8) -> f32 {
            let v = c as f32 / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * lin(self.r) + 0.7152 * lin(self.g) + 0.0722 * lin(self.b)
    }

    pub fn blend(self, other: Self, fraction: f32) -> Self {
        let t = fraction.clamp(0.0, 1.0);
        let lerp = |a: u8, b: u8| -> u8 {
            ((a as f32) + ((b as f32) - (a as f32)) * t).round().clamp(0.0, 255.0) as u8
        };
        Self {
            r: lerp(self.r, other.r),
            g: lerp(self.g, other.g),
            b: lerp(self.b, other.b),
        }
    }

    pub fn on_color(self) -> Self {
        // WCAG relative luminance crossover for readable dark vs light foregrounds.
        if self.luminance() > 0.24 {
            Self::from_u32(0x141229)
        } else {
            Self::from_u32(0xFFFFFF)
        }
    }

    pub fn hue(self) -> f32 {
        let r = self.r as f32 / 255.0;
        let g = self.g as f32 / 255.0;
        let b = self.b as f32 / 255.0;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;
        if delta <= f32::EPSILON {
            return 0.0;
        }
        let mut h = if (max - r).abs() < f32::EPSILON {
            60.0 * (((g - b) / delta) % 6.0)
        } else if (max - g).abs() < f32::EPSILON {
            60.0 * (((b - r) / delta) + 2.0)
        } else {
            60.0 * (((r - g) / delta) + 4.0)
        };
        if h < 0.0 {
            h += 360.0;
        }
        h
    }

    pub fn is_warm(self) -> bool {
        let h = self.hue();
        h < 90.0 || h > 300.0
    }

    pub fn from_hsl(hue: f32, sat: f32, light: f32) -> Self {
        let h = ((hue % 360.0) + 360.0) % 360.0;
        let s = sat.clamp(0.0, 1.0);
        let l = light.clamp(0.0, 1.0);
        let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
        let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
        let m = l - c / 2.0;
        let (rp, gp, bp) = match h {
            h if h < 60.0 => (c, x, 0.0),
            h if h < 120.0 => (x, c, 0.0),
            h if h < 180.0 => (0.0, c, x),
            h if h < 240.0 => (0.0, x, c),
            h if h < 300.0 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        Self {
            r: ((rp + m) * 255.0).round().clamp(0.0, 255.0) as u8,
            g: ((gp + m) * 255.0).round().clamp(0.0, 255.0) as u8,
            b: ((bp + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KwhaleAccent {
    pub id: i32,
    pub name: &'static str,
    pub rgb_u32: u32,
}

impl KwhaleAccent {
    pub const fn rgb(self) -> Rgb {
        Rgb::from_u32(self.rgb_u32)
    }
}

/// All 129 accents lifted directly from `KwhaleAccentCatalog.kt`.
pub const ALL_ACCENTS: &[KwhaleAccent] = &[
    KwhaleAccent { id: 0, name: "По умолчанию", rgb_u32: 0x448AFF },
    KwhaleAccent { id: 1, name: "VK", rgb_u32: 0x5181B1 },
    KwhaleAccent { id: 2, name: "VK (светлый)", rgb_u32: 0x71AAEB },
    KwhaleAccent { id: 3, name: "Темно-синий", rgb_u32: 0x61A7E0 },
    KwhaleAccent { id: 4, name: "Temporary Bliss", rgb_u32: 0xE1637E },
    KwhaleAccent { id: 5, name: "OwO", rgb_u32: 0xFF00FF },
    KwhaleAccent { id: 6, name: "Rena", rgb_u32: 0xE52C5F },
    KwhaleAccent { id: 7, name: "AEX", rgb_u32: 0x21EF8B },
    KwhaleAccent { id: 8, name: "Modified Dark", rgb_u32: 0xAB0000 },
    KwhaleAccent { id: 9, name: "Неоновый", rgb_u32: 0xFF34B3 },
    KwhaleAccent { id: 10, name: "neth3me", rgb_u32: 0x9085FF },
    KwhaleAccent { id: 11, name: "iOS: Синий", rgb_u32: 0x0A84FF },
    KwhaleAccent { id: 12, name: "iOS: Зеленый", rgb_u32: 0x30D158 },
    KwhaleAccent { id: 13, name: "iOS: Индиго", rgb_u32: 0x5E5CE6 },
    KwhaleAccent { id: 14, name: "iOS: Оранжевый", rgb_u32: 0xFF9F0A },
    KwhaleAccent { id: 15, name: "iOS: Розовый", rgb_u32: 0xFF375F },
    KwhaleAccent { id: 16, name: "iOS: Пурпурный", rgb_u32: 0xBF5AF2 },
    KwhaleAccent { id: 17, name: "iOS: Красный", rgb_u32: 0xFF453A },
    KwhaleAccent { id: 18, name: "iOS: Сине-зеленый", rgb_u32: 0x64D2FF },
    KwhaleAccent { id: 19, name: "iOS: Желтый", rgb_u32: 0xFFD60A },
    KwhaleAccent { id: 20, name: "Netflix", rgb_u32: 0xE50914 },
    KwhaleAccent { id: 21, name: "Q: Океан", rgb_u32: 0x0A84FF },
    KwhaleAccent { id: 22, name: "Q: Корица", rgb_u32: 0xAF6050 },
    KwhaleAccent { id: 23, name: "Q: Зеленый", rgb_u32: 0x1B873B },
    KwhaleAccent { id: 24, name: "Q: Орхидея", rgb_u32: 0xC42CC9 },
    KwhaleAccent { id: 25, name: "Q: Пурпурный", rgb_u32: 0x725AFF },
    KwhaleAccent { id: 26, name: "Q: Космос", rgb_u32: 0x47618A },
    KwhaleAccent { id: 27, name: "VK Me", rgb_u32: 0x3F8AE0 },
    KwhaleAccent { id: 28, name: "Wayne", rgb_u32: 0xFFA741 },
    KwhaleAccent { id: 29, name: "XMB: Январь", rgb_u32: 0xCBCBCB },
    KwhaleAccent { id: 30, name: "XMB: Февраль", rgb_u32: 0xD8BF1A },
    KwhaleAccent { id: 31, name: "XMB: Март", rgb_u32: 0x6DB217 },
    KwhaleAccent { id: 32, name: "XMB: Апрель", rgb_u32: 0xE17E9A },
    KwhaleAccent { id: 33, name: "XMB: Май", rgb_u32: 0x178816 },
    KwhaleAccent { id: 34, name: "XMB: Июнь", rgb_u32: 0x9A61C8 },
    KwhaleAccent { id: 35, name: "XMB: Июль", rgb_u32: 0x02CDC7 },
    KwhaleAccent { id: 36, name: "XMB: Август", rgb_u32: 0x0C76C0 },
    KwhaleAccent { id: 37, name: "XMB: Сентябрь", rgb_u32: 0xB444C0 },
    KwhaleAccent { id: 38, name: "XMB: Октябрь", rgb_u32: 0xE5A708 },
    KwhaleAccent { id: 39, name: "XMB: Ноябрь", rgb_u32: 0x875B1E },
    KwhaleAccent { id: 40, name: "XMB: Декабрь", rgb_u32: 0xE3412A },
    KwhaleAccent { id: 41, name: "Solarized: Желтый", rgb_u32: 0xB58900 },
    KwhaleAccent { id: 42, name: "Solarized: Оранжевый", rgb_u32: 0xCB4B16 },
    KwhaleAccent { id: 43, name: "Solarized: Красный", rgb_u32: 0xDC322F },
    KwhaleAccent { id: 44, name: "Solarized: Пурпурный", rgb_u32: 0xD33682 },
    KwhaleAccent { id: 45, name: "Solarized: Фиолетовый", rgb_u32: 0x6C71C4 },
    KwhaleAccent { id: 46, name: "Solarized: Синий", rgb_u32: 0x268BD2 },
    KwhaleAccent { id: 47, name: "Solarized: Сине-зеленый", rgb_u32: 0x2AA198 },
    KwhaleAccent { id: 48, name: "Solarized: Зеленый", rgb_u32: 0x859900 },
    KwhaleAccent { id: 49, name: "Формалин", rgb_u32: 0x8AB4F8 },
    KwhaleAccent { id: 50, name: "Северное сияние", rgb_u32: 0x299A8B },
    KwhaleAccent { id: 51, name: "Ukrrooter", rgb_u32: 0x00FF00 },
    KwhaleAccent { id: 52, name: "Revz", rgb_u32: 0xE16364 },
    KwhaleAccent { id: 53, name: "Crimson", rgb_u32: 0xE55758 },
    KwhaleAccent { id: 54, name: "Ubuntu", rgb_u32: 0xE95420 },
    KwhaleAccent { id: 55, name: "Gentoo", rgb_u32: 0x54487A },
    KwhaleAccent { id: 56, name: "Kiwifruit", rgb_u32: 0x1DB954 },
    KwhaleAccent { id: 57, name: "White Red", rgb_u32: 0xCE0041 },
    KwhaleAccent { id: 58, name: "Mint", rgb_u32: 0x00A273 },
    KwhaleAccent { id: 59, name: "Sakura", rgb_u32: 0xEAB8B7 },
    KwhaleAccent { id: 60, name: "Pacific Ocean", rgb_u32: 0x149ECC },
    KwhaleAccent { id: 61, name: "Pacific Ocean Dark", rgb_u32: 0x2185A6 },
    KwhaleAccent { id: 62, name: "Jam", rgb_u32: 0xCC3D6D },
    KwhaleAccent { id: 63, name: "Jam Dark", rgb_u32: 0xB2365F },
    KwhaleAccent { id: 64, name: "Forest Green", rgb_u32: 0x1F991F },
    KwhaleAccent { id: 65, name: "Forest Green Dark", rgb_u32: 0x4C994C },
    KwhaleAccent { id: 66, name: "Deep Lilac", rgb_u32: 0x9D52CC },
    KwhaleAccent { id: 67, name: "Deep Lilac Dark", rgb_u32: 0x8042A6 },
    KwhaleAccent { id: 68, name: "Carrot", rgb_u32: 0xBF5926 },
    KwhaleAccent { id: 69, name: "Carrot Dark", rgb_u32: 0xF28149 },
    KwhaleAccent { id: 70, name: "Ярко-синий", rgb_u32: 0x319DE6 },
    KwhaleAccent { id: 71, name: "Ярко-оранжевый", rgb_u32: 0xFE822F },
    KwhaleAccent { id: 72, name: "UWP: Лайм", rgb_u32: 0xA4C400 },
    KwhaleAccent { id: 73, name: "UWP: Зеленый", rgb_u32: 0x60A917 },
    KwhaleAccent { id: 74, name: "UWP: Изумруд", rgb_u32: 0x008A00 },
    KwhaleAccent { id: 75, name: "UWP: Teal", rgb_u32: 0x00ABA9 },
    KwhaleAccent { id: 76, name: "UWP: Циан", rgb_u32: 0x1BA1E2 },
    KwhaleAccent { id: 77, name: "UWP: Кобальт", rgb_u32: 0x3E65FF },
    KwhaleAccent { id: 78, name: "UWP: Индиго", rgb_u32: 0x6A00FF },
    KwhaleAccent { id: 79, name: "UWP: Фиолетовый", rgb_u32: 0xAA00FF },
    KwhaleAccent { id: 80, name: "UWP: Розовый", rgb_u32: 0xF472D0 },
    KwhaleAccent { id: 81, name: "UWP: Magenta", rgb_u32: 0xD80073 },
    KwhaleAccent { id: 82, name: "UWP: Кримсон", rgb_u32: 0xA20025 },
    KwhaleAccent { id: 83, name: "UWP: Красный", rgb_u32: 0xE51400 },
    KwhaleAccent { id: 84, name: "UWP: Оранжевый", rgb_u32: 0xFA6800 },
    KwhaleAccent { id: 85, name: "UWP: Янтарь", rgb_u32: 0xF0A30A },
    KwhaleAccent { id: 86, name: "UWP: Желтый", rgb_u32: 0xE3C800 },
    KwhaleAccent { id: 87, name: "UWP: Коричневый", rgb_u32: 0x825A2C },
    KwhaleAccent { id: 88, name: "UWP: Оливковый", rgb_u32: 0x6D8764 },
    KwhaleAccent { id: 89, name: "UWP: Сталь", rgb_u32: 0x647687 },
    KwhaleAccent { id: 90, name: "UWP: Mauve", rgb_u32: 0x76608A },
    KwhaleAccent { id: 91, name: "UWP: Taupe", rgb_u32: 0x87794E },
    KwhaleAccent { id: 92, name: "Ruby", rgb_u32: 0xFFBCBC },
    KwhaleAccent { id: 93, name: "Пиво", rgb_u32: 0xFFBF00 },
    KwhaleAccent { id: 94, name: "Moonstone", rgb_u32: 0xFD7492 },
    KwhaleAccent { id: 95, name: "Jellygram", rgb_u32: 0xFF6088 },
    KwhaleAccent { id: 96, name: "Mineshaft Gold", rgb_u32: 0xCFAB73 },
    KwhaleAccent { id: 97, name: "Midnight Firewatch", rgb_u32: 0xADDDB1 },
    KwhaleAccent { id: 98, name: "Nord: Frost", rgb_u32: 0x5E81AC },
    KwhaleAccent { id: 99, name: "Nord: Aurora Red", rgb_u32: 0xBF616A },
    KwhaleAccent { id: 100, name: "Nord: Aurora Orange", rgb_u32: 0xD08770 },
    KwhaleAccent { id: 101, name: "Nord: Aurora Yellow", rgb_u32: 0xEBCB8B },
    KwhaleAccent { id: 102, name: "Nord: Aurora Green", rgb_u32: 0xA3BE8C },
    KwhaleAccent { id: 103, name: "Nord: Aurora Purple", rgb_u32: 0xB48EAD },
    KwhaleAccent { id: 104, name: "Gruvbox: Red", rgb_u32: 0xFB4934 },
    KwhaleAccent { id: 105, name: "Gruvbox: Green", rgb_u32: 0xB8BB26 },
    KwhaleAccent { id: 106, name: "Gruvbox: Yellow", rgb_u32: 0xFABD2F },
    KwhaleAccent { id: 107, name: "Gruvbox: Blue", rgb_u32: 0x83A598 },
    KwhaleAccent { id: 108, name: "Gruvbox: Purple", rgb_u32: 0xD3869B },
    KwhaleAccent { id: 109, name: "Gruvbox: Aqua", rgb_u32: 0x8EC07C },
    KwhaleAccent { id: 110, name: "Gruvbox: Orange", rgb_u32: 0xFE8019 },
    KwhaleAccent { id: 111, name: "Gruvbox: Black", rgb_u32: 0x504945 },
    KwhaleAccent { id: 112, name: "Gruvbox: White", rgb_u32: 0xD5C4A1 },
    KwhaleAccent { id: 113, name: "WD: Error (dark)", rgb_u32: 0x942929 },
    KwhaleAccent { id: 114, name: "WD: Error (light)", rgb_u32: 0xD23D3D },
    KwhaleAccent { id: 115, name: "WD: Accent (dark)", rgb_u32: 0x06AEFF },
    KwhaleAccent { id: 116, name: "WD: Accent (light)", rgb_u32: 0x33C8F6 },
    KwhaleAccent { id: 117, name: "WD: Warning (dark)", rgb_u32: 0x69641C },
    KwhaleAccent { id: 118, name: "WD: Warning (light)", rgb_u32: 0xC8D23D },
    KwhaleAccent { id: 119, name: "WD: Network", rgb_u32: 0x9461D7 },
    KwhaleAccent { id: 120, name: "Steam Green", rgb_u32: 0xBAB581 },
    KwhaleAccent { id: 121, name: "Accent (light)", rgb_u32: 0x1C71D8 },
    KwhaleAccent { id: 122, name: "Accent (dark)", rgb_u32: 0x78AEED },
    KwhaleAccent { id: 123, name: "Destructive (light)", rgb_u32: 0xC01C28 },
    KwhaleAccent { id: 124, name: "Destructive (dark)", rgb_u32: 0xFF7B63 },
    KwhaleAccent { id: 125, name: "Success (light)", rgb_u32: 0x26A269 },
    KwhaleAccent { id: 126, name: "Success (dark)", rgb_u32: 0x8FF0A4 },
    KwhaleAccent { id: 127, name: "Warning (light)", rgb_u32: 0xAE7B03 },
    KwhaleAccent { id: 128, name: "Warning (dark)", rgb_u32: 0xF8E45C },
];

pub fn accent_by_id(id: i32) -> KwhaleAccent {
    ALL_ACCENTS
        .iter()
        .copied()
        .find(|a| a.id == id)
        .unwrap_or(ALL_ACCENTS[2])
}

pub struct AccentFamily {
    pub title: &'static str,
    pub accents: Vec<KwhaleAccent>,
}

pub fn accent_families() -> Vec<AccentFamily> {
    let base_ids = [0, 1, 2, 3, 11, 12, 13, 14, 15, 16];
    let base: Vec<KwhaleAccent> = base_ids.iter().map(|id| accent_by_id(*id)).collect();
    let warm: Vec<KwhaleAccent> = ALL_ACCENTS
        .iter()
        .copied()
        .filter(|a| (20..=45).contains(&a.id) && a.rgb().is_warm())
        .collect();
    let cool: Vec<KwhaleAccent> = ALL_ACCENTS
        .iter()
        .copied()
        .filter(|a| (20..=45).contains(&a.id) && !a.rgb().is_warm())
        .collect();
    let nature: Vec<KwhaleAccent> = ALL_ACCENTS
        .iter()
        .copied()
        .filter(|a| (46..=75).contains(&a.id))
        .collect();
    let community: Vec<KwhaleAccent> = ALL_ACCENTS
        .iter()
        .copied()
        .filter(|a| (76..=105).contains(&a.id))
        .collect();
    let system: Vec<KwhaleAccent> = ALL_ACCENTS
        .iter()
        .copied()
        .filter(|a| a.id >= 106)
        .collect();
    vec![
        AccentFamily { title: "Базовые", accents: base },
        AccentFamily { title: "Тёплые", accents: warm },
        AccentFamily { title: "Холодные", accents: cool },
        AccentFamily { title: "Природа", accents: nature },
        AccentFamily { title: "Сообщество", accents: community },
        AccentFamily { title: "Системные", accents: system },
    ]
    .into_iter()
    .filter(|f| !f.accents.is_empty())
    .collect()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KwhalePalette {
    pub is_dark: bool,
    pub is_amoled: bool,
    pub primary: Rgb,
    pub on_primary: Rgb,
    pub primary_container: Rgb,
    pub on_primary_container: Rgb,
    pub secondary_container: Rgb,
    pub on_secondary_container: Rgb,
    pub surface: Rgb,
    pub surface_container_lowest: Rgb,
    pub surface_container_low: Rgb,
    pub surface_container: Rgb,
    pub surface_container_high: Rgb,
    pub surface_container_highest: Rgb,
    pub on_surface: Rgb,
    pub on_surface_variant: Rgb,
    pub outline: Rgb,
    pub outline_variant: Rgb,
    pub error: Rgb,
    pub error_container: Rgb,
}

impl KwhalePalette {
    pub fn build(mode: ThemeMode, accent: Rgb, system_is_dark: bool) -> Self {
        let use_dark = match mode {
            ThemeMode::Light => false,
            ThemeMode::Dark | ThemeMode::Amoled => true,
            ThemeMode::System => system_is_dark,
        };
        if use_dark {
            Self::dark(accent, mode == ThemeMode::Amoled)
        } else {
            Self::light(accent)
        }
    }

    fn dark(accent: Rgb, amoled: bool) -> Self {
        let tint_base = Rgb::from_u32(0x111119);
        let surface = if amoled {
            Rgb::from_u32(0x000000)
        } else {
            tint_base.blend(accent, 0.04)
        };
        let container_lowest = if amoled {
            Rgb::from_u32(0x000000)
        } else {
            Rgb::from_u32(0x0C0C13).blend(accent, 0.03)
        };
        let container_low = if amoled {
            Rgb::from_u32(0x09090D).blend(accent, 0.03)
        } else {
            Rgb::from_u32(0x161621).blend(accent, 0.05)
        };
        let container = if amoled {
            Rgb::from_u32(0x0F0F15).blend(accent, 0.04)
        } else {
            Rgb::from_u32(0x1C1C2A).blend(accent, 0.07)
        };
        let container_high = if amoled {
            Rgb::from_u32(0x171720).blend(accent, 0.05)
        } else {
            Rgb::from_u32(0x252536).blend(accent, 0.09)
        };
        let container_highest = if amoled {
            Rgb::from_u32(0x20202C).blend(accent, 0.06)
        } else {
            Rgb::from_u32(0x303044).blend(accent, 0.11)
        };

        let white = Rgb::from_u32(0xFFFFFF);
        let primary = if accent.luminance() < 0.42 {
            accent.blend(white, 0.36)
        } else {
            accent
        };
        let on_primary = primary.on_color();
        let primary_container = container.blend(accent, 0.28);
        let on_primary_container = Rgb::from_u32(if primary_container.luminance() > 0.48 {
            0x0B0B14
        } else {
            0xEFEBFD
        });
        let secondary_container = container.blend(accent, 0.16);
        let on_secondary_container = Rgb::from_u32(if secondary_container.luminance() > 0.48 {
            0x0B0B14
        } else {
            0xDDD8F2
        });

        Self {
            is_dark: true,
            is_amoled: amoled,
            primary,
            on_primary,
            primary_container,
            on_primary_container,
            secondary_container,
            on_secondary_container,
            surface,
            surface_container_lowest: container_lowest,
            surface_container_low: container_low,
            surface_container: container,
            surface_container_high: container_high,
            surface_container_highest: container_highest,
            on_surface: Rgb::from_u32(0xEEEAF6),
            on_surface_variant: Rgb::from_u32(0xABA7BA),
            outline: Rgb::from_u32(0x7B778D),
            outline_variant: Rgb::from_u32(0x2E2D40).blend(accent, 0.10),
            error: Rgb::from_u32(0xFFB4AB),
            error_container: Rgb::from_u32(0x93000A),
        }
    }

    fn light(accent: Rgb) -> Self {
        let white = Rgb::from_u32(0xFFFFFF);
        let black = Rgb::from_u32(0x000000);
        let surface = Rgb::from_u32(0xF7F7FC).blend(accent, 0.02);
        let primary = if accent.luminance() > 0.28 {
            accent.blend(black, 0.38)
        } else {
            accent
        };
        let on_primary = primary.on_color();
        let primary_container = white.blend(accent, 0.20);
        let on_primary_container = Rgb::from_u32(if primary_container.luminance() > 0.48 {
            0x101324
        } else {
            0x001D36
        });
        let secondary_container = surface.blend(accent, 0.14);
        let on_secondary_container = Rgb::from_u32(if secondary_container.luminance() > 0.48 {
            0x141728
        } else {
            0x101C26
        });

        Self {
            is_dark: false,
            is_amoled: false,
            primary,
            on_primary,
            primary_container,
            on_primary_container,
            secondary_container,
            on_secondary_container,
            surface,
            surface_container_lowest: white,
            surface_container_low: Rgb::from_u32(0xEFF0F8).blend(accent, 0.03),
            surface_container: Rgb::from_u32(0xFFFFFF),
            surface_container_high: Rgb::from_u32(0xE8E9F3).blend(accent, 0.04),
            surface_container_highest: Rgb::from_u32(0xDFDFF0).blend(accent, 0.06),
            on_surface: Rgb::from_u32(0x181A24),
            on_surface_variant: Rgb::from_u32(0x525568),
            outline: Rgb::from_u32(0x76788C),
            outline_variant: Rgb::from_u32(0xD0D2E2).blend(accent, 0.08),
            error: Rgb::from_u32(0xBA1A1A),
            error_container: Rgb::from_u32(0xFFDAD6),
        }
    }
}

/// Deterministic FNV-1a cover gradient generator matching KwhaleCoverPalette.
pub struct KwhaleCoverPalette;

impl KwhaleCoverPalette {
    fn fnv1a_hash(seed: &str) -> u32 {
        let mut h: i32 = -0x7EE3_623B;
        for ch in seed.chars() {
            h ^= ch as i32;
            h = h.wrapping_mul(0x0100_0193);
        }
        if h == i32::MIN {
            0
        } else {
            h.unsigned_abs()
        }
    }

    pub fn gradient(seed: &str, dark: bool) -> (Rgb, Rgb) {
        let clean = if seed.trim().is_empty() {
            "kwhale"
        } else {
            seed.trim()
        };
        if clean == "Вечерний город" {
            return if dark {
                (Rgb::from_u32(0x7B5CFF), Rgb::from_u32(0x281D59))
            } else {
                (Rgb::from_u32(0xA693FF), Rgb::from_u32(0x5240A4))
            };
        }
        let h = Self::fnv1a_hash(clean);
        // Curated harmonic hues (violet, indigo, magenta-rose, amethyst, ocean, dusk-coral, sapphire, slate-blue)
        const CURATED_HUES: [f32; 8] = [254.0, 226.0, 324.0, 278.0, 204.0, 344.0, 294.0, 238.0];
        let base_hue = CURATED_HUES[(h as usize) % CURATED_HUES.len()];
        let hue = (base_hue + ((h / 8) % 14) as f32) % 360.0;
        let hue2 = (hue + 28.0 + ((h / 144) % 18) as f32) % 360.0;
        if dark {
            (
                Rgb::from_hsl(hue, 0.62, 0.48),
                Rgb::from_hsl(hue2, 0.58, 0.17),
            )
        } else {
            (
                Rgb::from_hsl(hue, 0.66, 0.74),
                Rgb::from_hsl(hue2, 0.56, 0.48),
            )
        }
    }

    pub fn initial_of(seed: &str) -> String {
        let clean = seed.trim().trim_start_matches(['«', '"', '\'', '(', '[']);
        clean
            .chars()
            .next()
            .map(|c| c.to_uppercase().collect::<String>())
            .unwrap_or_default()
    }

    /// Generates a crisp SVG cover placeholder (used by Qt QQuickImageProvider / GTK Pixbuf fallback).
    pub fn placeholder_svg(seed: &str, dark: bool, show_initial: bool, size: u32) -> String {
        let (c1, c2) = Self::gradient(seed, dark);
        let fg = if c1.luminance() > 0.5 {
            "#14161A"
        } else {
            "#FFFFFF"
        };
        let initial = if show_initial {
            Self::initial_of(seed)
        } else {
            String::new()
        };
        let escaped = initial
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        let font_size = (size as f32 * 0.40).round() as u32;
        format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 {size} {size}">
  <defs>
    <linearGradient id="g" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="{c1}"/>
      <stop offset="100%" stop-color="{c2}"/>
    </linearGradient>
  </defs>
  <rect width="{size}" height="{size}" fill="url(#g)"/>
  <text x="50%" y="54%" dominant-baseline="middle" text-anchor="middle" fill="{fg}" font-family="VK Sans Display, sans-serif" font-weight="700" font-size="{font_size}">{escaped}</text>
</svg>"##,
            c1 = c1.to_hex(),
            c2 = c2.to_hex(),
        )
    }

    /// Generates an RGBA8 pixel buffer with atmospheric procedural cover artwork
    /// (3D cosmic orb, flowing silk waves, twilight cityscape, or lit dusk facade)
    /// and anti-aliased rounded corners for Slint `Image`.
    pub fn placeholder_rgba(seed: &str, dark: bool, size: u32, corner_radius: f32) -> Vec<u8> {
        let size = size.max(1);
        let clean = if seed.trim().is_empty() {
            "kwhale"
        } else {
            seed.trim()
        };
        let h = Self::fnv1a_hash(clean);
        let (c1, c2) = Self::gradient(clean, dark);
        let warm = Rgb::from_u32(match h % 4 {
            0 => 0xEC6898,
            1 => 0xF08270,
            2 => 0xC678FF,
            _ => 0x6AA8FF,
        });
        let accent = c1.blend(Rgb::from_u32(0xFFFFFF), if dark { 0.34 } else { 0.40 });
        let dark_ink = Rgb::from_u32(0x131126);
        let motif = match clean {
            "Вечерний город" | "Утро" | "В дороге" => 2,
            "Электроника" | "Вечерний микс" | "Синоптик" => 1,
            "Dreamscape" | "Сигнал из космоса" | "Пыяла" => 0,
            "Новые треки" | "Морская" | "Для работы" => 3,
            _ => {
                let mixed = h.wrapping_mul(0x9E37_79B9);
                ((mixed >> 16) ^ mixed) % 4
            }
        };

        let mut out = vec![0u8; (size * size * 4) as usize];
        let s = size as f32;
        let half = s * 0.5;
        let r = corner_radius.clamp(0.0, half);

        for y in 0..size {
            let fy = y as f32 + 0.5;
            let ty = fy / s;
            for x in 0..size {
                let fx = x as f32 + 0.5;
                let tx = fx / s;

                let alpha = if r > 0.0 {
                    let qx = (fx - half).abs() - (half - r);
                    let qy = (fy - half).abs() - (half - r);
                    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
                    let inside = qx.max(qy).min(0.0);
                    let dist = outside + inside - r;
                    (0.5 - dist).clamp(0.0, 1.0)
                } else {
                    1.0
                };

                let col = match motif {
                    // 0: 3D lit Cosmic Sphere + Fresnel rim + soft ambient halo
                    0 => {
                        let bg = c2.blend(dark_ink, if dark { 0.55 } else { 0.12 });
                        let mut c = bg.blend(c1, ((1.0 - ty) * 0.30).clamp(0.0, 1.0));
                        let dx = tx - 0.5;
                        let dy = ty - 0.48;
                        let d = (dx * dx + dy * dy).sqrt();
                        let halo = (1.0 - d * 1.95).clamp(0.0, 1.0).powi(2);
                        c = c.blend(warm, halo * 0.34);
                        let radius = 0.30;
                        if d < radius {
                            let nz = (1.0 - (d / radius).powi(2)).max(0.0).sqrt();
                            let nx = dx / radius;
                            let ny = dy / radius;
                            let lambert = (nx * -0.45 + ny * -0.45 + nz * 0.77).clamp(0.0, 1.0);
                            let rim = (1.0 - nz).powi(2);
                            let mut orb = c2.blend(c1, lambert * 0.85);
                            orb = orb.blend(warm, (0.35 + ny * 0.40).clamp(0.0, 0.85));
                            orb = orb.blend(accent, (lambert.powi(3) * 0.55 + rim * 0.40).clamp(0.0, 0.85));
                            let edge = ((radius - d) * 65.0).clamp(0.0, 1.0);
                            c = c.blend(orb, edge);
                        }
                        c
                    }
                    // 1: Layered Silk Waves with luminous crests
                    1 => {
                        let mut c = c1.blend(warm, (tx * 0.45 + (1.0 - ty) * 0.35).clamp(0.0, 0.75));
                        let phase = (h % 7) as f32 * 0.7;
                        let w1 = 0.34 + 0.12 * (tx * 6.0 + phase).sin();
                        let w2 = 0.54 + 0.11 * (tx * 5.2 + phase + 1.8).cos();
                        let w3 = 0.74 + 0.09 * (tx * 6.8 + phase + 0.5).sin();
                        let e1 = ((ty - w1) * 48.0).clamp(0.0, 1.0);
                        let g1 = (1.0 - (ty - w1).abs() * 14.0).clamp(0.0, 1.0);
                        let e2 = ((ty - w2) * 48.0).clamp(0.0, 1.0);
                        let g2 = (1.0 - (ty - w2).abs() * 14.0).clamp(0.0, 1.0);
                        let e3 = ((ty - w3) * 48.0).clamp(0.0, 1.0);
                        c = c.blend(warm.blend(c1, 0.35), e1 * 0.82);
                        c = c.blend(accent, g1 * 0.24);
                        c = c.blend(c1.blend(c2, 0.45), e2 * 0.86);
                        c = c.blend(warm, g2 * 0.22);
                        c.blend(c2.blend(dark_ink, 0.45), e3 * 0.90)
                    }
                    // 2: Sunset Horizon + Glowing Orb Sun + Distant Dusk Hills/Cityline
                    2 => {
                        let sky = c2.blend(c1, (ty * 0.8).clamp(0.0, 1.0));
                        let mut c = sky.blend(warm, (ty * 1.15 - 0.15).clamp(0.0, 0.82));
                        let sun_d = ((tx - 0.62).powi(2) + (ty - 0.46).powi(2)).sqrt();
                        let glow = (1.0 - sun_d * 2.1).clamp(0.0, 1.0).powi(2);
                        c = c.blend(Rgb::from_u32(0xF69B78), glow * 0.55);
                        if sun_d < 0.135 {
                            let sun_edge = ((0.135 - sun_d) * 55.0).clamp(0.0, 1.0);
                            c = c.blend(accent.blend(Rgb::from_u32(0xFFD1B3), 0.45), sun_edge * 0.82);
                        }
                        let city_h = if (0.30..0.37).contains(&tx) {
                            0.60
                        } else if (0.42..0.48).contains(&tx) {
                            0.63
                        } else if (0.72..0.79).contains(&tx) {
                            0.61
                        } else {
                            1.0
                        };
                        let ridge = (0.68 + 0.055 * (tx * 8.5).sin() + 0.025 * (tx * 20.0).cos()).min(city_h);
                        let ridge_mask = ((ty - ridge) * 55.0).clamp(0.0, 1.0);
                        c.blend(dark_ink, ridge_mask * 0.86)
                    }
                    // 3: Architectural Dusk Facade + Warm Diagonal Sun + Window Grid
                    _ => {
                        let dusk = c1.blend(warm, (tx * 0.55 + (1.0 - ty) * 0.25).clamp(0.0, 0.72));
                        let mut c = dusk.blend(accent, ((1.0 - tx) * 0.20 * (1.0 - ty)).clamp(0.0, 0.4));
                        let roof_y = 0.24 + (tx - 0.20) * 0.42;
                        if tx > 0.22 && ty > roof_y {
                            let is_front = tx < 0.66;
                            let base_wall = if is_front {
                                c2.blend(warm, 0.26)
                            } else {
                                c2.blend(dark_ink, 0.58)
                            };
                            let shade = ((ty - roof_y) * 0.9).clamp(0.0, 0.55);
                            let mut wall = base_wall.blend(dark_ink, shade);
                            if is_front && ty > roof_y + 0.06 {
                                let gx = ((tx - 0.26) * 14.0).fract();
                                let gy = ((ty - roof_y - 0.06) * 12.0).fract();
                                if gx > 0.22 && gx < 0.72 && gy > 0.20 && gy < 0.68 {
                                    wall = wall.blend(dark_ink, 0.45);
                                }
                            }
                            let bevel = ((ty - roof_y) * 80.0).clamp(0.0, 1.0);
                            c = c.blend(wall, bevel * 0.92);
                        }
                        c
                    }
                };

                let idx = ((y * size + x) * 4) as usize;
                out[idx] = col.r;
                out[idx + 1] = col.g;
                out[idx + 2] = col.b;
                out[idx + 3] = (alpha * 255.0).round() as u8;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_129_accents_present_and_contrast_safe() {
        assert_eq!(ALL_ACCENTS.len(), 129);
        for accent in ALL_ACCENTS {
            let dark = KwhalePalette::build(ThemeMode::Dark, accent.rgb(), true);
            let light = KwhalePalette::build(ThemeMode::Light, accent.rgb(), false);
            assert_ne!(dark.primary, dark.on_primary);
            assert_ne!(light.primary, light.on_primary);
        }
    }

    #[test]
    fn placeholder_svg_escapes_and_uses_seed() {
        let svg = KwhaleCoverPalette::placeholder_svg("«Кино»", true, true, 128);
        assert!(svg.contains(">К<"));
        let svg2 = KwhaleCoverPalette::placeholder_svg("Кино", true, true, 128);
        assert!(svg2.contains(">К<"));
    }

    #[test]
    fn platform_style_resolution_handles_desktops() {
        assert_eq!(
            PlatformStyle::Gnome.resolve(Some("KDE")),
            PlatformStyle::Gnome
        );
        assert_eq!(
            PlatformStyle::Auto.resolve(Some("KDE")),
            if cfg!(target_os = "windows") {
                PlatformStyle::Windows
            } else {
                PlatformStyle::Kde
            }
        );
        assert_eq!(
            PlatformStyle::Auto.resolve(Some("ubuntu:GNOME")),
            if cfg!(target_os = "windows") {
                PlatformStyle::Windows
            } else {
                PlatformStyle::Gnome
            }
        );
    }

    #[test]
    fn placeholder_rgba_renders_rounded_buffer() {
        let buf = KwhaleCoverPalette::placeholder_rgba("Кино", true, 32, 8.0);
        assert_eq!(buf.len(), 32 * 32 * 4);
        // Corner pixel (0,0) should be transparent because of rounded SDF mask.
        assert_eq!(buf[3], 0);
        // Center pixel (16,16) should be fully opaque.
        let center_idx = (16 * 32 + 16) * 4;
        assert_eq!(buf[center_idx + 3], 255);
    }
}
