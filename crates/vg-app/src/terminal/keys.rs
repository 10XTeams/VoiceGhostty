//! egui 按键 → 发给 PTY 的字节(xterm 风格)。可打印字符走 `Event::Text`,不在这里处理。

use egui::{Key, Modifiers};

/// xterm 修饰键参数:1 + shift(1) + alt(2) + ctrl(4)
fn modifier_param(m: Modifiers) -> u8 {
    1 + m.shift as u8 + (m.alt as u8) * 2 + (m.ctrl as u8) * 4
}

/// `app_cursor`:终端处于应用光标模式(DECCKM,vim/less 等会开)
pub fn encode(key: Key, m: Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    // ⌘← / ⌘→:光标跳到本行开头 / 结尾。按 iTerm2 的惯例发 Ctrl-A / Ctrl-E,
    // zsh(默认不绑 Home/End)、bash、readline、各类 TUI 都认
    if m.mac_cmd && !m.alt && !m.ctrl && !m.shift {
        match key {
            Key::ArrowLeft => return Some(vec![0x01]),
            Key::ArrowRight => return Some(vec![0x05]),
            _ => {}
        }
    }
    // macOS 上其余 ⌘ 组合键是应用快捷键,不发给终端
    if m.mac_cmd {
        return None;
    }
    let param = modifier_param(m);

    // 方向键 / Home / End:无修饰时区分应用光标模式,有修饰时用 CSI 1;m X
    let cursor = |c: char| -> Vec<u8> {
        if param > 1 {
            format!("\x1b[1;{param}{c}").into_bytes()
        } else if app_cursor {
            format!("\x1bO{c}").into_bytes()
        } else {
            format!("\x1b[{c}").into_bytes()
        }
    };
    // PageUp/Delete 等:CSI n ~ 或 CSI n;m ~
    let tilde = |n: u8| -> Vec<u8> {
        if param > 1 {
            format!("\x1b[{n};{param}~").into_bytes()
        } else {
            format!("\x1b[{n}~").into_bytes()
        }
    };
    let ss3 = |c: char| -> Vec<u8> {
        if param > 1 {
            format!("\x1b[1;{param}{c}").into_bytes()
        } else {
            format!("\x1bO{c}").into_bytes()
        }
    };

    let bytes = match key {
        Key::Enter => {
            if m.alt { b"\x1b\r".to_vec() } else { b"\r".to_vec() }
        }
        Key::Backspace => {
            if m.ctrl { vec![0x08] } else if m.alt { b"\x1b\x7f".to_vec() } else { vec![0x7f] }
        }
        Key::Tab => {
            if m.shift { b"\x1b[Z".to_vec() } else { b"\t".to_vec() }
        }
        Key::Escape => vec![0x1b],
        Key::ArrowUp => cursor('A'),
        Key::ArrowDown => cursor('B'),
        Key::ArrowRight => cursor('C'),
        Key::ArrowLeft => cursor('D'),
        Key::Home => cursor('H'),
        Key::End => cursor('F'),
        Key::Insert => tilde(2),
        Key::Delete => tilde(3),
        Key::PageUp => tilde(5),
        Key::PageDown => tilde(6),
        Key::F1 => ss3('P'),
        Key::F2 => ss3('Q'),
        Key::F3 => ss3('R'),
        Key::F4 => ss3('S'),
        Key::F5 => tilde(15),
        Key::F6 => tilde(17),
        Key::F7 => tilde(18),
        Key::F8 => tilde(19),
        Key::F9 => tilde(20),
        Key::F10 => tilde(21),
        Key::F11 => tilde(23),
        Key::F12 => tilde(24),
        Key::Space if m.ctrl => vec![0x00],
        _ if m.ctrl => ctrl_byte(key)?,
        // 非 macOS:Alt+字母 作为 Meta(ESC 前缀)。macOS 的 Option 组合交给系统产生字符
        _ if m.alt && !cfg!(target_os = "macos") => {
            let c = letter(key)?;
            vec![0x1b, if m.shift { c.to_ascii_uppercase() } else { c }]
        }
        _ => return None,
    };
    Some(bytes)
}

fn letter(key: Key) -> Option<u8> {
    let name = key.name();
    (name.len() == 1 && name.as_bytes()[0].is_ascii_alphabetic())
        .then(|| name.as_bytes()[0].to_ascii_lowercase())
}

/// Ctrl+字母 → 0x01..0x1A;以及几个传统控制符
fn ctrl_byte(key: Key) -> Option<Vec<u8>> {
    if let Some(c) = letter(key) {
        return Some(vec![c - b'a' + 1]);
    }
    let b = match key {
        Key::OpenBracket => 0x1b,
        Key::Backslash => 0x1c,
        Key::CloseBracket => 0x1d,
        Key::Slash | Key::Minus => 0x1f,
        Key::Num2 => 0x00,
        _ => return None,
    };
    Some(vec![b])
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers::NONE;
    const CTRL: Modifiers = Modifiers::CTRL;

    #[test]
    fn basic_keys() {
        assert_eq!(encode(Key::Enter, NONE, false).unwrap(), b"\r");
        assert_eq!(encode(Key::Backspace, NONE, false).unwrap(), b"\x7f");
        assert_eq!(encode(Key::Delete, NONE, false).unwrap(), b"\x1b[3~");
    }

    #[test]
    fn arrows_follow_cursor_mode() {
        assert_eq!(encode(Key::ArrowUp, NONE, false).unwrap(), b"\x1b[A");
        assert_eq!(encode(Key::ArrowUp, NONE, true).unwrap(), b"\x1bOA");
        assert_eq!(encode(Key::ArrowLeft, Modifiers::SHIFT, true).unwrap(), b"\x1b[1;2D");
    }

    #[test]
    fn ctrl_letters() {
        assert_eq!(encode(Key::C, CTRL, false).unwrap(), vec![3]);
        assert_eq!(encode(Key::A, CTRL, false).unwrap(), vec![1]);
        assert_eq!(encode(Key::Space, CTRL, false).unwrap(), vec![0]);
        assert_eq!(encode(Key::OpenBracket, CTRL, false).unwrap(), vec![0x1b]);
    }

    #[test]
    fn cmd_arrows_jump_to_line_edges() {
        assert_eq!(encode(Key::ArrowLeft, Modifiers::MAC_CMD, false).unwrap(), vec![0x01]);
        assert_eq!(encode(Key::ArrowRight, Modifiers::MAC_CMD, true).unwrap(), vec![0x05]);
        // 带其他修饰键的 ⌘ 组合仍是应用快捷键
        let cmd_alt = Modifiers { alt: true, ..Modifiers::MAC_CMD };
        assert_eq!(encode(Key::ArrowLeft, cmd_alt, false), None);
        assert_eq!(encode(Key::A, Modifiers::MAC_CMD, false), None);
    }

    #[test]
    fn plain_letters_go_through_text_events() {
        assert_eq!(encode(Key::A, NONE, false), None);
    }
}
