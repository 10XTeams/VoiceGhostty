//! 设置窗口。GUI 设置存 settings.json,覆盖配置文件里的同名项;其余选项仍写在配置文件里。

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use egui::{pos2, vec2, Align2, Color32, Rect, RichText, Sense};

use vg_core::settings::{clamp_scrollback, FONT_SIZE_RANGE, HISTORY_SAVE_RANGE, SCROLLBACK_RANGE};
use vg_core::{LlmProvider, Theme, VoiceEngineKind};

use crate::app::VoiceGhosttyApp;
use crate::icons::{self, Icon};
use crate::kit;
use crate::style::{self, palette};
use crate::widgets;

const WIDTH: f32 = 470.0;

/// 设置窗口的页签
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Terminal,
    Appearance,
    Voice,
    Llm,
    Data,
}

impl Tab {
    const ALL: [Tab; 5] = [Tab::Terminal, Tab::Appearance, Tab::Voice, Tab::Llm, Tab::Data];

    fn label(self) -> &'static str {
        match self {
            Tab::Terminal => "终端",
            Tab::Appearance => "外观",
            Tab::Voice => "语音",
            Tab::Llm => "自然语言",
            Tab::Data => "数据",
        }
    }
}

const TAB_ID: &str = "settings-tab";

/// 直接切到第 `index` 个页签(状态栏快捷按钮「编辑」、截图测试)
pub(crate) fn select_tab(ctx: &egui::Context, index: usize) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(TAB_ID), Tab::ALL[index]));
}

