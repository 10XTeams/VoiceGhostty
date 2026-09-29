//! VoiceGhostty 核心库。与平台、GUI 无关的逻辑都放这里,便于单测和跨平台复用。

pub mod claude;
pub mod config;
pub mod nl2command;
pub mod ollama;
pub mod paths;
pub mod settings;
pub mod shell;
pub mod theme;

pub use config::{Config, LlmProvider, VoiceConfig, VoiceEngineKind};
pub use nl2command::{NlError, Translation};
pub use settings::Settings;
pub use theme::{Rgb, Theme};
