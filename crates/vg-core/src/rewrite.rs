//! 转写结果的文本改写:个人词典替换,以及听写模式下把口述的符号名换成符号。
//!
//! - **个人词典**(`voice-replace = 听成的词 => 想要的词`,两种模式都生效):Whisper 老把项目名、工具名听成
//!   同音别字,一张用户自己维护的表最直接。长模式优先,原样匹配(不做大小写折叠,避免误伤)。
//! - **口述符号**(`voice-spoken-symbols`,只在听写模式):口述 shell 命令时「cargo 点 toml」「横杠 v」
//!   「src 斜杠 main」应该得到 `cargo.toml` `-v` `src/main`。但「点」「杠」这类字在中文句子里太常见
//!   (「帮我点一下」),所以只有符号词**紧挨着**(忽略空格)ASCII 字母数字、另一个符号、或另一个符号词时
//!   才转换;两边都是汉字就原样保留。每种符号自带粘合规则:`.` `_` `/` 两边都不留空格,`-` `~` 只吃掉
//!   后面的空格(`ls -l`),`,` `;` 只吃掉前面的,`|` `>` 两边都保留。

/// 符号两侧空格怎么处理
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Glue {
    /// 两边空格都去掉:`.` `_` `/` `:` `=`
    Both,
    /// 只去掉后面的:`-` `~` `$` `#` `*` `(`
    After,
    /// 只去掉前面的:`,` `;` `?` `!` `)`
    Before,
    /// 两边都保留:`|` `>` `<` `&`
    Keep,
}

/// (口述词, 符号, 粘合规则)。同一符号的多种说法分开列;匹配时长词优先,所以「双横杠」先于「横杠」
const SPOKEN_SYMBOLS: &[(&str, &str, Glue)] = &[
    // 中文
    ("双横杠", "--", Glue::After),
    ("双横线", "--", Glue::After),
    ("双减号", "--", Glue::After),
    ("横杠", "-", Glue::After),
    ("横线", "-", Glue::After),
    ("减号", "-", Glue::After),
    ("中划线", "-", Glue::After),
    ("连字符", "-", Glue::After),
    ("下划线", "_", Glue::Both),
    ("下横线", "_", Glue::Both),
    ("反斜杠", "\\", Glue::Both),
    ("反斜线", "\\", Glue::Both),
    ("斜杠", "/", Glue::Both),
    ("斜线", "/", Glue::Both),
    ("点点", "..", Glue::After),
    ("点", ".", Glue::Both),
    ("句点", ".", Glue::Both),
    ("冒号", ":", Glue::Both),
    ("分号", ";", Glue::Before),
    ("逗号", ",", Glue::Before),
    ("问号", "?", Glue::Before),
    ("感叹号", "!", Glue::Before),
    ("叹号", "!", Glue::Before),
    ("双引号", "\"", Glue::Keep),
    ("单引号", "'", Glue::Keep),
    ("引号", "\"", Glue::Keep),
    ("反引号", "`", Glue::Keep),
    ("等号", "=", Glue::Both),
    ("等于号", "=", Glue::Both),
    ("加号", "+", Glue::Both),
    ("星号", "*", Glue::After),
    ("井号", "#", Glue::After),
    ("美元符号", "$", Glue::After),
    ("美元符", "$", Glue::After),
    ("波浪号", "~", Glue::After),
    ("波浪线", "~", Glue::After),
    ("竖线", "|", Glue::Keep),
    ("竖杠", "|", Glue::Keep),
    ("管道符", "|", Glue::Keep),
    ("大于号", ">", Glue::Keep),
    ("小于号", "<", Glue::Keep),
    ("和号", "&", Glue::Keep),
    ("与号", "&", Glue::Keep),
    ("百分号", "%", Glue::Both),
    ("脱字符", "^", Glue::Both),
    ("艾特", "@", Glue::Both),
    ("左圆括号", "(", Glue::After),
    ("右圆括号", ")", Glue::Before),
    ("左括号", "(", Glue::After),
    ("右括号", ")", Glue::Before),
    ("左方括号", "[", Glue::After),
    ("右方括号", "]", Glue::Before),
    ("左中括号", "[", Glue::After),
    ("右中括号", "]", Glue::Before),
    ("左花括号", "{", Glue::After),
    ("右花括号", "}", Glue::Before),
    ("左大括号", "{", Glue::After),
    ("右大括号", "}", Glue::Before),
    // 英文(按词匹配:前后不能是字母数字,所以 dotfiles 不会被碰)
    ("double dash", "--", Glue::After),
    ("dash dash", "--", Glue::After),
    ("dash", "-", Glue::After),
    ("hyphen", "-", Glue::After),
    ("minus", "-", Glue::After),
    ("underscore", "_", Glue::Both),
    ("backslash", "\\", Glue::Both),
    ("slash", "/", Glue::Both),
    ("dot dot", "..", Glue::After),
    ("dot", ".", Glue::Both),
    ("period", ".", Glue::Both),
    ("colon", ":", Glue::Both),
    ("semicolon", ";", Glue::Before),
    ("comma", ",", Glue::Before),
    ("question mark", "?", Glue::Before),
    ("exclamation mark", "!", Glue::Before),
    ("double quote", "\"", Glue::Keep),
    ("single quote", "'", Glue::Keep),
    ("backtick", "`", Glue::Keep),
    ("equals", "=", Glue::Both),
    ("plus", "+", Glue::Both),
    ("asterisk", "*", Glue::After),
    ("star", "*", Glue::After),
    ("hash", "#", Glue::After),
    ("dollar", "$", Glue::After),
    ("tilde", "~", Glue::After),
    ("pipe", "|", Glue::Keep),
    ("greater than", ">", Glue::Keep),
    ("less than", "<", Glue::Keep),
    ("ampersand", "&", Glue::Keep),
    ("percent", "%", Glue::Both),
    ("caret", "^", Glue::Both),
    ("at sign", "@", Glue::Both),
    ("open paren", "(", Glue::After),
    ("close paren", ")", Glue::Before),
    ("open bracket", "[", Glue::After),
    ("close bracket", "]", Glue::Before),
    ("open brace", "{", Glue::After),
    ("close brace", "}", Glue::Before),
];

