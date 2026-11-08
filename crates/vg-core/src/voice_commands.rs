//! 语音动作命令:整句恰好是「清屏」「新标签」「切到第二个标签」这类话时,直接执行动作而不是把字敲进终端。
//!
//! 只做**整句精确匹配**(去掉标点、空格、大小写之后与词表比),绝不在句子里找关键词:
//! 「帮我清屏之前先保存」是一句要敲进去的话,不是命令。词表故意很短,全是终端自己的动作;
//! 真正要 shell 做事的话走自然语言模式。两种语音模式都先过这一层(`voice-commands = off` 可关)。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceCommand {
    ClearScreen,
    NewTab,
    /// 关闭当前分屏;没有分屏则关闭标签页(与 ⌘W 一致)
    CloseActive,
    NextTab,
    PrevTab,
    /// 1 起
    Tab(u8),
    LastTab,
    SplitRight,
    SplitDown,
    Find,
    Dictation,
    NaturalLanguage,
    /// 撤销上一次语音上屏的文本
    UndoVoice,
}

impl VoiceCommand {
    /// 状态栏里显示的动作名
    pub fn label(self) -> String {
        match self {
            VoiceCommand::ClearScreen => "清屏".into(),
            VoiceCommand::NewTab => "新建标签页".into(),
            VoiceCommand::CloseActive => "关闭分屏 / 标签页".into(),
            VoiceCommand::NextTab => "下一个标签页".into(),
            VoiceCommand::PrevTab => "上一个标签页".into(),
            VoiceCommand::Tab(n) => format!("第 {n} 个标签页"),
            VoiceCommand::LastTab => "最后一个标签页".into(),
            VoiceCommand::SplitRight => "向右分屏".into(),
            VoiceCommand::SplitDown => "向下分屏".into(),
            VoiceCommand::Find => "搜索".into(),
            VoiceCommand::Dictation => "听写模式".into(),
            VoiceCommand::NaturalLanguage => "自然语言模式".into(),
            VoiceCommand::UndoVoice => "撤销上次语音输入".into(),
        }
    }

    /// 帮助窗口里列的说法:(怎么说, 做什么)
    pub fn help() -> Vec<(&'static str, &'static str)> {
        vec![
            ("清屏", "清屏"),
            ("新标签 / 新建标签页", "新建标签页"),
            ("关闭标签 / 关闭分屏", "关闭当前分屏或标签页"),
            ("下一个标签 / 上一个标签", "切换标签页"),
            ("第二个标签 / 切到第三个标签", "跳到第 N 个标签页"),
            ("最后一个标签", "最后一个标签页"),
            ("向右分屏 / 向下分屏", "分屏"),
            ("搜索", "打开历史搜索"),
            ("听写模式 / 自然语言模式", "切换语音模式"),
            ("撤销", "撤销上一次语音上屏"),
        ]
    }
}

/// 固定说法(已归一化:无标点、无空格、小写)
const FIXED: &[(&str, VoiceCommand)] = &[
    ("清屏", VoiceCommand::ClearScreen),
    ("清除屏幕", VoiceCommand::ClearScreen),
    ("清空屏幕", VoiceCommand::ClearScreen),
    ("clearscreen", VoiceCommand::ClearScreen),
    ("clearthescreen", VoiceCommand::ClearScreen),
    ("新标签", VoiceCommand::NewTab),
    ("新标签页", VoiceCommand::NewTab),
    ("新建标签", VoiceCommand::NewTab),
    ("新建标签页", VoiceCommand::NewTab),
    ("打开新标签", VoiceCommand::NewTab),
    ("打开新标签页", VoiceCommand::NewTab),
    ("新开一个标签", VoiceCommand::NewTab),
    ("新开一个标签页", VoiceCommand::NewTab),
    ("newtab", VoiceCommand::NewTab),
    ("opennewtab", VoiceCommand::NewTab),
    ("关闭标签", VoiceCommand::CloseActive),
    ("关闭标签页", VoiceCommand::CloseActive),
    ("关闭当前标签", VoiceCommand::CloseActive),
    ("关闭当前标签页", VoiceCommand::CloseActive),
    ("关闭分屏", VoiceCommand::CloseActive),
    ("关闭当前分屏", VoiceCommand::CloseActive),
    ("关掉这个分屏", VoiceCommand::CloseActive),
    ("closetab", VoiceCommand::CloseActive),
    ("closepane", VoiceCommand::CloseActive),
    ("closesplit", VoiceCommand::CloseActive),
    ("下一个标签", VoiceCommand::NextTab),
    ("下一个标签页", VoiceCommand::NextTab),
    ("下个标签", VoiceCommand::NextTab),
    ("下个标签页", VoiceCommand::NextTab),
    ("nexttab", VoiceCommand::NextTab),
    ("上一个标签", VoiceCommand::PrevTab),
    ("上一个标签页", VoiceCommand::PrevTab),
    ("上个标签", VoiceCommand::PrevTab),
    ("上个标签页", VoiceCommand::PrevTab),
    ("previoustab", VoiceCommand::PrevTab),
    ("prevtab", VoiceCommand::PrevTab),
    ("最后一个标签", VoiceCommand::LastTab),
    ("最后一个标签页", VoiceCommand::LastTab),
    ("最后的标签", VoiceCommand::LastTab),
    ("lasttab", VoiceCommand::LastTab),
    ("向右分屏", VoiceCommand::SplitRight),
    ("左右分屏", VoiceCommand::SplitRight),
    ("右边分屏", VoiceCommand::SplitRight),
    ("分屏", VoiceCommand::SplitRight),
    ("splitright", VoiceCommand::SplitRight),
    ("split", VoiceCommand::SplitRight),
    ("向下分屏", VoiceCommand::SplitDown),
    ("上下分屏", VoiceCommand::SplitDown),
    ("下面分屏", VoiceCommand::SplitDown),
    ("splitdown", VoiceCommand::SplitDown),
    ("搜索", VoiceCommand::Find),
    ("查找", VoiceCommand::Find),
    ("搜索历史", VoiceCommand::Find),
    ("打开搜索", VoiceCommand::Find),
    ("find", VoiceCommand::Find),
    ("search", VoiceCommand::Find),
    ("听写模式", VoiceCommand::Dictation),
    ("切换到听写模式", VoiceCommand::Dictation),
    ("切到听写模式", VoiceCommand::Dictation),
    ("听写", VoiceCommand::Dictation),
    ("dictationmode", VoiceCommand::Dictation),
    ("自然语言模式", VoiceCommand::NaturalLanguage),
    ("切换到自然语言模式", VoiceCommand::NaturalLanguage),
    ("切到自然语言模式", VoiceCommand::NaturalLanguage),
    ("自然语言", VoiceCommand::NaturalLanguage),
    ("naturallanguagemode", VoiceCommand::NaturalLanguage),
    ("撤销", VoiceCommand::UndoVoice),
    ("撤回", VoiceCommand::UndoVoice),
    ("撤销刚才的", VoiceCommand::UndoVoice),
    ("撤销上一次", VoiceCommand::UndoVoice),
    ("undo", VoiceCommand::UndoVoice),
    ("undothat", VoiceCommand::UndoVoice),
];

