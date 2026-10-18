//! 版本信息。版本号的唯一来源是 Cargo.toml 的 workspace version;提交号与构建时刻由 build.rs 注入。

/// 市场版本号,如 `0.1.0`
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// 构建自哪个提交,如 `abc1234` 或 `abc1234+`(带未提交改动)
pub const GIT_COMMIT: &str = env!("VG_GIT_COMMIT");

/// 设置窗口底部那行:`v0.1.0 (abc1234+)`。提交号未知时只给版本号
pub fn label() -> String {
    match GIT_COMMIT {
        "" | "unknown" => format!("v{VERSION}"),
        commit => format!("v{VERSION} ({commit})"),
    }
}

/// 悬停提示:加上构建时刻,用来区分同一提交的两次构建
pub fn detail() -> String {
    let built = env!("VG_BUILD_EPOCH")
        .parse::<i64>()
        .ok()
        .and_then(|secs| chrono::DateTime::from_timestamp(secs, 0))
        .map(|t| t.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "-".into());
    format!("版本 {VERSION}\n提交 {GIT_COMMIT}\n构建于 {built}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_contains_version() {
        assert!(label().starts_with(&format!("v{VERSION}")));
        assert!(detail().contains(VERSION));
    }
}
