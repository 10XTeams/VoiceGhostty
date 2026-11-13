//! 「下载语音模型」浮窗:选模型、看进度、取消 / 转到后台 / 重试。下载本身在 `vg_core::model_download`。
//!
//! 首次启动编进了 whisper 却没有模型时自动弹出;之后从状态栏的语音胶囊或设置页打开。
//! 下载完成后把模型写进设置(`voice-model`)、立即在后台载入,不用重启。

use egui::{vec2, Color32, Key, RichText, Sense, Stroke};

use vg_core::config::models_dir;
use vg_core::model_download::{human_bytes, Download, ModelSpec, Phase, MODELS};
use vg_core::VoiceEngineKind;

use crate::app::VoiceGhosttyApp;
use crate::kit;
use crate::style::{self, alpha, palette};

impl VoiceGhosttyApp {
    /// 开始下载当前选中的模型(已有同名的 .part 会接着下)
    fn start_model_download(&mut self, ctx: &egui::Context) {
        let spec = MODELS[self.model_choice.min(MODELS.len() - 1)];
        let repaint = ctx.clone();
        self.model_download = Some(Download::start(spec, models_dir(), move || repaint.request_repaint()));
    }

    /// 每帧调用:下载完成就启用模型、关浮窗、在状态栏报告
    pub(crate) fn finish_model_download(&mut self, ctx: &egui::Context) {
        let Some(Phase::Done(path)) = self.model_download.as_ref().map(Download::phase) else { return };
        self.model_download = None;
        self.show_model_dialog = false;
        self.settings.voice_model = Some(path);
        self.reload_config(ctx); // 顺带保存设置
        vg_voice::preload(&self.config.voice);
        let msg = if matches!(self.config.voice.engine, VoiceEngineKind::Auto | VoiceEngineKind::Whisper) {
            format!("语音模型已就绪,按住 {} 说话", self.ptt_key_label())
        } else {
            "语音模型已下载;把设置里的语音引擎改成「自动」或「内置 whisper」即可使用".to_string()
        };
        self.set_status(msg, false);
    }

    /// 状态栏胶囊用:正在下载时的简短进度,如「下载模型 37%」
    pub(crate) fn model_download_progress(&self) -> Option<String> {
        match self.model_download.as_ref()?.phase() {
            Phase::Connecting(_) => Some("下载模型 · 连接中".into()),
            Phase::Retrying { .. } => Some("下载模型 · 重连中".into()),
            Phase::Downloading { done, total: Some(total), .. } if total > 0 => {
                Some(format!("下载模型 {}%", done * 100 / total))
            }
            Phase::Downloading { done, .. } => Some(format!("下载模型 {}", human_bytes(done))),
            Phase::Verifying => Some("下载模型 · 校验中".into()),
            _ => None,
        }
    }

    pub(crate) fn model_dialog(&mut self, ctx: &egui::Context) {
        let p = palette(ctx);
        let phase = self.model_download.as_ref().map(Download::phase);
        let running = phase.as_ref().is_some_and(Phase::is_running);
        let frame = egui::Frame::NONE
            .fill(p.surface)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(14)
            .shadow(ctx.global_style().visuals.window_shadow)
            .inner_margin(22);
        let (mut start, mut later, mut background, mut cancel) = (false, false, false, false);

        egui::Modal::new(egui::Id::new("model-download")).frame(frame).backdrop_color(Color32::from_black_alpha(110)).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label(RichText::new("下载语音识别模型").font(crate::fonts::semibold(16.0)).color(p.text));
            ui.add_space(4.0);
            ui.label(
                RichText::new(format!(
                    "VoiceGhostty 用内置 whisper 在本机离线识别,音频不出这台电脑。先下载一个模型(只需一次),保存在 {}。",
                    vg_core::loader::tilde(&models_dir())
                ))
                .font(style::small())
                .color(p.text2),
            );
            ui.add_space(12.0);

            // 下载中仍列出全部(浮窗高度不跳),但不能改选
            for (i, spec) in MODELS.iter().enumerate() {
                if model_row(ui, spec, i == 0, self.model_choice == i, !running).clicked() {
                    self.model_choice = i;
                }
            }

            let spec = MODELS[self.model_choice.min(MODELS.len() - 1)];
            let partial = partial_bytes(&spec);
            if let Some(phase) = &phase {
                ui.add_space(10.0);
                progress(ui, phase);
            } else if partial > 0 {
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!("上次已下载 {},这次接着下", human_bytes(partial))).font(style::small()).color(p.text3),
                );
            }

            ui.add_space(16.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if running {
                    if kit::button(ui, "后台下载", kit::Kind::Primary, true).clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                        background = true;
                    }
                    if kit::button(ui, "取消下载", kit::Kind::Secondary, true).clicked() {
                        cancel = true;
                    }
                } else {
                    let retry = matches!(phase, Some(Phase::Failed(_) | Phase::Cancelled)) || partial > 0;
                    let label = if retry {
                        format!("继续下载 · {}", human_bytes(spec.bytes.saturating_sub(partial)))
                    } else {
                        format!("下载 · {}", human_bytes(spec.bytes))
                    };
                    if kit::button(ui, &label, kit::Kind::Primary, true).clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                        start = true;
                    }
                    if kit::button(ui, "以后再说", kit::Kind::Secondary, true).clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                        later = true;
                    }
                }
            });
        });

        if start {
            self.start_model_download(ctx);
        }
        if cancel {
            if let Some(d) = &self.model_download {
                d.cancel();
            }
        }
        if background {
            self.show_model_dialog = false;
            self.set_status("模型在后台下载,进度见状态栏", false);
        }
        if later {
            self.show_model_dialog = false;
            self.model_download = None;
            if !self.settings.model_prompt_dismissed {
                self.settings.model_prompt_dismissed = true;
                let _ = self.settings.save();
            }
        }
    }
}

