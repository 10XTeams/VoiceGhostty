//! 界面截图:用真 App(真 shell)+ wgpu 离屏渲染出 PNG,用来逐张检查视觉效果。默认跳过,手动跑:
//!   VG_SHOTS=/tmp/shots cargo test -p vg-app shots -- --ignored --nocapture
//! 会在你的真实环境里起 shell(读你的 zsh 配置),只读不写:会话记录关闭,设置不保存。

use std::path::{Path, PathBuf};
use std::time::Duration;

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

    h.state_mut().set_status("识别失败:No whisper model found", true);
    h.state_mut().confirm_quit = true;
    settle(&mut h, Duration::from_millis(400));
    save(&mut h, &dir, "07-quit-error");

    h.state_mut().confirm_quit = false;

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
