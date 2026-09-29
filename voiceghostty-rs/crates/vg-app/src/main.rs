//! VoiceGhostty(Rust 版):跨平台语音终端。

mod app;
mod fonts;
mod monitor;
mod pipeline;
mod ptt;
mod terminal;

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    // 让 shell 继承正确的 locale 等环境变量(alacritty 的约定)
    alacritty_terminal::tty::setup_env();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("VoiceGhostty")
            .with_inner_size([960.0, 600.0])
            .with_min_inner_size([400.0, 240.0]),
        ..Default::default()
    };
    eframe::run_native(
        "VoiceGhostty",
        options,
        Box::new(|cc| Ok(Box::new(app::VoiceGhosttyApp::new(cc)))),
    )
}
