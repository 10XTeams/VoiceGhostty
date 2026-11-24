//! 界面截图:用真 App(真 shell)+ wgpu 离屏渲染出 PNG,用来逐张检查视觉效果。默认跳过,手动跑:
//!   VG_SHOTS=/tmp/shots cargo test -p vg-app shots -- --ignored --nocapture
//! 会在你的真实环境里起 shell(读你的 zsh 配置),只读不写:会话记录关闭,设置不保存。

use std::path::{Path, PathBuf};
use std::time::Duration;

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use super::{VoiceGhosttyApp, VoiceUi};

const SIZE: [f32; 2] = [1120.0, 700.0];

fn settle(h: &mut Harness<'_, VoiceGhosttyApp>, total: Duration) {
    let step = Duration::from_millis(50);
    let mut waited = Duration::ZERO;
    while waited < total {
        h.step();
        std::thread::sleep(step);
        waited += step;
    }
    h.step();
}

fn save(h: &mut Harness<'_, VoiceGhosttyApp>, dir: &Path, name: &str) {
    let img = h.render().expect("wgpu render");
    let path = dir.join(format!("{name}.png"));
    img.save(&path).unwrap();
    eprintln!("saved {}", path.display());
}

fn type_demo(app: &VoiceGhosttyApp, repo: &Path) {
    let s = app.active_session().unwrap();
    s.write(format!("cd '{}' && clear && ls -G && git log --oneline -5 --color=always\r", repo.display()).into_bytes());
}