/// 个人词典:`(听成的, 想要的)`。长的先匹配,原样替换;空模式忽略
pub fn apply_dictionary(text: &str, rules: &[(String, String)]) -> String {
    let mut rules: Vec<&(String, String)> = rules.iter().filter(|(from, _)| !from.trim().is_empty()).collect();
    rules.sort_by_key(|(from, _)| std::cmp::Reverse(from.chars().count()));
    let mut out = text.to_string();
    for (from, to) in rules {
        out = out.replace(from.trim(), to.trim());
    }
    out
}

/// 一行配置 `听成的 => 想要的`(也接受 `->`、`=`);没有分隔符返回 None
pub fn parse_rule(line: &str) -> Option<(String, String)> {
    for sep in ["=>", "->", "="] {
        if let Some((from, to)) = line.split_once(sep) {
            let from = from.trim();
            if from.is_empty() {
                return None;
            }
            return Some((from.to_string(), to.trim().to_string()));
        }
    }
    None
}

/// 听写模式:把口述的符号名换成符号(见模块说明里的相邻规则与粘合规则)
pub fn spoken_symbols(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        match match_symbol(&chars, i) {
            Some((len, symbol, glue)) if should_convert(&out, &chars, i, len) => {
                if matches!(glue, Glue::Both | Glue::Before) {
                    let trimmed = out.trim_end().len();
                    out.truncate(trimmed);
                }
                out.push_str(symbol);
                i += len;
                if matches!(glue, Glue::Both | Glue::After) {
                    while i < chars.len() && chars[i] == ' ' {
                        i += 1;
                    }
                }
            }
            _ => {
                out.push(chars[i]);
                i += 1;
            }
        }
    }
    out
}

