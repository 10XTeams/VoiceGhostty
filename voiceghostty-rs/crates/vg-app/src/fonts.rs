//! 字体:egui 自带字体没有中日韩字形,中文会显示成方块。启动时从系统字体里找一个 CJK 字体
//! 作为等宽/比例字体的后备;配置了 font-family 且能找到对应文件时,用它当终端主字体。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

/// 各平台常见的 CJK 字体位置(按优先级)
const CJK_CANDIDATES: &[&str] = &[
    // macOS
    "/System/Library/Fonts/PingFang.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/System/Library/Fonts/STHeiti Medium.ttc",
    "/Library/Fonts/Arial Unicode.ttf",
    // Windows
    "C:\\Windows\\Fonts\\msyh.ttc",
    "C:\\Windows\\Fonts\\simhei.ttf",
    // Linux
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    "/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc",
];

/// 等宽主字体的搜索目录
fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![];
    if cfg!(target_os = "macos") {
        dirs.push(PathBuf::from("/System/Library/Fonts"));
        dirs.push(PathBuf::from("/Library/Fonts"));
        dirs.push(vg_core::paths::home_dir().join("Library/Fonts"));
    } else if cfg!(windows) {
        dirs.push(PathBuf::from("C:\\Windows\\Fonts"));
    } else {
        dirs.push(PathBuf::from("/usr/share/fonts"));
        dirs.push(vg_core::paths::home_dir().join(".local/share/fonts"));
    }
    dirs
}

pub fn install(ctx: &egui::Context, font_family: &str) {
    let mut defs = FontDefinitions::default();

    if !font_family.is_empty() {
        if let Some(path) = find_font_file(font_family) {
            if let Ok(bytes) = std::fs::read(&path) {
                defs.font_data.insert("user-mono".into(), Arc::new(FontData::from_owned(bytes)));
                defs.families.entry(FontFamily::Monospace).or_default().insert(0, "user-mono".into());
            }
        } else {
            log::warn!("font-family `{font_family}` not found; using the default monospace font");
        }
    }

    if let Some(bytes) = CJK_CANDIDATES.iter().find_map(|p| std::fs::read(p).ok()) {
        defs.font_data.insert("cjk".into(), Arc::new(FontData::from_owned(bytes)));
        for family in [FontFamily::Monospace, FontFamily::Proportional] {
            defs.families.entry(family).or_default().push("cjk".into());
        }
    } else {
        log::warn!("no CJK font found; Chinese text will not render");
    }
    ctx.set_fonts(defs);
}

/// 在字体目录里找文件名包含 family(忽略大小写和空格)的 ttf/otf
fn find_font_file(family: &str) -> Option<PathBuf> {
    let needle = family.to_lowercase().replace(' ', "");
    font_dirs().iter().find_map(|dir| search(dir, &needle, 3))
}

fn search(dir: &Path, needle: &str, depth: u8) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut subdirs = vec![];
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            subdirs.push(path);
            continue;
        }
        let name = path.file_name()?.to_string_lossy().to_lowercase().replace([' ', '-', '_'], "");
        let ext_ok = matches!(path.extension().and_then(|e| e.to_str()), Some("ttf" | "otf" | "TTF" | "OTF"));
        // 优先 Regular:名字里带 bold/italic 的跳过
        if ext_ok && name.contains(needle) && !name.contains("bold") && !name.contains("italic") {
            return Some(path);
        }
    }
    if depth == 0 {
        return None;
    }
    subdirs.iter().find_map(|d| search(d, needle, depth - 1))
}