#[test]
#[ignore]
fn shots() {
    let Some(dir) = std::env::var_os("VG_SHOTS").map(PathBuf::from) else {
        eprintln!("skipped: set VG_SHOTS=<dir>");
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");

    let mut h = Harness::builder()
        .with_size(SIZE)
        .with_pixels_per_point(2.0)
        .wgpu()
        .build_eframe(|cc| {
            let mut app = VoiceGhosttyApp::new(cc);
            app.settings.save_history = false;
            app
        });
    settle(&mut h, Duration::from_secs(2));
    type_demo(h.state(), &repo);
    settle(&mut h, Duration::from_secs(2));
    h.state().active_session().unwrap().type_text("可以讲话吗？Mixed 中英文 baseline 对齐 check");
    settle(&mut h, Duration::from_millis(800));
    save(&mut h, &dir, "01-main");

    h.state_mut().set_status("已上屏:可以讲话吗？Mixed 中英文 baseline 对齐 check", false);
    settle(&mut h, Duration::from_millis(200));
    save(&mut h, &dir, "02-status");

    // 录音中:模拟一段说话的电平
    let levels: Vec<f32> = (0..super::chrome::WAVE_BARS).map(|i| 0.02 + 0.10 * ((i as f32 * 0.9).sin().abs() * (i as f32 / 9.0).cos().abs())).collect();
    h.state_mut().preview_voice = Some(VoiceUi::Recording { secs: 3.4, levels });
    settle(&mut h, Duration::from_millis(200));
    save(&mut h, &dir, "02b-recording");
    h.state_mut().preview_voice = Some(VoiceUi::Working("识别中…".into()));
    settle(&mut h, Duration::from_millis(200));
    save(&mut h, &dir, "02c-working");
    h.state_mut().preview_voice = None;

    h.state_mut().show_settings = true;
    settle(&mut h, Duration::from_millis(400));
    save(&mut h, &dir, "03-settings");
    for (i, name) in [(1, "03b-settings-appearance"), (2, "03c-settings-voice"), (3, "03d-settings-llm"), (4, "03e-settings-data")] {
        crate::settings_ui::select_tab(&h.ctx, i);
        settle(&mut h, Duration::from_millis(200));
        save(&mut h, &dir, name);
    }
    // 选了 HTTP 引擎时多出的接口设置
    crate::settings_ui::select_tab(&h.ctx, 2);
    let ctx = h.ctx.clone();
    h.state_mut().settings.voice_engine = Some("http".into());
    h.state_mut().reload_config(&ctx);
    settle(&mut h, Duration::from_millis(200));
    save(&mut h, &dir, "03c2-settings-voice-http");
    h.state_mut().settings.voice_engine = None;
    h.state_mut().reload_config(&ctx);
    crate::settings_ui::select_tab(&h.ctx, 0);
    h.state_mut().show_settings = false;

    h.state_mut().toggle_loader();
    settle(&mut h, Duration::from_millis(400));
    save(&mut h, &dir, "04-loader");
    h.state_mut().show_loader = false;

    h.state_mut().show_help = true;
    settle(&mut h, Duration::from_millis(400));
    save(&mut h, &dir, "05-help");
    h.state_mut().show_help = false;

    let ctx = h.ctx.clone();
    h.state_mut().split(&ctx, crate::split::Axis::Horizontal);
    h.state_mut().new_tab(&ctx);
    h.state_mut().ws.select_tab(0);
    h.state_mut().search_open = true;
    settle(&mut h, Duration::from_secs(2));
    save(&mut h, &dir, "06-split-tabs-search");
    h.state_mut().search_open = false;

    // 窗格列表:顶部标签栏让位给左侧列表
    h.state_mut().settings.show_sidebar = true;
    settle(&mut h, Duration::from_millis(300));
    save(&mut h, &dir, "06b-sidebar");
    h.state_mut().settings.show_sidebar = false;

    h.state_mut().set_status("识别失败:No whisper model found", true);
    h.state_mut().confirm_quit = true;
    settle(&mut h, Duration::from_millis(400));
    save(&mut h, &dir, "07-quit-error");

    h.state_mut().confirm_quit = false;

    h.state_mut().new_project = Some("my-new-app".into());
    settle(&mut h, Duration::from_millis(400));
    save(&mut h, &dir, "07b-new-project");
    h.state_mut().new_project = None;

    for theme in ["light", "dracula"] {
        h.state_mut().config.theme_name = theme.into();
        let t = h.state().config.theme();
        crate::style::apply(&h.ctx, &t);
        h.state_mut().set_status("已上屏:git status", false);
        settle(&mut h, Duration::from_millis(400));
        save(&mut h, &dir, &format!("08-theme-{theme}"));
    }
    h.state_mut().show_settings = true;
    settle(&mut h, Duration::from_millis(400));
    save(&mut h, &dir, "09-settings-dracula");
    // kittest 不调 on_exit:手动释放 whisper 模型,否则 Metal 后端在进程退出时 abort
    drop(h);
    vg_voice::shutdown();
}

fn quick_buttons() -> Vec<vg_core::settings::QuickButton> {
    use vg_core::settings::QuickButton;
    vec![
        QuickButton { label: "状态".into(), text: "git status".into(), run: false },
        QuickButton { label: "回显".into(), text: "echo vg-quick-$((40+2))".into(), run: true },
        QuickButton { label: String::new(), text: "cargo test --workspace".into(), run: true },
    ]
}

/// 点状态栏的快捷按钮:内容进当前窗格;勾了「回车」的会执行,没勾的只输入不执行
#[test]
fn quick_button_types_into_active_pane() {
    let mut h = Harness::builder().with_size(SIZE).build_eframe(|cc| {
        let mut app = VoiceGhosttyApp::new(cc);
        app.settings.save_history = false;
        app.settings.quick_buttons = quick_buttons();
        app
    });
    let wait_for = |h: &mut Harness<'_, VoiceGhosttyApp>, want: &str| {
        for _ in 0..100 {
            h.step();
            if h.state().active_session().unwrap().screen_tail(40).contains(want) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    };
    settle(&mut h, Duration::from_secs(1));
    h.get_by_label("回显").click();
    // 执行了才会出现算好的 42
    assert!(wait_for(&mut h, "vg-quick-42"), "回车执行的按钮应当执行命令");
    h.get_by_label("状态").click();
    assert!(wait_for(&mut h, "git status"), "按钮内容应输入到窗格");
    settle(&mut h, Duration::from_millis(500));
    let tail = h.state().active_session().unwrap().screen_tail(40);
    assert!(!tail.contains("On branch") && !tail.contains("not a git repository"), "没勾回车的按钮不应执行:\n{tail}");
    drop(h);
    vg_voice::shutdown();
}

/// 状态栏快捷按钮与设置页的截图:VG_SHOTS=/tmp/shots cargo test -p vg-app quick_buttons_shots -- --ignored
#[test]
#[ignore]
fn quick_buttons_shots() {
    let Some(dir) = std::env::var_os("VG_SHOTS").map(PathBuf::from) else {
        eprintln!("skipped: set VG_SHOTS=<dir>");
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let mut h = Harness::builder()
        .with_size(SIZE)
        .with_pixels_per_point(2.0)
        .wgpu()
        .build_eframe(|cc| {
            let mut app = VoiceGhosttyApp::new(cc);
            app.settings.save_history = false;
            app.settings.quick_buttons = quick_buttons();
            app
        });
    settle(&mut h, Duration::from_secs(2));
    save(&mut h, &dir, "10-quick-buttons");
    h.state_mut().show_settings = true;
    crate::settings_ui::select_tab(&h.ctx, 0);
    settle(&mut h, Duration::from_millis(400));
    save(&mut h, &dir, "10b-quick-buttons-settings");
    drop(h);
    vg_voice::shutdown();
}

/// 回归:「新建项目」浮窗打开后,键盘输入要进输入框,而不是被终端窗格抢走
#[test]
fn new_project_dialog_takes_keyboard() {
    let mut h = Harness::builder().with_size(SIZE).build_eframe(|cc| {
        let mut app = VoiceGhosttyApp::new(cc);
        app.settings.save_history = false;
        app
    });
    for _ in 0..3 {
        h.step();
    }
    assert!(h.ctx.memory(|m| m.focused()).is_some(), "终端窗格起来后应持有焦点");
    h.state_mut().new_project = Some(String::new());
    h.state_mut().new_project_focus = true;
    h.step();
    h.input_mut().events.push(egui::Event::Text("demo".into()));
    h.step();
    h.step();
    assert_eq!(h.state().new_project.as_deref(), Some("demo"));
    drop(h);
    vg_voice::shutdown();
}

/// 回归:分隔线能用鼠标拖动、双击恢复等分;⌘⇧↩ 放大 / 还原;拖分隔线不会在终端里拉出选区
#[test]
fn drag_divider_resizes_and_zoom_toggles() {
    use crate::split::Axis;
    use egui::{Event, PointerButton, Pos2};

    // 每步 1/60 秒:默认 0.25 秒一步,两次点击就超出了双击判定的 0.3 秒
    let mut h = Harness::builder().with_size(SIZE).with_step_dt(1.0 / 60.0).build_eframe(|cc| {
        let mut app = VoiceGhosttyApp::new(cc);
        app.settings.save_history = false;
        app.settings.show_sidebar = false;
        app
    });
    h.step();
    let ctx = h.ctx.clone();
    h.state_mut().split(&ctx, Axis::Horizontal);
    h.step();
    h.step();
    let widths = |h: &Harness<'_, VoiceGhosttyApp>| -> Vec<f32> {
        let tab = h.state().ws.active_tab().unwrap();
        let area = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 500.0));
        tab.root.frames(area, 1.0).iter().map(|(_, r)| r.width()).collect()
    };
    assert!((widths(&h)[0] - widths(&h)[1]).abs() < 0.01, "刚分出来是等分");

    // 沿窗口中线横扫,光标变成左右箭头的地方就是分隔线
    let y = SIZE[1] / 2.0;
    let mut found = None;
    for x in (400..720).map(|x| x as f32) {
        h.input_mut().events.push(Event::PointerMoved(Pos2::new(x, y)));
        h.step();
        if h.output().platform_output.cursor_icon == egui::CursorIcon::ResizeHorizontal {
            found = Some(x);
            break;
        }
    }
    let x0 = found.expect("中线附近应有可拖动的分隔线");

    let at = |x: f32, pressed| Event::PointerButton {
        pos: Pos2::new(x, y),
        button: PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    h.input_mut().events.push(at(x0, true));
    h.step();
    for i in 1..=10 {
        h.input_mut().events.push(Event::PointerMoved(Pos2::new(x0 - 20.0 * i as f32, y)));
        h.step();
    }
    h.input_mut().events.push(at(x0 - 200.0, false));
    h.step();
    let w = widths(&h);
    assert!(w[0] < w[1] - 100.0, "向左拖 200 点后左格应明显变窄:{w:?}");
    for pane in h.state().ws.panes.values() {
        assert!(pane.session.term.lock().selection.is_none(), "拖分隔线不应在终端里拉出选区");
    }

    // 双击新位置的分隔线:恢复等分
    let x1 = x0 - 200.0;
    h.input_mut().events.push(Event::PointerMoved(Pos2::new(x1, y)));
    h.step();
    for _ in 0..2 {
        h.input_mut().events.push(at(x1, true));
        h.step();
        h.input_mut().events.push(at(x1, false));
        h.step();
    }
    let w = widths(&h);
    assert!((w[0] - w[1]).abs() < 0.01, "双击后应恢复等分:{w:?}");

    // 放大 / 还原
    let zoom = crate::shortcuts::Action::ToggleZoom.shortcuts()[0];
    let press = |h: &mut Harness<'_, VoiceGhosttyApp>| {
        h.input_mut().events.push(Event::Key {
            key: zoom.logical_key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: zoom.modifiers,
        });
        h.step();
    };
    press(&mut h);
    assert!(h.state().ws.active_tab().unwrap().is_zoomed());
    press(&mut h);
    assert!(!h.state().ws.active_tab().unwrap().is_zoomed());
    drop(h);
    vg_voice::shutdown();
}