impl VoiceGhosttyApp {
    pub(crate) fn settings_window(&mut self, ctx: &egui::Context) {
        let p = palette(ctx);
        let mut changed = false;
        let mut close = false;
        kit::window("设置").anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_width(WIDTH);
            close = kit::window_header(ui, "设置", Some("改动立即生效"));

            // 当前页签记在 egui 的临时存储里:关掉再开还在原来那页,重启回到第一页
            let tab_id = egui::Id::new(TAB_ID);
            let mut tab = ui.data(|d| d.get_temp::<Tab>(tab_id)).unwrap_or_default();
            let labels = Tab::ALL.map(Tab::label);
            let selected = Tab::ALL.iter().position(|t| *t == tab).unwrap_or(0);
            ui.vertical_centered(|ui| {
                if let Some(i) = kit::segmented(ui, &labels, selected) {
                    tab = Tab::ALL[i];
                    ui.data_mut(|d| d.insert_temp(tab_id, tab));
                }
            });
            ui.add_space(8.0);

            // 各页用同一个高度,切换时窗口不跳;每页各记各的滚动位置
            let height = (ctx.content_rect().height() - 230.0).clamp(160.0, 520.0);
            egui::ScrollArea::vertical().id_salt(tab.label()).max_height(height).auto_shrink([false, false]).show(ui, |ui| {
                ui.set_width(WIDTH);
                changed |= match tab {
                    Tab::Terminal => self.terminal_tab(ui),
                    Tab::Appearance => self.appearance_tab(ui),
                    Tab::Voice => self.voice_tab(ui),
                    Tab::Llm => self.llm_tab(ui),
                    Tab::Data => self.data_tab(ui),
                };
            });

            ui.add_space(8.0);
            let path = vg_core::paths::config_file();
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(RichText::new("麦克风、热词、按住时长、配色覆盖等其余选项写在").font(style::small()).color(p.text3));
                let link = ui.add(
                    egui::Label::new(RichText::new(vg_core::loader::tilde(&path)).font(style::small()).color(p.accent))
                        .sense(Sense::click()),
                );
                if link.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("在文件管理器里显示").clicked() {
                    if let Some(dir) = path.parent() {
                        widgets::open_in_file_manager(dir);
                    }
                }
            });
            // 版本 + 提交号:一眼确认跑的是哪个构建(市场版本跨构建不变,单看它分不出新旧)
            ui.add_space(6.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(format!("VoiceGhostty {}", crate::version::label())).font(style::small()).color(p.text3))
                    .on_hover_text(crate::version::detail());
            });
        });
        if close {
            self.show_settings = false;
        }
        if changed {
            let font_family = self.config.font_family.clone();
            self.reload_config(ctx);
            if self.config.font_family != font_family {
                crate::fonts::install(ctx, &self.config.font_family);
            }
        }
    }

    /// 「终端」页:提示音、通知、会话记录、目录、历史行数,以及加载文件夹
    fn terminal_tab(&mut self, ui: &mut egui::Ui) -> bool {
        let p = palette(ui.ctx());
        let mut changed = false;
        kit::card(ui, |ui| {
            kit::setting_row(ui, "完成提示音", Some("命令跑完、等你处理时响一声"), |ui| {
                if toggle(ui, &mut self.settings.done_sound).changed() {
                    self.monitor.chime_enabled.store(self.settings.done_sound, Ordering::Relaxed);
                    changed = true;
                }
                if kit::icon_button_sized(ui, Icon::Play, false, 26.0, 13.0).on_hover_text("试听").clicked() {
                    vg_voice::chime::play();
                }
            });
            kit::hairline(ui);
            kit::setting_row(ui, "后台通知", Some("窗口在后台时,窗格做完或等你输入就发一条系统通知"), |ui| {
                if toggle(ui, &mut self.settings.notify_in_background).changed() {
                    self.monitor.notify_enabled.store(self.settings.notify_in_background, Ordering::Relaxed);
                    changed = true;
                }
                if kit::icon_button_sized(ui, Icon::Play, false, 26.0, 13.0).on_hover_text("试一条").clicked() {
                    crate::notify::send("VoiceGhostty", "通知就长这样");
                }
            });
            kit::hairline(ui);
            kit::setting_row(ui, "保存会话记录", Some("对之后新开的窗格生效"), |ui| {
                if toggle(ui, &mut self.settings.save_history).changed() {
                    changed = true;
                }
                if kit::icon_button_sized(ui, Icon::Folder, false, 26.0, 13.0).on_hover_text("打开记录文件夹").clicked() {
                    widgets::open_in_file_manager(&vg_core::paths::history_dir());
                }
            });
            kit::hairline(ui);
            kit::setting_row(ui, "默认文件夹", Some("新标签页和启动时从这里开始;分屏跟随原窗格"), |ui| {
                let current = self.settings.default_dir.clone();
                let shown = current.as_deref().map(vg_core::loader::tilde).unwrap_or_else(|| "家目录".into());
                let full = current.as_deref().map(|d| d.display().to_string()).unwrap_or_default();
                if current.is_some() {
                    if kit::icon_button_sized(ui, Icon::Close, false, 26.0, 13.0).on_hover_text("恢复为家目录").clicked() {
                        self.settings.default_dir = None;
                        changed = true;
                    }
                }
                if kit::icon_button_sized(ui, Icon::Folder, false, 26.0, 13.0).on_hover_text("选择文件夹").clicked() {
                    let start = current.clone().unwrap_or_else(vg_core::paths::home_dir);
                    if let Some(picked) = rfd::FileDialog::new().set_title("选择新标签页的默认文件夹").set_directory(start).pick_folder() {
                        self.settings.default_dir = Some(picked);
                        changed = true;
                    }
                }
                let label = ui.add(egui::Label::new(RichText::new(shown).font(style::small()).color(p.text2)).truncate());
                if !full.is_empty() {
                    label.on_hover_text(full);
                }
            });
            kit::hairline(ui);
            kit::setting_row(ui, "项目目录", Some("✨ 常用项目里「新建项目」在这里建文件夹;没设就用默认文件夹"), |ui| {
                let current = self.settings.projects_dir.clone();
                let shown = current
                    .as_deref()
                    .map(vg_core::loader::tilde)
                    .unwrap_or_else(|| format!("同默认文件夹({})", vg_core::loader::tilde(&self.settings.projects_root())));
                let full = current.as_deref().map(|d| d.display().to_string()).unwrap_or_default();
                if current.is_some()
                    && kit::icon_button_sized(ui, Icon::Close, false, 26.0, 13.0).on_hover_text("恢复为跟随默认文件夹").clicked()
                {
                    self.settings.projects_dir = None;
                    changed = true;
                }
                if kit::icon_button_sized(ui, Icon::Folder, false, 26.0, 13.0).on_hover_text("选择文件夹").clicked() {
                    let start = self.settings.projects_root();
                    if let Some(picked) = rfd::FileDialog::new().set_title("选择新建项目的父目录").set_directory(start).pick_folder() {
                        self.settings.projects_dir = Some(picked);
                        changed = true;
                    }
                }
                let label = ui.add(egui::Label::new(RichText::new(shown).font(style::small()).color(p.text2)).truncate());
                if !full.is_empty() {
                    label.on_hover_text(full);
                }
            });
            kit::hairline(ui);
            kit::setting_row(ui, "历史行数", Some("每个窗格保留多少行滚出屏幕的输出"), |ui| {
                let mut lines = self.config.scrollback;
                let drag = egui::DragValue::new(&mut lines).range(SCROLLBACK_RANGE).speed(100.0).suffix(" 行");
                if ui.add_sized(vec2(96.0, 24.0), drag).changed() {
                    let lines = clamp_scrollback(lines);
                    self.settings.scrollback = Some(lines);
                    for pane in self.ws.panes.values() {
                        pane.session.set_scrollback(lines);
                    }
                    changed = true;
                }
            });
        });

        ui.add_space(10.0);
        kit::section(ui, "快捷按钮");
        if self.quick_buttons_section(ui) {
            changed = true;
        }

        ui.add_space(10.0);
        kit::section(ui, "加载文件夹");
        if self.library_section(ui) {
            changed = true;
        }
        changed
    }

    /// 「数据」页:运行数据存在哪、多久写一次,以及用量洞察
    fn data_tab(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        kit::section(ui, "定时保存");
        kit::card(ui, |ui| {
            kit::setting_row(ui, "会话记录写盘间隔", Some("每个窗格多久写一次记录文件,关窗格时会补写"), |ui| {
                let mut secs = self.settings.history_save_seconds;
                let drag = egui::DragValue::new(&mut secs).range(HISTORY_SAVE_RANGE).speed(1.0).suffix(" 秒");
                if ui.add_sized(vec2(96.0, 24.0), drag).changed() {
                    self.settings.history_save_seconds = secs;
                    for pane in self.ws.panes.values() {
                        pane.session.set_history_interval(secs);
                    }
                    changed = true;
                }
            });
        });

        ui.add_space(10.0);
        kit::section(ui, "运行数据");
        kit::card(ui, |ui| {
            let rows = [
                ("配置与设置", "settings.json 和配置文件", vg_core::paths::config_dir()),
                ("会话记录", "每个窗格一个 .log", vg_core::paths::history_dir()),
                ("窗格状态", "shell 钩子写入、每 0.5 秒读一次,决定状态灯颜色", vg_core::paths::run_dir()),
                ("用量台账", "用量洞察整理出的按月台账与扫描进度", vg_core::paths::usage_dir()),
                ("Claude 会话记录", "Claude Code 自己的文件,只读:状态栏用量条和用量洞察的数据源", vg_core::insights::claude::projects_root()),
            ];
            for (i, (title, hint, dir)) in rows.into_iter().enumerate() {
                if i > 0 {
                    kit::hairline(ui);
                }
                path_row(ui, title, hint, &dir);
            }
        });

        ui.add_space(10.0);
        kit::section(ui, "用量洞察");
        if self.insights_section(ui) {
            changed = true;
        }
        changed
    }

    /// 后台整理 Claude Code 用量记录:开关、周期、恢复「不再提示」、清除台账
    fn insights_section(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        kit::card(ui, |ui| {
            kit::setting_row(ui, "后台整理", Some("定时读 ~/.claude/projects 下的会话记录,整理成只有数字的台账;关掉后只在打开面板时整理"), |ui| {
                if toggle(ui, &mut self.settings.insights_enabled).changed() {
                    self.insights.set_background(self.settings.insights_enabled);
                    changed = true;
                }
            });
            kit::hairline(ui);
            kit::setting_row(ui, "整理周期", Some("Claude Code 约 30 天后清理会话记录,台账替你留着更久的历史"), |ui| {
                let mut m = self.settings.insights_scan_minutes;
                let drag = egui::DragValue::new(&mut m).range(5..=1440).speed(5.0).suffix(" 分钟");
                if ui.add_sized(vec2(96.0, 24.0), drag).changed() {
                    self.settings.insights_scan_minutes = m;
                    self.insights.set_period(m);
                    changed = true;
                }
            });
            if !self.settings.insights_muted_rules.is_empty() {
                kit::hairline(ui);
                let n = self.settings.insights_muted_rules.len();
                kit::setting_row(ui, "已隐藏的建议", Some(&format!("{n} 类建议点过「不再提示」")), |ui| {
                    if kit::button(ui, "恢复", kit::Kind::Secondary, true).clicked() {
                        self.settings.insights_muted_rules.clear();
                        changed = true;
                    }
                });
            }
            kit::hairline(ui);
            kit::setting_row(ui, "清除用量数据", Some("删除台账,下次整理时从现存的会话记录重建(已被 Claude Code 清理的部分补不回来)"), |ui| {
                if kit::button(ui, "清除", kit::Kind::Danger, true).clicked() {
                    self.insights.clear();
                }
                if kit::icon_button_sized(ui, Icon::Folder, false, 26.0, 13.0).on_hover_text("打开台账文件夹").clicked() {
                    widgets::open_in_file_manager(&vg_core::paths::usage_dir());
                }
            });
        });
        changed
    }

    /// 「外观」页:主题、字体、字号和命令输入颜色
    fn appearance_tab(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        kit::card(ui, |ui| {
            kit::setting_row(ui, "主题", None, |ui| {
                let theme_id = self.config.theme_name.clone();
                ui.spacing_mut().item_spacing.x = 6.0;
                // right_to_left:倒着放,屏幕上从左到右是 ALL 的顺序
                for t in Theme::ALL.iter().rev() {
                    if theme_swatch(ui, t, t.id == theme_id).clicked() {
                        self.settings.theme = Some(t.id.into());
                        changed = true;
                    }
                }
            });
            kit::hairline(ui);
            // 字体名在文件系统里找,开销不小:只在名字变了时查一次,结果记在临时存储里
            let family = self.config.font_family.clone();
            let found_id = egui::Id::new(("font-found", family.clone()));
            let found = ui.data(|d| d.get_temp::<bool>(found_id)).unwrap_or_else(|| {
                let ok = crate::fonts::font_available(&family);
                ui.data_mut(|d| d.insert_temp(found_id, ok));
                ok
            });
            let font_hint = if found { "如 JetBrains Mono,留空用默认" } else { "找不到这个字体,用的是默认" };
            kit::setting_row(ui, "终端字体", Some(font_hint), |ui| {
                if let Some(v) = text_field(ui, "font-family", &family, "默认等宽字体", false, 150.0) {
                    self.settings.font_family = Some(v);
                    changed = true;
                }
            });
            kit::hairline(ui);
            kit::setting_row(ui, "字号", Some("⌘+ / ⌘- 临时缩放,⌘0 回到这里的字号"), |ui| {
                let mut size = self.config.font_size;
                let drag = egui::DragValue::new(&mut size).range(FONT_SIZE_RANGE).speed(0.2).fixed_decimals(0).suffix(" pt");
                if ui.add_sized(vec2(96.0, 24.0), drag).changed() {
                    let size = size.round();
                    self.settings.font_size = Some(size);
                    self.font_size = size;
                    changed = true;
                }
            });
            kit::hairline(ui);
            kit::setting_row(ui, "命令输入颜色", Some("正在输入的命令行用这个颜色"), |ui| {
                let mut color = vg_core::Rgb::parse(&self.settings.input_color)
                    .map(|c| Color32::from_rgb(c.0, c.1, c.2))
                    .unwrap_or(Color32::LIGHT_BLUE);
                if ui.color_edit_button_srgba(&mut color).changed() {
                    self.settings.input_color = vg_core::Rgb(color.r(), color.g(), color.b()).to_hex();
                    changed = true;
                }
            });
        });
        changed
    }

    /// 「语音」页:引擎、模型、说话键等,以及语音词典
    fn voice_tab(&mut self, ui: &mut egui::Ui) -> bool {
        let p = palette(ui.ctx());
        let mut changed = false;
        kit::card(ui, |ui| {
            kit::setting_row(ui, "语音引擎", Some(engine_hint(self.config.voice.engine)), |ui| {
                let current = self.config.voice.engine;
                egui::ComboBox::from_id_salt("engine").width(150.0).selected_text(engine_label(current)).show_ui(ui, |ui| {
                    for kind in [VoiceEngineKind::Auto, VoiceEngineKind::Whisper, VoiceEngineKind::Http, VoiceEngineKind::Command, VoiceEngineKind::Off] {
                        if ui.selectable_label(current == kind, engine_label(kind)).clicked() {
                            self.settings.voice_engine = Some(kind.as_str().into());
                            changed = true;
                        }
                    }
                });
            });
            match self.config.voice.engine {
                VoiceEngineKind::Http => {
                    let v = self.config.voice.clone();
                    let defaults = vg_core::VoiceConfig::default();
                    kit::hairline(ui);
                    kit::setting_row(ui, "接口地址", Some("文本框清空就回到配置文件 / 默认值"), |ui| {
                        if let Some(x) = text_field(ui, "voice-api-url", &v.api_url, &defaults.api_url, false, 200.0) {
                            self.settings.voice_api_url = non_empty(x);
                            changed = true;
                        }
                    });
                    kit::hairline(ui);
                    kit::setting_row(ui, "API Key", Some("留空读环境变量;本机服务不用"), |ui| {
                        if let Some(x) = text_field(ui, "voice-api-key", &v.api_key, "sk-…", true, 200.0) {
                            self.settings.voice_api_key = non_empty(x);
                            changed = true;
                        }
                    });
                    kit::hairline(ui);
                    kit::setting_row(ui, "模型", None, |ui| {
                        if let Some(x) = text_field(ui, "voice-api-model", &v.api_model, &defaults.api_model, false, 200.0) {
                            self.settings.voice_api_model = non_empty(x);
                            changed = true;
                        }
                    });
                }
                VoiceEngineKind::Command => {
                    let command = self.config.voice.command.clone();
                    kit::hairline(ui);
                    kit::setting_row(ui, "识别命令", Some("{wav} 换成录音文件、{lang} 换成语言,读标准输出"), |ui| {
                        if let Some(x) = text_field(ui, "voice-command", &command, "whisper-cli -nt -f {wav}", false, 200.0) {
                            self.settings.voice_command = non_empty(x);
                            changed = true;
                        }
                    });
                }
                _ => {}
            }
            if vg_voice::HAS_WHISPER {
                kit::hairline(ui);
                let model = match (self.model_download_progress(), self.config.voice.resolved_model()) {
                    (Some(progress), _) => progress,
                    (None, Some(path)) => {
                        let size = std::fs::metadata(&path).map(|m| vg_core::model_download::human_bytes(m.len())).unwrap_or_default();
                        format!("{} · {size}", path.file_name().unwrap_or_default().to_string_lossy())
                    }
                    (None, None) => "还没有模型,内置 whisper 用不了".to_string(),
                };
                kit::setting_row(ui, "whisper 模型", Some(&model), |ui| {
                    if kit::button(ui, "下载模型…", kit::Kind::Secondary, true).clicked() {
                        self.show_model_dialog = true;
                    }
                });
            }
            kit::hairline(ui);
            kit::setting_row(ui, "识别语言", None, |ui| {
                let lang = self.config.voice.language.clone();
                egui::ComboBox::from_id_salt("lang").width(150.0).selected_text(widgets::language_label(&lang)).show_ui(ui, |ui| {
                    for code in ["zh", "en", "auto"] {
                        if ui.selectable_label(lang == code, widgets::language_label(code)).clicked() {
                            self.settings.default_language = Some(code.into());
                            changed = true;
                        }
                    }
                });
            });
            kit::hairline(ui);
            kit::setting_row(ui, "自动标点", Some("主要口述 shell 命令时建议关掉"), |ui| {
                let mut punct = self.config.voice.punctuation;
                if toggle(ui, &mut punct).changed() {
                    self.settings.adds_punctuation = Some(punct);
                    changed = true;
                }
            });
            kit::hairline(ui);
            kit::setting_row(ui, "说话键", Some("按住它说话,点一下仍是这个键本来的功能;F13–F19 适合把 CapsLock 映射过去的人"), |ui| {
                let current = self.config.voice.ptt_key.clone();
                let shown = if current == "Space" { "空格".to_string() } else { current.clone() };
                egui::ComboBox::from_id_salt("ptt-key").width(150.0).selected_text(shown).show_ui(ui, |ui| {
                    for name in PTT_KEYS {
                        let label = if *name == "Space" { "空格" } else { name };
                        if ui.selectable_label(current == *name, label).clicked() {
                            self.settings.ptt_key = Some((*name).into());
                            changed = true;
                        }
                    }
                });
            });
            kit::hairline(ui);
            kit::setting_row(ui, "全局热键", Some("窗口在后台也能按住说话;在配置文件里写 voice-global-hotkey,如 ctrl+alt+space"), |ui| {
                let text = match (&self.global_hotkey_error, self.global_hotkey_label()) {
                    (Some(err), _) => RichText::new(err.as_str()).font(style::small()).color(p.error),
                    (None, Some(label)) => RichText::new(label).font(style::small()).color(p.text),
                    (None, None) => RichText::new("未设置").font(style::small()).color(p.text3),
                };
                ui.label(text);
            });
            kit::hairline(ui);
            kit::setting_row(ui, "实时字幕", Some("边说边在状态栏显示识别到的内容;只对本机引擎生效"), |ui| {
                let mut live = self.config.voice.live_caption;
                if toggle(ui, &mut live).changed() {
                    self.settings.live_caption = Some(live);
                    changed = true;
                }
            });
            kit::hairline(ui);
            kit::setting_row(ui, "语音命令", Some("整句说「清屏」「新标签」「第二个标签」时直接执行;完整列表见快捷键表"), |ui| {
                let mut on = self.config.voice.commands;
                if toggle(ui, &mut on).changed() {
                    self.settings.voice_commands = Some(on);
                    changed = true;
                }
            });
            kit::hairline(ui);
            kit::setting_row(ui, "口述符号", Some("听写时「点」「横杠」「斜杠」等换成 . - /;只在挨着英文数字时转换"), |ui| {
                let mut sym = self.config.voice.spoken_symbols;
                if toggle(ui, &mut sym).changed() {
                    self.settings.spoken_symbols = Some(sym);
                    changed = true;
                }
            });
        });

        ui.add_space(10.0);
        kit::section(ui, "语音词典");
        if self.replacements_section(ui) {
            changed = true;
        }
        changed
    }

    /// 「自然语言」页:说人话转命令用哪个模型
    fn llm_tab(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        let c = self.config.clone();
        let defaults = vg_core::Config::default();
        kit::card(ui, |ui| {
            kit::setting_row(ui, "模型提供方", Some(provider_hint(c.llm_provider)), |ui| {
                egui::ComboBox::from_id_salt("llm-provider").width(150.0).selected_text(provider_label(c.llm_provider)).show_ui(ui, |ui| {
                    for p in [LlmProvider::Auto, LlmProvider::Claude, LlmProvider::Ollama] {
                        if ui.selectable_label(c.llm_provider == p, provider_label(p)).clicked() {
                            self.settings.llm_provider = Some(p.as_str().into());
                            changed = true;
                        }
                    }
                });
            });
            kit::hairline(ui);
            kit::setting_row(ui, "带上屏幕内容", Some("把屏幕最后几十行和上一条命令一起发给模型,「刚才为什么报错」才有所指"), |ui| {
                let mut on = c.llm_screen_context;
                if toggle(ui, &mut on).changed() {
                    self.settings.llm_screen_context = Some(on);
                    changed = true;
                }
            });
            kit::hairline(ui);
            kit::setting_row(ui, "转写纠错", Some("先用本地 Ollama 修正识别错字再转命令;听写模式不受影响"), |ui| {
                let mut on = c.correction_enabled;
                if toggle(ui, &mut on).changed() {
                    self.settings.llm_correction = Some(on);
                    changed = true;
                }
            });
        });

        ui.add_space(10.0);
        kit::section(ui, "Claude");
        kit::card(ui, |ui| {
            kit::setting_row(ui, "API Key", Some("留空则用环境变量 ANTHROPIC_API_KEY"), |ui| {
                if let Some(x) = text_field(ui, "llm-api-key", &c.llm_api_key, "sk-ant-…", true, 230.0) {
                    self.settings.llm_api_key = non_empty(x);
                    changed = true;
                }
            });
            kit::hairline(ui);
            kit::setting_row(ui, "模型", None, |ui| {
                if let Some(x) = text_field(ui, "llm-model", &c.llm_model, &defaults.llm_model, false, 230.0) {
                    self.settings.llm_model = non_empty(x);
                    changed = true;
                }
            });
        });

        ui.add_space(10.0);
        kit::section(ui, "Ollama(本地)");
        kit::card(ui, |ui| {
            kit::setting_row(ui, "地址", None, |ui| {
                if let Some(x) = text_field(ui, "llm-local-url", &c.llm_local_url, &defaults.llm_local_url, false, 230.0) {
                    self.settings.llm_local_url = non_empty(x);
                    changed = true;
                }
            });
            kit::hairline(ui);
            kit::setting_row(ui, "模型", Some("转命令和纠错都用它"), |ui| {
                if let Some(x) = text_field(ui, "llm-local-model", &c.llm_local_model, &defaults.llm_local_model, false, 230.0) {
                    self.settings.llm_local_model = non_empty(x);
                    changed = true;
                }
            });
        });
        changed
    }

    /// 状态栏快捷按钮:名称 + 输入的内容 + 是否回车执行。整个应用一套
    fn quick_buttons_section(&mut self, ui: &mut egui::Ui) -> bool {
        let p = palette(ui.ctx());
        let mut changed = false;
        let mut action: Option<(usize, bool)> = None; // (下标, true=删除 / false=上移)
        kit::card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.label(
                RichText::new("显示在底部状态栏,所有窗口共用一套。点一下把内容输入到当前窗格;勾了「回车」的会直接执行,右键按钮可回到这里编辑")
                    .font(style::small())
                    .color(p.text3),
            );
            ui.add_space(2.0);
            for (i, b) in self.settings.quick_buttons.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let content_w = (ui.available_width() - 84.0 - 54.0 - 24.0 * 2.0 - 6.0 * 4.0).max(80.0);
                    if ui.add_sized(vec2(84.0, 24.0), egui::TextEdit::singleline(&mut b.label).hint_text("名称")).changed() {
                        changed = true;
                    }
                    let edit = egui::TextEdit::singleline(&mut b.text).hint_text("输入的内容,如 git status");
                    if ui.add_sized(vec2(content_w, 24.0), edit).changed() {
                        changed = true;
                    }
                    if ui.checkbox(&mut b.run, "回车").on_hover_text("输入后直接回车执行").changed() {
                        changed = true;
                    }
                    if i > 0 {
                        if kit::icon_button_sized(ui, Icon::ArrowUp, false, 24.0, 13.0).on_hover_text("上移").clicked() {
                            action = Some((i, false));
                        }
                    } else {
                        // 占住「上移」的位置(含间距),删除按钮才和下面几行对齐
                        ui.add_space(24.0 + ui.spacing().item_spacing.x);
                    }
                    if kit::icon_button_sized(ui, Icon::Trash, false, 24.0, 13.0).on_hover_text("移除").clicked() {
                        action = Some((i, true));
                    }
                });
            }
            if kit::button(ui, "添加按钮…", kit::Kind::Secondary, true).clicked() {
                self.settings.quick_buttons.push(vg_core::settings::QuickButton::default());
                changed = true;
            }
        });
        match action {
            Some((i, true)) => {
                self.settings.quick_buttons.remove(i);
                changed = true;
            }
            Some((i, false)) => {
                self.settings.quick_buttons.swap(i - 1, i);
                changed = true;
            }
            None => {}
        }
        changed
    }

    /// 设置页的个人词典:每行「听成的 → 想要的」,两种语音模式都先按它替换
    fn replacements_section(&mut self, ui: &mut egui::Ui) -> bool {
        let p = palette(ui.ctx());
        let mut rules = self.config.voice.replacements.clone();
        let mut changed = false;
        let mut remove: Option<usize> = None;
        kit::card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.label(
                RichText::new("老被听错的词写在左边,想要的写在右边;长的先匹配,听写和自然语言都生效")
                    .font(style::small())
                    .color(p.text3),
            );
            ui.add_space(2.0);
            for (i, (from, to)) in rules.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let w = (ui.available_width() - 26.0 - 18.0 - 12.0) / 2.0;
                    if ui.add_sized(vec2(w, 24.0), egui::TextEdit::singleline(from).hint_text("听成的")).changed() {
                        changed = true;
                    }
                    ui.label(RichText::new("→").color(p.text3));
                    if ui.add_sized(vec2(w, 24.0), egui::TextEdit::singleline(to).hint_text("想要的")).changed() {
                        changed = true;
                    }
                    if kit::icon_button_sized(ui, Icon::Trash, false, 24.0, 13.0).on_hover_text("移除").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if kit::button(ui, "添加一条…", kit::Kind::Secondary, true).clicked() {
                rules.push((String::new(), String::new()));
                changed = true;
            }
        });
        if let Some(i) = remove {
            rules.remove(i);
            changed = true;
        }
        if changed {
            self.settings.voice_replacements = Some(rules);
        }
        changed
    }

    /// 设置页的"加载文件夹"列表:顺序决定重名时谁优先
    fn library_section(&mut self, ui: &mut egui::Ui) -> bool {
        let p = palette(ui.ctx());
        let mut dirs = self.config.skill_library_dirs.clone();
        let mut changed = false;
        let mut action: Option<(usize, bool)> = None; // (下标, true=删除 / false=上移)
        kit::card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.label(
                RichText::new("加载器从这些文件夹识别技能、Agent、插件;同名时靠上的优先")
                    .font(style::small())
                    .color(p.text3),
            );
            ui.add_space(2.0);
            for (i, dir) in dirs.iter().enumerate() {
                let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
                let hovered = resp.hovered();
                if hovered {
                    ui.painter().rect_filled(row.expand2(vec2(6.0, 0.0)), 6.0, p.hover);
                }
                let icon = Rect::from_center_size(pos2(row.min.x + 9.0, row.center().y), vec2(16.0, 16.0));
                icons::paint(ui.painter(), icon, 14.0, Icon::Folder, p.text2);
                ui.painter().text(
                    pos2(row.min.x + 26.0, row.center().y),
                    Align2::LEFT_CENTER,
                    vg_core::loader::tilde(dir),
                    style::body(),
                    p.text,
                );
                let order = format!("{}", i + 1);
                let mut x = row.max.x;
                let mut btn = |icon: Icon, tip: &str, danger: bool, ui: &mut egui::Ui| -> bool {
                    let r = Rect::from_center_size(pos2(x - 12.0, row.center().y), vec2(24.0, 24.0));
                    x -= 26.0;
                    let b = ui.interact(r, resp.id.with(tip), Sense::click());
                    if b.hovered() {
                        ui.painter().rect_filled(r, 5.0, p.pressed);
                    }
                    let color = if b.hovered() && danger { p.error } else if b.hovered() { p.text } else { p.text3 };
                    icons::paint(ui.painter(), r, 13.0, icon, color);
                    b.on_hover_text(tip).clicked()
                };
                if hovered || ui.rect_contains_pointer(row) {
                    if btn(Icon::Trash, "移除", true, ui) {
                        action = Some((i, true));
                    }
                    if i > 0 && btn(Icon::ArrowUp, "上移(优先级更高)", false, ui) {
                        action = Some((i, false));
                    }
                } else {
                    ui.painter().text(pos2(row.max.x - 4.0, row.center().y), Align2::RIGHT_CENTER, order, crate::fonts::medium(11.0), p.text3);
                }
                resp.on_hover_text(dir.display().to_string());
            }
            if !dirs.is_empty() {
                ui.add_space(2.0);
            }
            if kit::button(ui, "添加文件夹…", kit::Kind::Secondary, true).clicked() {
                let start = dirs.last().cloned().unwrap_or_else(vg_core::paths::home_dir);
                if let Some(picked) = rfd::FileDialog::new()
                    .set_title("选择要加载的文件夹 —— 其中的插件、技能、Agent 会自动识别(可多选)")
                    .set_directory(start)
                    .pick_folders()
                {
                    let added: Vec<PathBuf> = picked.into_iter().filter(|p| !dirs.contains(p)).collect();
                    if !added.is_empty() {
                        dirs.extend(added.iter().cloned());
                        self.library_notice = widgets::duplicate_notice(&added, &dirs);
                        changed = true;
                    }
                }
            }
            if let Some(n) = &self.library_notice {
                ui.label(RichText::new(n).font(style::small()).color(p.warn));
            }
        });
        match action {
            Some((i, true)) => {
                dirs.remove(i);
                self.library_notice = None; // 列表变了,之前的提示未必还成立
                changed = true;
            }
            Some((i, false)) => {
                dirs.swap(i - 1, i);
                changed = true;
            }
            None => {}
        }
        if changed {
            self.settings.skill_library_dirs = Some(dirs);
        }
        changed
    }
}

