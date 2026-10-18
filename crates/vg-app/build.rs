//! 把 git 提交号注入二进制(env `VG_GIT_COMMIT`),设置窗口底部显示 `v0.1.0 (abc1234+)`。
//! 市场版本号跨构建不变,单看它分不出新旧;提交号才能说明跑的 .app 里有没有某个修复。
//! 末尾的 `+` 表示构建时带着未提交改动。
//!
//! 打包脚本(make-app.sh / CI)会直接设好 `VG_GIT_COMMIT`,这里原样采用;没设时自己问 git。
//! 开发时 `cargo run` 只在 HEAD 或暂存区变化时重新计算,所以 `+` 可能滞后 —— 以打包产物为准。

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn main() {
    println!("cargo:rerun-if-env-changed=VG_GIT_COMMIT");
    let commit = match std::env::var("VG_GIT_COMMIT") {
        Ok(c) if !c.is_empty() => c,
        _ => {
            if let Some(dir) = git(&["rev-parse", "--git-dir"]) {
                println!("cargo:rerun-if-changed={dir}/HEAD");
                println!("cargo:rerun-if-changed={dir}/index");
            }
            let short = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".into());
            let dirty = git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
            format!("{short}{}", if dirty { "+" } else { "" })
        }
    };
    println!("cargo:rustc-env=VG_GIT_COMMIT={commit}");

    // 构建时刻(Unix 秒),用来区分同一提交的两次构建;显示时由 chrono 格式化
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    println!("cargo:rustc-env=VG_BUILD_EPOCH={epoch}");
}
