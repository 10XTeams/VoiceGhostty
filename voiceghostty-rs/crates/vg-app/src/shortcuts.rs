//! 快捷键表:一个动作一条定义,macOS 与 Linux/Windows 各一套。按键处理和帮助窗口都读这张表,不会对不上。
//! macOS 沿用 Swift 版(⌘ 系);其他平台单 Ctrl 组合要留给终端(Ctrl+T/W/D 都是 shell 里常用的),
//! 所以基本用 Ctrl+Shift,分屏移动这类再加 Alt。

use egui::{Key, KeyboardShortcut, Modifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    NewTab,
    Close,
    SplitRight,
    SplitDown,
    NextTab,
    PrevTab,
    Tab(u8), // 1…8
    LastTab,
    FocusLeft,
    FocusRight,
    FocusUp,
    FocusDown,
    MoveToTab(u8), // 1…8
    MoveToNewTab,
    MoveToPrevTab,
    MoveToNextTab,
    FontBigger,
    FontSmaller,
    FontReset,
    ClearScreen,
    Find,
    SelectAll,
    ToggleRecording,
    ToggleMode,
    Loader,
    Settings,
    Help,
}

const MAC: bool = cfg!(target_os = "macos");

fn m(mods: Modifiers, key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(mods, key)
}

const CMD: Modifiers = Modifiers::MAC_CMD;
const CMD_SHIFT: Modifiers = Modifiers { shift: true, ..Modifiers::MAC_CMD };
const CMD_ALT: Modifiers = Modifiers { alt: true, ..Modifiers::MAC_CMD };
const CMD_CTRL: Modifiers = Modifiers { ctrl: true, ..Modifiers::MAC_CMD };
const CS: Modifiers = Modifiers { ctrl: true, shift: true, ..Modifiers::NONE };
const CA: Modifiers = Modifiers { ctrl: true, alt: true, ..Modifiers::NONE };
const CAS: Modifiers = Modifiers { ctrl: true, alt: true, shift: true, ..Modifiers::NONE };
const ALT: Modifiers = Modifiers::ALT;

const DIGITS: [Key; 9] = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];

impl Action {
    /// 该动作的快捷键(可能有备选,比如 ⌘+ 与 ⌘=)
    pub fn shortcuts(self) -> Vec<KeyboardShortcut> {
        use Action::*;
        let pick = |mac: KeyboardShortcut, other: KeyboardShortcut| vec![if MAC { mac } else { other }];
        match self {
            NewTab => pick(m(CMD, Key::T), m(CS, Key::T)),
            Close => pick(m(CMD, Key::W), m(CS, Key::W)),
            SplitRight => pick(m(CMD, Key::D), m(CS, Key::D)),
            SplitDown => pick(m(CMD_SHIFT, Key::D), m(CS, Key::E)),
            // ⇧] 在美式键盘上是 },两种写法都认
            NextTab => {
                if MAC {
                    vec![m(CMD_SHIFT, Key::CloseBracket), m(CMD_SHIFT, Key::CloseCurlyBracket)]
                } else {
                    vec![m(Modifiers::CTRL, Key::PageDown)]
                }
            }
            PrevTab => {
                if MAC {
                    vec![m(CMD_SHIFT, Key::OpenBracket), m(CMD_SHIFT, Key::OpenCurlyBracket)]
                } else {
                    vec![m(Modifiers::CTRL, Key::PageUp)]
                }
            }
            Tab(n) => pick(m(CMD, DIGITS[n as usize - 1]), m(ALT, DIGITS[n as usize - 1])),
            LastTab => pick(m(CMD, Key::Num9), m(ALT, Key::Num9)),
            FocusLeft => pick(m(CMD_ALT, Key::ArrowLeft), m(CA, Key::ArrowLeft)),
            FocusRight => pick(m(CMD_ALT, Key::ArrowRight), m(CA, Key::ArrowRight)),
            FocusUp => pick(m(CMD_ALT, Key::ArrowUp), m(CA, Key::ArrowUp)),
            FocusDown => pick(m(CMD_ALT, Key::ArrowDown), m(CA, Key::ArrowDown)),
            MoveToTab(n) => pick(m(CMD_CTRL, DIGITS[n as usize - 1]), m(CA, DIGITS[n as usize - 1])),
            MoveToNewTab => pick(m(CMD_CTRL, Key::T), m(CAS, Key::T)),
            MoveToPrevTab => pick(m(CMD_CTRL, Key::ArrowLeft), m(CAS, Key::ArrowLeft)),
            MoveToNextTab => pick(m(CMD_CTRL, Key::ArrowRight), m(CAS, Key::ArrowRight)),
            FontBigger => {
                if MAC {
                    vec![m(CMD, Key::Plus), m(CMD, Key::Equals)]
                } else {
                    vec![m(CS, Key::Plus), m(CS, Key::Equals)]
                }
            }
            FontSmaller => pick(m(CMD, Key::Minus), m(CS, Key::Minus)),
            FontReset => pick(m(CMD, Key::Num0), m(CS, Key::Num0)),
            ClearScreen => pick(m(CMD, Key::K), m(CS, Key::K)),
            Find => pick(m(CMD, Key::F), m(CS, Key::F)),
            SelectAll => pick(m(CMD, Key::A), m(CS, Key::A)),
            ToggleRecording => pick(m(CMD_SHIFT, Key::M), m(CS, Key::M)),
            ToggleMode => pick(m(CMD_SHIFT, Key::N), m(CS, Key::N)),
            Loader => pick(m(CMD, Key::L), m(CS, Key::L)),
            Settings => pick(m(CMD, Key::Comma), m(CS, Key::Comma)),
            Help => pick(m(CMD, Key::Slash), m(CS, Key::Slash)),
        }
    }