fn engine_label(kind: VoiceEngineKind) -> &'static str {
    match kind {
        VoiceEngineKind::Auto => "自动",
        VoiceEngineKind::Whisper => "内置 whisper",
        VoiceEngineKind::Http => "HTTP 接口",
        VoiceEngineKind::Command => "外部命令",
        VoiceEngineKind::Off => "关闭",
    }
}

fn engine_hint(kind: VoiceEngineKind) -> &'static str {
    match kind {
        VoiceEngineKind::Auto => "按可用性挑:内置 whisper → 外部命令 → HTTP",
        VoiceEngineKind::Whisper => "本地离线,模型放在 models/ 目录",
        VoiceEngineKind::Http => "OpenAI 兼容的转写接口",
        VoiceEngineKind::Command => "调用 voice-command 配置的程序",
        VoiceEngineKind::Off => "说话键恢复成普通按键",
    }
}

/// 设置页可选的说话键(egui 按键名)。都是不会产生字符的键,点按补发时不会和文字输入打架
const PTT_KEYS: &[&str] = &["Space", "Insert", "F13", "F14", "F15", "F16", "F17", "F18", "F19"];

/// iOS 风格的开关
fn provider_label(p: LlmProvider) -> &'static str {
    match p {
        LlmProvider::Auto => "自动",
        LlmProvider::Claude => "Claude",
        LlmProvider::Ollama => "Ollama(本地)",
    }
}

