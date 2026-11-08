//! 系统通知。macOS 用 `osascript -e 'display notification'`:零依赖、不弹权限请求(通知归 Script Editor 名下,
//! 系统设置里显示为「脚本编辑器」)。Linux 用 `notify-send`(没有就静默)。Windows 暂无。
//! 都是在后台线程里起子进程,不阻塞监视线程。

/// 发一条通知。标题 / 正文里的引号会被转义,不会把脚本拆坏
pub fn send(title: &str, body: &str) {
    let (title, body) = (title.to_string(), body.to_string());
    std::thread::spawn(move || {
        #[cfg(target_os = "macos")]
        {
            let script = format!(
                "display notification \"{}\" with title \"{}\"",
                escape_applescript(&body),
                escape_applescript(&title)
            );
            let _ = std::process::Command::new("osascript").arg("-e").arg(script).output();
        }
        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("notify-send").arg("--app-name=VoiceGhostty").arg(&title).arg(&body).output();
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            let _ = (title, body);
        }
    });
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn escape_applescript(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    #[test]
    fn escapes_quotes_and_backslashes() {
        assert_eq!(super::escape_applescript(r#"a "b" \c"#), r#"a \"b\" \\c"#);
    }
}
