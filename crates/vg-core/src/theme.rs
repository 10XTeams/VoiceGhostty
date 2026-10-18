//! 终端配色主题:前景/背景/光标 + 16 色 ANSI 调色板。与 Swift 版 Theme.swift 的四套内置主题一致。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// 解析 "#rrggbb" / "rrggbb";非法输入返回 None
    pub fn parse(raw: &str) -> Option<Rgb> {
        let s = raw.trim().trim_start_matches('#');
        if s.len() != 6 {
            return None;
        }
        let v = u32::from_str_radix(s, 16).ok()?;
        Some(Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
    }

    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.0, self.1, self.2)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    /// 同时也是配置文件里 `theme =` 的取值
    pub id: &'static str,
    pub display_name: &'static str,
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    pub ansi: [Rgb; 16],
}

const fn hex(v: u32) -> Rgb {
    Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

impl Theme {
    pub const DARK: Theme = Theme {
        id: "dark",
        display_name: "Dark",
        foreground: hex(0xd4d4d4),
        background: hex(0x1e1e1e),
        cursor: hex(0xffffff),
        ansi: [
            hex(0x1e1e1e), hex(0xcd3131), hex(0x0dbc79), hex(0xe5e510),
            hex(0x2472c8), hex(0xbc3fbc), hex(0x11a8cd), hex(0xe5e5e5),
            hex(0x666666), hex(0xf14c4c), hex(0x23d18b), hex(0xf5f543),
            hex(0x3b8eea), hex(0xd670d6), hex(0x29b8db), hex(0xffffff),
        ],
    };

    pub const LIGHT: Theme = Theme {
        id: "light",
        display_name: "Light",
        foreground: hex(0x1a1a1a),
        background: hex(0xffffff),
        cursor: hex(0x000000),
        ansi: [
            hex(0x000000), hex(0xc91b00), hex(0x00c200), hex(0xc7c400),
            hex(0x0225c7), hex(0xc930c7), hex(0x00c5c7), hex(0xc7c7c7),
            hex(0x686868), hex(0xff6e67), hex(0x5ffa68), hex(0xfffc67),
            hex(0x6871ff), hex(0xff77ff), hex(0x60fdff), hex(0xffffff),
        ],
    };

    pub const SOLARIZED_DARK: Theme = Theme {
        id: "solarized-dark",
        display_name: "Solarized Dark",
        foreground: hex(0x839496),
        background: hex(0x002b36),
        cursor: hex(0x93a1a1),
        ansi: [
            hex(0x073642), hex(0xdc322f), hex(0x859900), hex(0xb58900),
            hex(0x268bd2), hex(0xd33682), hex(0x2aa198), hex(0xeee8d5),
            hex(0x002b36), hex(0xcb4b16), hex(0x586e75), hex(0x657b83),
            hex(0x839496), hex(0x6c71c4), hex(0x93a1a1), hex(0xfdf6e3),
        ],
    };

    pub const DRACULA: Theme = Theme {
        id: "dracula",
        display_name: "Dracula",
        foreground: hex(0xf8f8f2),
        background: hex(0x282a36),
        cursor: hex(0xf8f8f2),
        ansi: [
            hex(0x21222c), hex(0xff5555), hex(0x50fa7b), hex(0xf1fa8c),
            hex(0xbd93f9), hex(0xff79c6), hex(0x8be9fd), hex(0xf8f8f2),
            hex(0x6272a4), hex(0xff6e6e), hex(0x69ff94), hex(0xffffa5),
            hex(0xd6acff), hex(0xff92df), hex(0xa4ffff), hex(0xffffff),
        ],
    };

    pub const ALL: [&'static Theme; 4] = [
        &Theme::DARK,
        &Theme::LIGHT,
        &Theme::SOLARIZED_DARK,
        &Theme::DRACULA,
    ];

    /// 按 id 查找,找不到回退到 dark
    pub fn named(id: &str) -> Theme {
        Theme::ALL
            .iter()
            .find(|t| t.id == id)
            .map(|t| (*t).clone())
            .unwrap_or(Theme::DARK)
    }

    /// 配置文件显式指定 foreground/background 时覆盖
    pub fn overriding(mut self, fg: Option<Rgb>, bg: Option<Rgb>) -> Theme {
        if let Some(fg) = fg {
            self.foreground = fg;
        }
        if let Some(bg) = bg {
            self.background = bg;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex() {
        assert_eq!(Rgb::parse("#5CC8FF"), Some(Rgb(0x5c, 0xc8, 0xff)));
        assert_eq!(Rgb::parse("ffffff"), Some(Rgb(255, 255, 255)));
        assert_eq!(Rgb::parse("#fff"), None);
        assert_eq!(Rgb::parse("zzzzzz"), None);
        assert_eq!(Rgb(0x5c, 0xc8, 0xff).to_hex(), "#5CC8FF");
    }

    #[test]
    fn unknown_theme_falls_back_to_dark() {
        assert_eq!(Theme::named("nope").id, "dark");
        assert_eq!(Theme::named("dracula").id, "dracula");
    }
}