/// 模型目录里这个模型下到一半的字节数
fn partial_bytes(spec: &ModelSpec) -> u64 {
    std::fs::metadata(models_dir().join(format!("{}.part", spec.file))).map(|m| m.len()).unwrap_or(0)
}

/// 一行可选的模型:单选圈、名字、大小、说明
fn model_row(ui: &mut egui::Ui, spec: &ModelSpec, recommended: bool, selected: bool, enabled: bool) -> egui::Response {
    let p = palette(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), if enabled { Sense::click() } else { Sense::hover() });
    let painter = ui.painter();
    let fill = if selected {
        alpha(p.accent, 0.12)
    } else if enabled && resp.hovered() {
        p.hover
    } else {
        Color32::TRANSPARENT
    };
    painter.rect_filled(rect, 8.0, fill);
    if selected {
        painter.rect_stroke(rect, 8.0, Stroke::new(1.0, alpha(p.accent, 0.6)), egui::StrokeKind::Inside);
    }
    let c = egui::pos2(rect.min.x + 16.0, rect.center().y);
    // 下载中没被选中的几行压暗:看得出能选什么,也看得出现在不能选
    let dim = |c: Color32| if enabled || selected { c } else { alpha(c, 0.4) };
    painter.circle_stroke(c, 6.0, Stroke::new(1.5, if selected { p.accent } else { dim(p.text3) }));
    if selected {
        painter.circle_filled(c, 3.0, p.accent);
    }
    let x = rect.min.x + 32.0;
    let title = if recommended { format!("{}  · 推荐", spec.title) } else { spec.title.to_string() };
    painter.text(egui::pos2(x, rect.min.y + 13.0), egui::Align2::LEFT_CENTER, title, crate::fonts::medium(13.0), dim(p.text));
    painter.text(egui::pos2(x, rect.max.y - 12.0), egui::Align2::LEFT_CENTER, spec.note, style::small(), dim(p.text3));
    painter.text(
        egui::pos2(rect.max.x - 12.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        human_bytes(spec.bytes),
        style::small(),
        dim(p.text2),
    );
    ui.add_space(2.0);
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// 进度条 + 一行说明
fn progress(ui: &mut egui::Ui, phase: &Phase) {
    let p = palette(ui.ctx());
    let small = |text: String, color| RichText::new(text).font(style::small()).color(color);
    match phase {
        Phase::Connecting(host) => {
            ui.add(egui::ProgressBar::new(0.0).desired_height(6.0).animate(true));
            let to = if host.is_empty() { String::new() } else { format!(" {host}") };
            ui.label(small(format!("正在连接{to}…"), p.text2));
        }
        Phase::Retrying { host, reason } => {
            ui.add(egui::ProgressBar::new(0.0).desired_height(6.0).animate(true));
            ui.label(small(format!("网络中断({reason}),正在重连 {host},已下载的部分不会丢…"), p.warn));
        }
        Phase::Downloading { host, done, total, speed } => {
            let frac = total.map(|t| *done as f32 / t.max(1) as f32).unwrap_or(0.0);
            ui.add(egui::ProgressBar::new(frac).desired_height(6.0).fill(p.accent));
            let size = match total {
                Some(t) => format!("{} / {}", human_bytes(*done), human_bytes(*t)),
                None => human_bytes(*done),
            };
            let eta = match total {
                Some(t) if *speed > 1.0 => format!(" · 还要 {}", duration(((t - done) as f64 / speed) as u64)),
                _ => String::new(),
            };
            ui.label(small(format!("{size} · {}/s{eta} · 来自 {host}", human_bytes(*speed as u64)), p.text2));
        }
        Phase::Verifying => {
            ui.add(egui::ProgressBar::new(1.0).desired_height(6.0).fill(p.accent));
            ui.label(small("正在校验…".into(), p.text2));
        }
        Phase::Failed(msg) => {
            ui.label(small(msg.clone(), p.error));
        }
        Phase::Cancelled => {
            ui.label(small("已取消。已下载的部分会保留,下次接着下".into(), p.text3));
        }
        Phase::Done(_) => {}
    }
}

/// 「45 秒」「3 分钟」「1 小时 5 分」
fn duration(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs} 秒"),
        60..=3599 => format!("{} 分钟", secs.div_ceil(60)),
        _ => format!("{} 小时 {} 分", secs / 3600, secs % 3600 / 60),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn durations_read_naturally() {
        assert_eq!(super::duration(45), "45 秒");
        assert_eq!(super::duration(61), "2 分钟");
        assert_eq!(super::duration(3900), "1 小时 5 分");
    }
}
