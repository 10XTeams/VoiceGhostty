//! 字体:egui 自带字体没有中日韩字形,也没有 ⌘⇧ 这类符号。启动时从系统字体里组一套:
//! - 终端(Monospace):用户配置的 font-family → macOS 上的 SF Mono → egui 自带的 Hack
//! - 界面(Proportional):macOS 上的 SF Pro → egui 自带的 Ubuntu;另有 Medium / Semibold 两个字重
//! - 之后依次是 CJK 字体、符号字体、egui 自带的 emoji 字体作后备
//!
//! 字体文件有几十 MB,读进来后 leak 成 'static,同一个文件的多个字重/字体族共享同一块字节。
//! 只在启动时调一次,泄漏有界。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::epaint::text::{Fonts, TextOptions, VariationCoords};
use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId, FontTweak};

/// 界面用的两个加粗字重(FontFamily::Name)
pub const MEDIUM: &str = "ui-medium";
pub const SEMIBOLD: &str = "ui-semibold";

pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MEDIUM.into()))
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

/// 各平台常见的 CJK 字体位置(按优先级)。`.1` 是加粗字重在 ttc 里的下标(没有就 None)
const CJK_CANDIDATES: &[(&str, Option<u32>)] = &[
    // macOS
    ("/System/Library/Fonts/PingFang.ttc", None),
    ("/System/Library/Fonts/Hiragino Sans GB.ttc", Some(2)), // 0 = W3,2 = W6(1、3 是界面专用变体)
    ("/System/Library/Fonts/STHeiti Medium.ttc", None),
    ("/Library/Fonts/Arial Unicode.ttf", None),
    // Windows
    ("C:\\Windows\\Fonts\\msyh.ttc", None), // 粗体在单独的 msyhbd.ttc 里
    ("C:\\Windows\\Fonts\\simhei.ttf", None),
    // Linux
    ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", None),
    ("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc", None),
    ("/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc", None),
    ("/usr/share/fonts/truetype/wqy/wqy-microhei.ttc", None),
    ("/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc", None),
];

/// 界面主字体(可变字体,字重靠 wght 轴)
const UI_CANDIDATES: &[&str] = &["/System/Library/Fonts/SFNS.ttf"];
/// 终端主字体(没配置 font-family 时)
const MONO_CANDIDATES: &[&str] = &["/System/Library/Fonts/SFNSMono.ttf"];
/// ⌘⇧⌥⏎✕ 这类符号
const SYMBOL_CANDIDATES: &[&str] = &[
    "/System/Library/Fonts/Apple Symbols.ttf",
    "C:\\Windows\\Fonts\\seguisym.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
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

fn read_static(path: &Path) -> Option<&'static [u8]> {
    std::fs::read(path).ok().map(|b| &*Box::leak(b.into_boxed_slice()))
}

fn first_static(paths: &[&str]) -> Option<&'static [u8]> {
    paths.iter().find_map(|p| read_static(Path::new(p)))
}

fn weight(data: &FontData, wght: f32) -> FontData {
    let tweak = FontTweak { coords: VariationCoords::new([(b"wght", wght)]), ..data.tweak.clone() };
    data.clone().tweak(tweak)
}

pub fn install(ctx: &egui::Context, font_family: &str) {
    ctx.set_fonts(definitions(font_family));
}