/// 「下载语音模型」浮窗:截图看选择 / 进度 / 取消三个状态,走真实网络(含系统代理)下载 base 模型的一段。
/// 完整下载、断点续传、换源由 vg-core 的本机服务器测试覆盖;这里验证真实下载源和界面。
/// 会把 HOME 指到 VG_SHOTS 下的临时目录,不碰你的真实模型目录和设置。重复运行会接着上次的 .part 下。
///   VG_SHOTS=/tmp/shots cargo test -p vg-app model_download_shots -- --ignored --nocapture
#[test]
#[ignore]
fn model_download_shots() {
    use vg_core::model_download::Phase;
    let Some(dir) = std::env::var_os("VG_SHOTS").map(PathBuf::from) else {
        eprintln!("skipped: set VG_SHOTS=<dir>");
        return;
    };
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    // 这个测试只单独运行(#[ignore] + 名字过滤),没有别的线程在读环境变量
    std::env::set_var("HOME", &home);
    let part = home.join(".config/voiceghostty/models/ggml-base.bin.part");
    let had = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);

    let mut h = Harness::builder()
        .with_size(SIZE)
        .with_pixels_per_point(2.0)
        .wgpu()
        .build_eframe(|cc| {
            let mut app = VoiceGhosttyApp::new(cc);
            app.settings.save_history = false;
            app
        });
    // 编进了 whisper 时,空的 HOME(没有模型)启动就该自动弹出;没编进来时手动打开看界面
    if vg_voice::HAS_WHISPER {
        assert!(h.state().show_model_dialog, "首次启动没有模型,应自动弹出下载窗口");
    }
    h.state_mut().show_model_dialog = true;
    h.state_mut().model_choice = 3; // base,148 MB
    settle(&mut h, Duration::from_millis(500));
    save(&mut h, &dir, "model-1-choose");

    h.input_mut().events.push(egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    let started = std::time::Instant::now();
    let mut first_seen = None;
    loop {
        settle(&mut h, Duration::from_millis(250));
        let phase = h.state().model_download.as_ref().map(|d| d.phase());
        if let Some(Phase::Downloading { done, .. }) = phase {
            let first = *first_seen.get_or_insert(done);
            if done >= first + 1_000_000 {
                break;
            }
        }
        if let Some(Phase::Failed(e)) = &phase {
            panic!("download failed: {e}");
        }
        assert!(started.elapsed() < Duration::from_secs(180), "no progress: {phase:?}");
    }
    let first = first_seen.unwrap();
    eprintln!("had {had} bytes on disk; first progress report at {first}");
    assert!(first >= had, "应接着已有的 {had} 字节下,而不是从头开始(首次报告 {first})");
    save(&mut h, &dir, "model-2-progress");

    h.state().model_download.as_ref().unwrap().cancel();
    settle(&mut h, Duration::from_secs(1));
    assert_eq!(h.state().model_download.as_ref().map(|d| d.phase()), Some(Phase::Cancelled));
    assert!(std::fs::metadata(&part).unwrap().len() > had, "取消后已下载的部分保留");
    save(&mut h, &dir, "model-3-cancelled");
    drop(h);
    vg_voice::shutdown();
}