/// 在 `at` 处是否以某个符号词开头;返回 (词长, 符号, 粘合)。英文词要求前后都不是字母数字
fn match_symbol(chars: &[char], at: usize) -> Option<(usize, &'static str, Glue)> {
    let mut best: Option<(usize, &'static str, Glue)> = None;
    for (word, symbol, glue) in SPOKEN_SYMBOLS {
        let w: Vec<char> = word.chars().collect();
        if at + w.len() > chars.len() {
            continue;
        }
        let ascii = w[0].is_ascii_alphabetic();
        let matches = chars[at..at + w.len()]
            .iter()
            .zip(&w)
            .all(|(a, b)| if ascii { a.to_ascii_lowercase() == *b } else { a == b });
        if !matches {
            continue;
        }
        if ascii {
            let before_ok = at == 0 || !chars[at - 1].is_ascii_alphanumeric();
            let after_ok = at + w.len() == chars.len() || !chars[at + w.len()].is_ascii_alphanumeric();
            if !before_ok || !after_ok {
                continue;
            }
        }
        if best.is_none_or(|(len, _, _)| w.len() > len) {
            best = Some((w.len(), symbol, *glue));
        }
    }
    best
}

/// 相邻规则:符号词左边(已输出部分的最后一个非空格字符)或右边(跳过空格后)得是 ASCII 字母数字、
/// 一个符号字符、或另一个符号词
fn should_convert(out: &str, chars: &[char], at: usize, len: usize) -> bool {
    let left = out.trim_end().chars().last();
    let left_ok = left.is_some_and(|c| c.is_ascii_alphanumeric() || is_symbol_char(c));
    let mut j = at + len;
    while j < chars.len() && chars[j] == ' ' {
        j += 1;
    }
    let right_ok = j < chars.len()
        && (chars[j].is_ascii_alphanumeric() || is_symbol_char(chars[j]) || match_symbol(chars, j).is_some());
    left_ok || right_ok
}

fn is_symbol_char(c: char) -> bool {
    c.is_ascii_punctuation()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_file_names_and_flags() {
        assert_eq!(spoken_symbols("cargo 点 toml"), "cargo.toml");
        assert_eq!(spoken_symbols("main点rs"), "main.rs");
        assert_eq!(spoken_symbols("ls 横杠 l"), "ls -l");
        assert_eq!(spoken_symbols("git push 双横杠 force"), "git push --force");
        assert_eq!(spoken_symbols("src 斜杠 main 点 rs"), "src/main.rs");
        assert_eq!(spoken_symbols("cd 点点 斜杠 docs"), "cd ../docs");
        assert_eq!(spoken_symbols("my 下划线 var 等号 1"), "my_var=1");
        assert_eq!(spoken_symbols("localhost 冒号 8080"), "localhost:8080");
        assert_eq!(spoken_symbols("ls 竖线 grep rs"), "ls | grep rs");
        assert_eq!(spoken_symbols("cd 波浪号 斜杠 code"), "cd ~/code");
        assert_eq!(spoken_symbols("rm 星号 点 log"), "rm *.log");
    }

    #[test]
    fn leaves_chinese_prose_alone() {
        assert_eq!(spoken_symbols("帮我点一下这个"), "帮我点一下这个");
        assert_eq!(spoken_symbols("今天有点冷"), "今天有点冷");
        assert_eq!(spoken_symbols("画一条横线"), "画一条横线");
        assert_eq!(spoken_symbols("列出当前目录"), "列出当前目录");
    }

    #[test]
    fn english_words_need_boundaries() {
        assert_eq!(spoken_symbols("ls dash la"), "ls -la");
        assert_eq!(spoken_symbols("main dot rs"), "main.rs");
        assert_eq!(spoken_symbols("show my dotfiles"), "show my dotfiles");
        assert_eq!(spoken_symbols("Dot Dot slash src"), "../src");
        assert_eq!(spoken_symbols("npm run double dash fix"), "npm run --fix");
    }

    #[test]
    fn dictionary_longest_first_and_trims() {
        let rules = vec![
            ("语音 ghostty".to_string(), "VoiceGhostty".to_string()),
            ("ghostty".to_string(), "Ghostty".to_string()),
            ("  ".to_string(), "x".to_string()),
        ];
        assert_eq!(apply_dictionary("打开 语音 ghostty 的仓库", &rules), "打开 VoiceGhostty 的仓库");
        assert_eq!(apply_dictionary("ghostty 1.3", &rules), "Ghostty 1.3");
    }

    #[test]
    fn parses_rules() {
        assert_eq!(parse_rule("卡狗 => cargo"), Some(("卡狗".into(), "cargo".into())));
        assert_eq!(parse_rule("foo -> bar"), Some(("foo".into(), "bar".into())));
        assert_eq!(parse_rule("a = "), Some(("a".into(), "".into())));
        assert_eq!(parse_rule("=> x"), None);
        assert_eq!(parse_rule("nothing"), None);
    }
}
