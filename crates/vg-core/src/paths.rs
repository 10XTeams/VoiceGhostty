//! 配置目录。沿用 Swift 版的 `~/.config/voiceghostty/`,这样两个版本能共用同一份配置;
//! Windows 上没有 `~/.config` 的惯例,改用系统配置目录(`%APPDATA%\voiceghostty`)。

use std::path::PathBuf;

pub fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

pub fn config_dir() -> PathBuf {
    if cfg!(windows) {
        dirs::config_dir()
            .unwrap_or_else(home_dir)
            .join("voiceghostty")
    } else {
        home_dir().join(".config").join("voiceghostty")
    }
}

/// 只读的 `key = value` 配置文件
pub fn config_file() -> PathBuf {
    config_dir().join("config")
}

/// GUI 设置(对应 Swift 版的 UserDefaults)
pub fn settings_file() -> PathBuf {
    config_dir().join("settings.json")
}

/// 展开开头的 `~`
pub fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        return home_dir();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return home_dir().join(rest);
    }
    PathBuf::from(path)
}

/// 会话记录(每个窗格一个 .log)
pub fn history_dir() -> PathBuf {
    config_dir().join("history")
}

/// 运行期文件(各窗格的命令边界状态文件)
pub fn run_dir() -> PathBuf {
    config_dir().join("run")
}