/// 用量洞察面板的三个页签。默认注入示例数据;设 VG_INSIGHTS_LEDGER=<台账目录> 则读那份真实台账。
///   VG_SHOTS=/tmp/shots cargo test -p vg-app insights_shots -- --ignored --nocapture
#[test]
#[ignore]
fn insights_shots() {
    use crate::insights_panel::Tab;
    let Some(dir) = std::env::var_os("VG_SHOTS").map(PathBuf::from) else {
        eprintln!("skipped: set VG_SHOTS=<dir>");
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let mut h = Harness::builder()
        .with_size([1120.0, 900.0])
        .with_pixels_per_point(2.0)
        .wgpu()
        .build_eframe(|cc| {
            let mut app = VoiceGhosttyApp::new(cc);
            app.settings.save_history = false;
            app
        });
    settle(&mut h, Duration::from_millis(800));
    let now = vg_core::insights::time::now_ms();
    let snap = match std::env::var_os("VG_INSIGHTS_LEDGER") {
        Some(ledger) => crate::insights::Snapshot {
            records: std::sync::Arc::new(vg_core::insights::ledger::load(std::path::Path::new(&ledger), 0)),
            last_scan_ms: Some(now),
            ..Default::default()
        },
        None => crate::insights_panel::sample_snapshot(now),
    };
    h.state().insights.replace(snap);
    for (tab, name) in [(Tab::Advice, "insights-1-advice"), (Tab::Usage, "insights-2-usage"), (Tab::Sessions, "insights-3-sessions")] {
        h.state_mut().insights_panel.open(tab, None);
        h.state_mut().show_insights = true;
        settle(&mut h, Duration::from_millis(300));
        save(&mut h, &dir, name);
    }
}

