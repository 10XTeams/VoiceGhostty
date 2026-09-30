//! Shell 选择与 zsh 集成(ZDOTDIR 包装)。
//!
//! 对 zsh,写一个 ZDOTDIR 包装目录(`~/.config/voiceghostty/shell/`),其中的点文件先 source 用户真正的
//! `~/.z*`,再追加 VoiceGhostty 的集成。放在 `.zshrc` 里执行,意味着**首个提示符绘制前**就已生效,不闪。
//! - **输入高亮**:`line-pre-redraw` 钩子给正在输入的命令行上色,每次重绘用内建 `read` 读颜色文件(无子进程),
//!   所以设置里改色立即对所有标签页生效
//! - **命令边界**:`preexec`/`precmd` 把 `C <n>` / `D <n>` 写进 `$VG_STATE_FILE`(zsh 内建重定向,无子进程),
//!   App 轮询它驱动精确的状态灯。alacritty_terminal 会丢弃不认识的 OSC,所以没法像 Swift 版那样走 OSC 133;
//!   OSC 133 仍照发,换别的终端也能用
//!
//! 包装目录用 `shell-rs/` 而不是 Swift 版的 `shell/`:两个 App 每次开窗格都会重写自己的版本,共用会互相覆盖。

use std::path::{Path, PathBuf};

use crate::paths::config_dir;
use crate::settings::Settings;

/// 要启动的 shell 程序
pub fn default_shell() -> String {
    if cfg!(windows) {
        // pwsh(PowerShell 7)优先,没有再用系统自带的 powershell
        return if which("pwsh.exe") { "pwsh.exe".into() } else { "powershell.exe".into() };
    }
    std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/sh".into())
}

fn which(program: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
        .unwrap_or(false)
}

/// 给 shell 进程追加的环境变量(包括 zsh 的 ZDOTDIR 包装)
/// `state_file`:这个窗格的命令边界状态文件(见模块说明)
pub fn shell_environment(shell: &str, settings: &Settings, state_file: &Path) -> Vec<(String, String)> {
    let mut env = vec![
        ("TERM".to_string(), "xterm-256color".to_string()),
        ("COLORTERM".to_string(), "truecolor".to_string()),
        ("TERM_PROGRAM".to_string(), "VoiceGhostty".to_string()),
        ("VG_STATE_FILE".to_string(), state_file.to_string_lossy().into_owned()),
    ];
    let name = Path::new(shell).file_name().and_then(|s| s.to_str()).unwrap_or("");
    if name == "zsh" {
        if let Some(dir) = prepare_zdotdir(settings) {
            env.push(("ZDOTDIR".into(), dir.to_string_lossy().into_owned()));
        }
    }
    env
}

/// 写 ZDOTDIR 包装并返回其路径(失败返回 None,shell 仍能正常启动,只是没有集成)
pub fn prepare_zdotdir(settings: &Settings) -> Option<PathBuf> {
    // 钩子读颜色文件之前确保它存在
    settings.write_input_color_file().ok()?;
    let dir = config_dir().join("shell-rs");
    std::fs::create_dir_all(&dir).ok()?;
    let color_file = Settings::input_color_file();

    let files = zdotdir_files(&dir, &color_file);
    for (name, content) in files {
        std::fs::write(dir.join(name), content).ok()?;
    }
    Some(dir)
}

fn zdotdir_files(dir: &Path, color_file: &Path) -> [(&'static str, String); 4] {
    let dir = dir.display();
    let color = color_file.display();
    [
        (
            ".zshenv",
            format!("[ -f \"$HOME/.zshenv\" ] && source \"$HOME/.zshenv\"\nZDOTDIR='{dir}'\n"),
        ),
        (".zprofile", "[ -f \"$HOME/.zprofile\" ] && source \"$HOME/.zprofile\"\n".into()),
        (".zlogin", "[ -f \"$HOME/.zlogin\" ] && source \"$HOME/.zlogin\"\n".into()),
        (
            ".zshrc",
            format!(
                r#"[ -f "$HOME/.zshrc" ] && source "$HOME/.zshrc"
# --- VoiceGhostty shell integration (auto-generated; regenerated each launch) ---
export COLORTERM=truecolor
autoload -Uz add-zsh-hook add-zle-hook-widget
_vg_ic(){{ local c; IFS= read -r c < '{color}' 2>/dev/null; region_highlight=("0 ${{#BUFFER}} fg=${{c:-#5CC8FF}}") }}
add-zle-hook-widget line-pre-redraw _vg_ic
typeset -gi _vg_n=0
_vg_pe(){{ print -n '\e]133;C\e\\'; (( ++_vg_n )); [[ -n $VG_STATE_FILE ]] && print -r -- "C $_vg_n" >| $VG_STATE_FILE }}
_vg_pc(){{ print -n '\e]133;D\e\\'; print -n "\e]7;file://$HOST$PWD\e\\"; [[ -n $VG_STATE_FILE ]] && print -r -- "D $_vg_n" >| $VG_STATE_FILE }}
add-zsh-hook preexec _vg_pe
add-zsh-hook precmd _vg_pc
"#
            ),
        ),
    ]
}

/// 把路径安全地放进一条 shell 命令(空格、引号都不会把命令拆坏)
pub fn quote(path: &str) -> String {
    // POSIX shell 与 PowerShell 的单引号都是字面量;区别只在单引号本身怎么转义
    if cfg!(windows) {
        format!("'{}'", path.replace('\'', "''"))
    } else {
        format!("'{}'", path.replace('\'', r"'\''"))
    }
}

/// `cd <dir> && claude`:常用项目菜单的动作。这是用户明确点击发出的,所以带回车执行,
/// 与语音"永不自动回车"的安全规则不冲突
pub fn launch_claude_command(dir: &Path) -> String {
    format!("cd {} && claude\r", quote(&dir.to_string_lossy()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn quotes_paths() {
        assert_eq!(quote("/a b/it's"), r"'/a b/it'\''s'");
        assert_eq!(launch_claude_command(Path::new("/p q")), "cd '/p q' && claude\r");
    }

    #[test]
    fn zshrc_sources_user_rc_and_installs_hooks() {
        let files = zdotdir_files(Path::new("/x/shell"), Path::new("/x/input-color"));
        let zshrc = &files[3].1;
        assert!(zshrc.starts_with("[ -f \"$HOME/.zshrc\" ] && source"));
        assert!(zshrc.contains("read -r c < '/x/input-color'"));
        assert!(zshrc.contains("${#BUFFER}"));
        assert!(zshrc.contains(r"'\e]133;C\e\\'"));
        assert!(zshrc.contains(r#"print -r -- "C $_vg_n" >| $VG_STATE_FILE"#));
        assert!(zshrc.contains(r#"print -r -- "D $_vg_n" >| $VG_STATE_FILE"#));
        assert!(files[0].1.contains("ZDOTDIR='/x/shell'"));
    }
}
