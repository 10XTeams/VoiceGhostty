//! 热词:喂给 Whisper 提示词的专有名词,让「VoiceGhostty」「cargo」「main.rs」这类词不再被听成同音别字。
//!
//! 来源有两个,都在开始录音那一刻采集(实时字幕与最终识别用同一份):
//! - **自动**(`voice-hotwords-auto`,默认开):当前目录下的文件与文件夹名(跳过隐藏项与纯数字名),
//!   以及 git 分支名。你正在这个目录里干活,说出来的多半就是这些名字。
//! - **手动**(`voice-hotwords`,逗号分隔):项目、同事、工具的名字,长期有效。
//!
//! Whisper 的提示词只有 224 个 token 的窗口,所以总量封顶;手动词放前面,自动词按名字长度从长到短
//! (短名字如 `src` 模型本来就认识,长名字才需要提示)。

use std::path::{Path, PathBuf};

/// 自动热词最多取这么多个;整体最多这么多字符
pub const MAX_AUTO_WORDS: usize = 30;
pub const MAX_TOTAL_CHARS: usize = 400;

/// 采集热词:手动列表 + (开着自动时)目录条目与分支名。去重、封顶
pub fn collect(cwd: Option<&Path>, manual: &[String], auto: bool) -> Vec<String> {
    let mut words: Vec<String> = manual.iter().map(|w| w.trim().to_string()).filter(|w| !w.is_empty()).collect();
    if auto {
        if let Some(dir) = cwd {
            if let Some(branch) = git_branch(dir) {
                words.push(branch);
            }
            words.extend(directory_names(dir, MAX_AUTO_WORDS));
        }
    }
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    let mut total = 0usize;
    for w in words {
        if !seen.insert(w.to_lowercase()) {
            continue;
        }
        total += w.chars().count() + 2;
        if total > MAX_TOTAL_CHARS {
            break;
        }
        out.push(w);
    }
    out
}

/// 目录下值得当热词的名字:跳过隐藏项、纯数字 / 纯符号的名字和太短的(≤ 2 个字符模型本来就认识);
/// 长的在前,最多 `limit` 个
pub fn directory_names(dir: &Path, limit: usize) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| !n.starts_with('.'))
        .filter(|n| n.chars().count() > 2)
        .filter(|n| n.chars().any(|c| c.is_alphabetic()))
        .collect();
    names.sort_by(|a, b| b.chars().count().cmp(&a.chars().count()).then_with(|| a.cmp(b)));
    names.truncate(limit);
    names
}

/// 当前 git 分支名(读 `.git/HEAD`,不起子进程;支持 worktree 的 `.git` 文件)。分离头指针返回 None
pub fn git_branch(dir: &Path) -> Option<String> {
    let git_dir = find_git_dir(dir)?;
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let rest = head.trim().strip_prefix("ref: refs/heads/")?;
    (!rest.is_empty()).then(|| rest.to_string())
}

/// 从 `dir` 往上找 `.git`;它是文件时(worktree / 子模块)读里面的 `gitdir:`
fn find_git_dir(dir: &Path) -> Option<PathBuf> {
    for d in dir.ancestors() {
        let dot = d.join(".git");
        if dot.is_dir() {
            return Some(dot);
        }
        if dot.is_file() {
            let text = std::fs::read_to_string(&dot).ok()?;
            let target = text.trim().strip_prefix("gitdir:")?.trim();
            let path = PathBuf::from(target);
            return Some(if path.is_absolute() { path } else { d.join(path) });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_branch_and_names_with_caps() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir(root.join(".git")).unwrap();
        std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/loader-panel\n").unwrap();
        std::fs::create_dir(root.join("crates")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "").unwrap();
        std::fs::write(root.join("README.zh-CN.md"), "").unwrap();
        std::fs::write(root.join("ab"), "").unwrap(); // 太短
        std::fs::write(root.join("2026"), "").unwrap(); // 纯数字
        std::fs::write(root.join(".hidden"), "").unwrap();

        let words = collect(Some(root), &["VoiceGhostty".into(), " ".into(), "voiceghostty".into()], true);
        assert_eq!(words[0], "VoiceGhostty");
        assert_eq!(words[1], "loader-panel");
        assert_eq!(&words[2..], &["README.zh-CN.md", "Cargo.toml", "crates"]);
        assert_eq!(words.len(), 5);

        // 关掉自动只剩手动
        assert_eq!(collect(Some(root), &["x".into()], false), vec!["x".to_string()]);
    }

    #[test]
    fn git_worktree_file_and_detached_head() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("real/worktrees/w1")).unwrap();
        std::fs::write(root.join("real/worktrees/w1/HEAD"), "ref: refs/heads/feature/x\n").unwrap();
        std::fs::create_dir(root.join("wt")).unwrap();
        std::fs::write(root.join("wt/.git"), format!("gitdir: {}\n", root.join("real/worktrees/w1").display())).unwrap();
        assert_eq!(git_branch(&root.join("wt")).as_deref(), Some("feature/x"));

        std::fs::write(root.join("real/worktrees/w1/HEAD"), "0123abcd\n").unwrap();
        assert_eq!(git_branch(&root.join("wt")), None);
        assert_eq!(git_branch(Path::new("/nonexistent/dir")), None);
    }

    #[test]
    fn total_chars_cap() {
        let manual: Vec<String> = (0..100).map(|i| format!("word-number-{i:03}")).collect();
        let words = collect(None, &manual, false);
        let total: usize = words.iter().map(|w| w.chars().count() + 2).sum();
        assert!(total <= MAX_TOTAL_CHARS);
        assert!(words.len() < 100);
    }
}