fn provider_hint(p: LlmProvider) -> &'static str {
    match p {
        LlmProvider::Auto => "有 Claude API Key 用 Claude,否则用本地 Ollama",
        LlmProvider::Claude => "需要 API Key",
        LlmProvider::Ollama => "不联网,需要本机开着 Ollama",
    }
}

/// 单行文本设置:编辑中的内容放在临时存储里,失焦(或回车)时才提交,
/// 免得每敲一个字就重读配置、存一次文件。返回 Some(新值) 表示要提交;空串表示「回到配置文件 / 默认值」
fn text_field(ui: &mut egui::Ui, key: &str, value: &str, placeholder: &str, password: bool, width: f32) -> Option<String> {
    let id = egui::Id::new(("settings-text", key));
    let mut buf = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| value.to_string());
    let edit = egui::TextEdit::singleline(&mut buf).id(id.with("edit")).hint_text(placeholder).password(password);
    let resp = ui.add_sized(vec2(width, 24.0), edit);
    if resp.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(id));
        let v = buf.trim().to_string();
        return (v != value).then_some(v);
    }
    if resp.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, buf));
    }
    None
}

/// 文本框清空 = 不在界面里设,回到配置文件 / 默认值
fn non_empty(v: String) -> Option<String> {
    (!v.is_empty()).then_some(v)
}