fn definitions(font_family: &str) -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    let add = |defs: &mut FontDefinitions, name: &str, data: FontData| {
        defs.font_data.insert(name.into(), Arc::new(data));
    };

    // 终端主字体
    let user_mono = (!font_family.is_empty())
        .then(|| {
            let found = find_font_file(font_family).and_then(|p| read_static(&p));
            if found.is_none() {
                log::warn!("font-family `{font_family}` not found; using the default monospace font");
            }
            found
        })
        .flatten();
    if let Some(bytes) = user_mono.or_else(|| first_static(MONO_CANDIDATES)) {
        add(&mut defs, "mono", FontData::from_static(bytes));
        defs.families.entry(FontFamily::Monospace).or_default().insert(0, "mono".into());
    }

    // 界面主字体 + 两个字重。没有系统字体时,两个字重退回 egui 自带的比例字体
    let base_proportional = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let ui = first_static(UI_CANDIDATES).map(FontData::from_static);
    for (family, wght) in [
        (FontFamily::Proportional, None),
        (FontFamily::Name(MEDIUM.into()), Some(510.0)),
        (FontFamily::Name(SEMIBOLD.into()), Some(590.0)),
    ] {
        let mut chain = base_proportional.clone();
        if let Some(ui) = &ui {
            let name = format!("ui-{}", wght.unwrap_or(400.0) as u32);
            add(&mut defs, &name, wght.map_or_else(|| ui.clone(), |w| weight(ui, w)));
            chain.insert(0, name);
        }
        defs.families.insert(family, chain);
    }

    // 后备:CJK(加粗字重给 Semibold 用)→ 符号。插在 egui 自带 emoji 字体之前,
    // 否则 ⌘、✕ 之类会先被 emoji-icon-font 里风格不搭的同名字形截走
    let families: Vec<FontFamily> = defs.families.keys().cloned().collect();
    let cjk = CJK_CANDIDATES.iter().find_map(|(p, bold)| read_static(Path::new(p)).map(|b| (b, *bold)));
    let symbols = first_static(SYMBOL_CANDIDATES);
    let mut cjk_names = vec![];
    for family in &families {
        let mut fallbacks = vec![];
        if let Some((bytes, bold)) = cjk {
            let mut data = FontData::from_static(bytes);
            if *family == FontFamily::Name(SEMIBOLD.into()) {
                if let Some(index) = bold {
                    data.index = index;
                }
            }
            // 每个字体族一份,因为基线偏移要对着各自的主字体量(见 cjk_baseline_shift)
            let name = format!("cjk-{}", family_key(family));
            add(&mut defs, &name, data);
            cjk_names.push((family.clone(), name.clone()));
            fallbacks.push(name);
        }
        if let Some(bytes) = symbols {
            add(&mut defs, "symbols", FontData::from_static(bytes));
            fallbacks.push("symbols".into());
        }
        let chain = defs.families.get_mut(family).unwrap();
        let at = chain.iter().position(|n| n.starts_with("NotoEmoji") || n.starts_with("emoji")).unwrap_or(chain.len());
        chain.splice(at..at, fallbacks);
    }
    if cjk.is_none() {
        log::warn!("no CJK font found; Chinese text will not render");
    }

    for (family, name) in cjk_names {
        let shift = cjk_baseline_shift(&defs, family);
        let data = defs.font_data[&name].as_ref().clone();
        let tweak = FontTweak { y_offset_factor: shift, ..data.tweak.clone() };
        add(&mut defs, &name, data.tweak(tweak));
    }
    defs
}

fn family_key(family: &FontFamily) -> String {
    match family {
        FontFamily::Monospace => "mono".into(),
        FontFamily::Proportional => "ui".into(),
        FontFamily::Name(n) => n.to_string(),
    }
}

/// egui 按后备字体自己的度量摆放字形。CJK 字体的行高比拉丁字体大得多,结果中文的基线
/// 比同一行的英文高出一截(冬青黑体配 Hack 约 0.21em,看上去差了半行)。
/// 在大字号下量出两者基线之差(按字号的比例),交给 `FontTweak::y_offset_factor` 拉回来
fn cjk_baseline_shift(defs: &FontDefinitions, family: FontFamily) -> f32 {
    const SIZE: f32 = 100.0;
    let mut fonts = Fonts::new(TextOptions::default(), defs.clone());
    let mut view = fonts.with_pixels_per_point(1.0);
    let font = FontId::new(SIZE, family);
    let mut baseline = |c: char| {
        let galley = view.layout_no_wrap(c.to_string(), font.clone(), Color32::WHITE);
        galley.rows.first().and_then(|r| r.glyphs.first()).map(|g| g.pos.y)
    };
    match (baseline('M'), baseline('中')) {
        (Some(latin), Some(cjk)) => (latin - cjk) / SIZE,
        _ => 0.0,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 中文和英文在同一行要坐在同一条基线上(之前中文高出约 0.2em)
    #[test]
    fn cjk_sits_on_the_latin_baseline() {
        let ctx = egui::Context::default();
        install(&ctx, "");
        let mut out = ctx.run_ui(Default::default(), |ui| {
            let families = [FontFamily::Monospace, FontFamily::Proportional, FontFamily::Name(SEMIBOLD.into())];
            for family in families {
                for size in [11.0, 14.0, 20.0, 32.0] {
                    let galley = ui.fonts_mut(|f| {
                        f.layout_no_wrap("中M".into(), FontId::new(size, family.clone()), Color32::WHITE)
                    });
                    let g = &galley.rows[0].glyphs;
                    if g[0].chr != '中' || g[1].chr != 'M' {
                        return; // 没找到 CJK 字体的机器上不适用
                    }
                    // FontTweak 只挪画出来的字形(uv_rect),不改 pos,所以比墨迹:中心对齐、不出行
                    let ink = |g: &egui::epaint::text::Glyph| {
                        let top = g.pos.y + g.uv_rect.offset.y;
                        (top, top + g.uv_rect.size.y)
                    };
                    let ((ct, cb), (lt, lb)) = (ink(&g[0]), ink(&g[1]));
                    let off = ((ct + cb) - (lt + lb)).abs() / 2.0;
                    // 修之前约 0.21em
                    assert!(off <= 0.1 * size, "{family:?} {size}pt: CJK ink center off by {off}px");
                    assert!(ct >= -1.0 && cb <= galley.rows[0].height() + 1.0, "{family:?} {size}pt: CJK ink outside the row");
                }
            }
        });
        out.textures_delta.clear(); // headless:没有渲染器消费纹理更新
    }
}