    /// 本帧按下了就消费掉(不再传给终端)
    pub fn consume(self, ctx: &egui::Context) -> bool {
        self.shortcuts().iter().any(|s| ctx.input_mut(|i| i.consume_shortcut(s)))
    }

    pub fn label(self) -> String {
        let s = self.shortcuts()[0];
        if MAC {
            s.format(&egui::ModifierNames::SYMBOLS, true)
        } else {
            s.format(&egui::ModifierNames::NAMES, false)
        }
    }
}

/// 帮助窗口的分组,按 Swift 版的结构
pub fn help_sections() -> Vec<(&'static str, Vec<(String, &'static str)>)> {
    use Action::*;
    let row = |a: Action, desc: &'static str| (a.label(), desc);
    let tab_n = if MAC { "⌘1…⌘8" } else { "Alt+1…8" };
    let move_n = if MAC { "⌘⌃1…⌘⌃8" } else { "Ctrl+Alt+1…8" };
    vec![
        (
            "语音",
            vec![
                ("按住空格".into(), "说话(松开结束;快速点按仍是空格)"),
                row(ToggleRecording, "开始 / 停止录音"),
                row(ToggleMode, "切换听写 / 自然语言"),
            ],
        ),
        (
            "标签页",
            vec![
                row(NewTab, "新建标签页"),
                row(Close, "关闭分屏 / 标签页"),
                (format!("{} / {}", NextTab.label(), PrevTab.label()), "下一个 / 上一个标签页"),
                (tab_n.into(), "跳到第 N 个标签页"),
                row(LastTab, "最后一个标签页"),
                ("双击标签".into(), "改名(清空恢复自动命名)"),
            ],
        ),
        (
            "分屏",
            vec![
                row(SplitRight, "向右分屏"),
                row(SplitDown, "向下分屏"),
                (format!("{} 等方向键", FocusLeft.label()), "按方向切换分屏焦点"),
                (move_n.into(), "把当前分屏移到第 N 个标签页(shell 与历史一起走)"),
                (format!("{} / {}", MoveToPrevTab.label(), MoveToNextTab.label()), "移到上一个 / 下一个标签页"),
                row(MoveToNewTab, "把当前分屏移出成新标签页"),
                ("点击某个分屏".into(), "语音与输入发往该分屏"),
            ],
        ),
        (
            "视图",
            vec![
                (format!("{} / {}", FontBigger.label(), FontSmaller.label()), "增大 / 减小字号"),
                row(FontReset, "重置字号"),
                row(ClearScreen, "清屏"),
                row(Find, "搜索历史(回车上一个更早的,⇧回车下一个)"),
            ],
        ),
        (
            "其他",
            vec![
                row(SelectAll, "全选"),
                row(Loader, "🧩 加载器"),
                row(Settings, "设置"),
                row(Help, "快捷键帮助"),
            ],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_two_actions_share_a_shortcut() {
        use Action::*;
        let mut all: Vec<Action> = vec![
            NewTab, Close, SplitRight, SplitDown, NextTab, PrevTab, LastTab, FocusLeft, FocusRight, FocusUp,
            FocusDown, MoveToNewTab, MoveToPrevTab, MoveToNextTab, FontBigger, FontSmaller, FontReset,
            ClearScreen, Find, SelectAll, ToggleRecording, ToggleMode, Loader, Settings, Help,
        ];
        all.extend((1..=8).map(Tab));
        all.extend((1..=8).map(MoveToTab));
        let mut seen = std::collections::HashMap::new();
        for a in all {
            for s in a.shortcuts() {
                let key = (s.logical_key, s.modifiers.alt, s.modifiers.ctrl, s.modifiers.shift, s.modifiers.mac_cmd);
                if let Some(prev) = seen.insert(key, a) {
                    panic!("{a:?} and {prev:?} share {:?}", s);
                }
            }
        }
    }

    #[test]
    fn labels_render() {
        assert!(!Action::SplitDown.label().is_empty());
        assert!(!help_sections().is_empty());
    }
}