/// 一行「名称 + 路径 + 打开文件夹」
fn path_row(ui: &mut egui::Ui, title: &str, hint: &str, dir: &std::path::Path) {
    let p = palette(ui.ctx());
    kit::setting_row(ui, title, Some(hint), |ui| {
        if kit::icon_button_sized(ui, Icon::Folder, false, 26.0, 13.0).on_hover_text("在文件管理器里打开").clicked() {
            let _ = std::fs::create_dir_all(dir);
            widgets::open_in_file_manager(dir);
        }
        ui.add(egui::Label::new(RichText::new(vg_core::loader::tilde(dir)).font(style::small()).color(p.text2)).truncate())
            .on_hover_text(dir.display().to_string());
    });
}

fn toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let p = palette(ui.ctx());
    let size = vec2(34.0, 20.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(resp.id, *on, 0.14);
    let track = style::mix(p.pressed, p.accent, t);
    ui.painter().rect_filled(rect, 10.0, track);
    let x = egui::lerp(rect.min.x + 10.0..=rect.max.x - 10.0, t);
    let knob = pos2(x, rect.center().y);
    ui.painter().circle_filled(knob + vec2(0.0, 0.6), 8.0, Color32::from_black_alpha(50));
    ui.painter().circle_filled(knob, 8.0, Color32::WHITE);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// 主题色块:终端底色 + 前景色的一道,选中描强调色
fn theme_swatch(ui: &mut egui::Ui, theme: &Theme, selected: bool) -> egui::Response {
    let p = palette(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(30.0, 22.0), Sense::click());
    let c = |rgb: vg_core::Rgb| Color32::from_rgb(rgb.0, rgb.1, rgb.2);
    let inner = rect.shrink(if selected { 2.5 } else { 1.0 });
    ui.painter().rect_filled(inner, 5.0, c(theme.background));
    for (i, k) in [1usize, 2, 4].iter().enumerate() {
        let r = Rect::from_min_size(pos2(inner.min.x + 5.0 + i as f32 * 6.0, inner.center().y - 2.0), vec2(4.0, 4.0));
        ui.painter().rect_filled(r, 1.0, c(theme.ansi[*k]));
    }
    ui.painter().hline(inner.min.x + 5.0..=inner.max.x - 5.0, inner.center().y + 5.0, egui::Stroke::new(1.5, c(theme.foreground)));
    let stroke = if selected {
        egui::Stroke::new(1.5, p.accent)
    } else {
        egui::Stroke::new(1.0, if resp.hovered() { p.text3 } else { p.border })
    };
    ui.painter().rect_stroke(rect.shrink(if selected { 0.75 } else { 0.5 }), 6.5, stroke, egui::StrokeKind::Middle);
    resp.on_hover_text(theme.display_name).on_hover_cursor(egui::CursorIcon::PointingHand)
}