/// 整句是不是一条动作命令
pub fn parse(text: &str) -> Option<VoiceCommand> {
    let norm = normalize(text);
    if norm.is_empty() {
        return None;
    }
    if let Some((_, cmd)) = FIXED.iter().find(|(k, _)| *k == norm) {
        return Some(*cmd);
    }
    tab_number(&norm).map(VoiceCommand::Tab)
}

/// 「第N个标签(页)」「切(换)到第N个标签(页)」「跳到第N个标签」「标签N」「tabN」「gototabN」
fn tab_number(norm: &str) -> Option<u8> {
    for prefix in ["切换到第", "切到第", "跳到第", "跳转到第", "去第", "第", "切换到标签", "切到标签", "标签", "gototab", "switchtotab", "tab"] {
        let Some(rest) = norm.strip_prefix(prefix) else { continue };
        let rest = rest.trim_end_matches("标签页").trim_end_matches("标签").trim_end_matches("个");
        // 「第二个标签页」去掉后缀后剩「二」;「tab2」剩「2」
        let rest = rest.strip_suffix("个").unwrap_or(rest);
        if let Some(n) = number(rest) {
            if (1..=9).contains(&n) {
                return Some(n);
            }
        }
    }
    None
}

fn number(s: &str) -> Option<u8> {
    if let Ok(n) = s.parse::<u8>() {
        return Some(n);
    }
    Some(match s {
        "一" | "壹" | "one" | "first" => 1,
        "二" | "两" | "贰" | "two" | "second" => 2,
        "三" | "叁" | "three" | "third" => 3,
        "四" | "肆" | "four" | "fourth" => 4,
        "五" | "伍" | "five" | "fifth" => 5,
        "六" | "陆" | "six" | "sixth" => 6,
        "七" | "柒" | "seven" | "seventh" => 7,
        "八" | "捌" | "eight" | "eighth" => 8,
        "九" | "玖" | "nine" | "ninth" => 9,
        _ => return None,
    })
}

/// 去掉标点与空白,转小写;口述符号(「点」)不在这里处理
fn normalize(text: &str) -> String {
    text.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_whole_utterances_only() {
        assert_eq!(parse("清屏。"), Some(VoiceCommand::ClearScreen));
        assert_eq!(parse(" Clear screen "), Some(VoiceCommand::ClearScreen));
        assert_eq!(parse("新建标签页"), Some(VoiceCommand::NewTab));
        assert_eq!(parse("向下分屏"), Some(VoiceCommand::SplitDown));
        assert_eq!(parse("撤销"), Some(VoiceCommand::UndoVoice));
        // 句子里含关键词不算
        assert_eq!(parse("帮我清屏之前先保存"), None);
        assert_eq!(parse("echo 新标签"), None);
        assert_eq!(parse(""), None);
        assert_eq!(parse("列出文件"), None);
    }

    #[test]
    fn tab_numbers() {
        assert_eq!(parse("第二个标签页"), Some(VoiceCommand::Tab(2)));
        assert_eq!(parse("切到第三个标签"), Some(VoiceCommand::Tab(3)));
        assert_eq!(parse("切换到第 1 个标签页"), Some(VoiceCommand::Tab(1)));
        assert_eq!(parse("跳到第两个标签"), Some(VoiceCommand::Tab(2)));
        assert_eq!(parse("标签 4"), Some(VoiceCommand::Tab(4)));
        assert_eq!(parse("tab 5"), Some(VoiceCommand::Tab(5)));
        assert_eq!(parse("go to tab three"), Some(VoiceCommand::Tab(3)));
        assert_eq!(parse("第十个标签"), None);
        assert_eq!(parse("第二个文件"), None);
    }
}
