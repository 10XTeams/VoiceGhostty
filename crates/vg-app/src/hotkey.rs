//! 全局热键(`voice-global-hotkey`):窗口在后台也能按住说话,识别结果照样落到活动窗格。
//!
//! macOS 用 Carbon 的 `RegisterEventHotKey`:系统级独占这个组合键(别的 App 收不到),不需要辅助功能权限,
//! 按下 / 松开各有事件。回调在主线程的 run loop 上触发,eframe 的事件循环就是这个 run loop,所以回调里
//! 直接把事件塞进通道并请求重绘即可。其他平台暂不支持:对应的 crate 这次没拉下来,而且 X11 / Wayland /
//! Windows 三套 API 各不相同,留到以后。
//!
//! 组合键写法:`ctrl+alt+space`、`cmd+shift+f13`、`f19`,修饰键 ctrl / alt(option)/ cmd / shift,
//! 主键是字母、数字、space、f1–f20、方向键等(见 [`key_code`])。

use std::sync::mpsc::{self, Receiver};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    Pressed,
    Released,
}

pub const CMD: u32 = 0x100;
pub const SHIFT: u32 = 0x200;
pub const ALT: u32 = 0x800;
pub const CTRL: u32 = 0x1000;

/// 解析 `ctrl+alt+space` 这样的写法。空串返回 None(未设置);写错了返回 Err 给状态栏显示
pub fn parse(spec: &str) -> Result<Option<(u32, u32)>, String> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Ok(None);
    }
    let mut modifiers = 0u32;
    let mut key = None;
    for part in spec.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "⌃" => modifiers |= CTRL,
            "alt" | "option" | "opt" | "⌥" => modifiers |= ALT,
            "cmd" | "command" | "super" | "meta" | "⌘" => modifiers |= CMD,
            "shift" | "⇧" => modifiers |= SHIFT,
            other => {
                if key.is_some() {
                    return Err(format!("热键「{spec}」里有两个主键"));
                }
                key = Some(key_code(other).ok_or_else(|| format!("热键「{spec}」里不认识的键「{other}」"))?);
            }
        }
    }
    let Some(code) = key else { return Err(format!("热键「{spec}」缺主键(如 space、f13)")) };
    // 单独一个字母 / 数字做全局热键会把它从所有 App 里抢走,必须带修饰键;功能键可以单独用
    if modifiers == 0 && !is_function_key(code) {
        return Err(format!("热键「{spec}」需要至少一个修饰键(ctrl / alt / cmd / shift),除非用 F 键"));
    }
    Ok(Some((code, modifiers)))
}

fn is_function_key(code: u32) -> bool {
    matches!(code, 122 | 120 | 99 | 118 | 96 | 97 | 98 | 100 | 101 | 109 | 103 | 111 | 105 | 107 | 113 | 106 | 64 | 79 | 80 | 90)
}

/// 美式键盘布局下的 Carbon 虚拟键码(字母 / 数字按物理位置,与当前输入法无关)
pub fn key_code(name: &str) -> Option<u32> {
    Some(match name {
        "a" => 0, "s" => 1, "d" => 2, "f" => 3, "h" => 4, "g" => 5, "z" => 6, "x" => 7, "c" => 8, "v" => 9,
        "b" => 11, "q" => 12, "w" => 13, "e" => 14, "r" => 15, "y" => 16, "t" => 17,
        "1" => 18, "2" => 19, "3" => 20, "4" => 21, "6" => 22, "5" => 23, "=" | "equal" => 24, "9" => 25,
        "7" => 26, "-" | "minus" => 27, "8" => 28, "0" => 29, "]" => 30, "o" => 31, "u" => 32, "[" => 33,
        "i" => 34, "p" => 35, "enter" | "return" => 36, "l" => 37, "j" => 38, "'" => 39, "k" => 40,
        ";" => 41, "\\" => 42, "," => 43, "/" => 44, "n" => 45, "m" => 46, "." => 47, "tab" => 48,
        "space" => 49, "`" => 50, "backspace" | "delete" => 51, "esc" | "escape" => 53,
        "f1" => 122, "f2" => 120, "f3" => 99, "f4" => 118, "f5" => 96, "f6" => 97, "f7" => 98, "f8" => 100,
        "f9" => 101, "f10" => 109, "f11" => 103, "f12" => 111, "f13" => 105, "f14" => 107, "f15" => 113,
        "f16" => 106, "f17" => 64, "f18" => 79, "f19" => 80, "f20" => 90,
        "home" => 115, "pageup" => 116, "forwarddelete" => 117, "end" => 119, "pagedown" => 121,
        "left" => 123, "right" => 124, "down" => 125, "up" => 126,
        _ => return None,
    })
}

/// 把配置里的写法整理成 macOS 菜单风格的标签(`⌃⌥Space`)
pub fn pretty(spec: &str) -> String {
    let mut mods = String::new();
    let mut key = String::new();
    for part in spec.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods.push('⌃'),
            "alt" | "option" | "opt" => mods.push('⌥'),
            "shift" => mods.push('⇧'),
            "cmd" | "command" | "super" | "meta" => mods.push('⌘'),
            other => {
                let mut c = other.chars();
                key = match c.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
                    None => String::new(),
                };
            }
        }
    }
    format!("{mods}{key}")
}

#[cfg(target_os = "macos")]
mod platform {
    use super::HotkeyEvent;
    use std::ffi::c_void;
    use std::sync::mpsc::Sender;

    type OSStatus = i32;
    type EventRef = *mut c_void;
    type EventHandlerCallRef = *mut c_void;
    type EventTargetRef = *mut c_void;
    type EventHotKeyRef = *mut c_void;
    type EventHandlerRef = *mut c_void;
    type EventHandlerUPP = unsafe extern "C" fn(EventHandlerCallRef, EventRef, *mut c_void) -> OSStatus;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct EventHotKeyID {
        signature: u32,
        id: u32,
    }
    #[repr(C)]
    struct EventTypeSpec {
        event_class: u32,
        event_kind: u32,
    }

    const EVENT_CLASS_KEYBOARD: u32 = 0x6B65_7962; // 'keyb'
    const EVENT_HOT_KEY_PRESSED: u32 = 5;
    const EVENT_HOT_KEY_RELEASED: u32 = 6;
    const PARAM_DIRECT_OBJECT: u32 = 0x2D2D_2D2D; // '----'
    const TYPE_EVENT_HOT_KEY_ID: u32 = 0x686B_6964; // 'hkid'
    const SIGNATURE: u32 = 0x5647_686B; // 'VGhk'

    #[link(name = "Carbon", kind = "framework")]
    extern "C" {
        fn GetApplicationEventTarget() -> EventTargetRef;
        fn InstallEventHandler(
            target: EventTargetRef,
            handler: EventHandlerUPP,
            num_types: u32,
            list: *const EventTypeSpec,
            user_data: *mut c_void,
            out: *mut EventHandlerRef,
        ) -> OSStatus;
        fn RemoveEventHandler(handler: EventHandlerRef) -> OSStatus;
        fn RegisterEventHotKey(
            key_code: u32,
            modifiers: u32,
            id: EventHotKeyID,
            target: EventTargetRef,
            options: u32,
            out: *mut EventHotKeyRef,
        ) -> OSStatus;
        fn UnregisterEventHotKey(hotkey: EventHotKeyRef) -> OSStatus;
        fn GetEventParameter(
            event: EventRef,
            name: u32,
            desired_type: u32,
            actual_type: *mut u32,
            size: u32,
            actual_size: *mut u32,
            data: *mut c_void,
        ) -> OSStatus;
        fn GetEventKind(event: EventRef) -> u32;
    }

    struct Shared {
        tx: Sender<HotkeyEvent>,
        ctx: egui::Context,
    }

    unsafe extern "C" fn handler(_call: EventHandlerCallRef, event: EventRef, user: *mut c_void) -> OSStatus {
        let shared = &*(user as *const Shared);
        let mut id = EventHotKeyID { signature: 0, id: 0 };
        let status = GetEventParameter(
            event,
            PARAM_DIRECT_OBJECT,
            TYPE_EVENT_HOT_KEY_ID,
            std::ptr::null_mut(),
            std::mem::size_of::<EventHotKeyID>() as u32,
            std::ptr::null_mut(),
            &mut id as *mut EventHotKeyID as *mut c_void,
        );
        if status != 0 || id.signature != SIGNATURE {
            return 0;
        }
        let ev = match GetEventKind(event) {
            EVENT_HOT_KEY_PRESSED => HotkeyEvent::Pressed,
            EVENT_HOT_KEY_RELEASED => HotkeyEvent::Released,
            _ => return 0,
        };
        let _ = shared.tx.send(ev);
        shared.ctx.request_repaint();
        0
    }

    pub struct Registration {
        hotkey: EventHotKeyRef,
        handler: EventHandlerRef,
        shared: *mut Shared,
    }

    impl Registration {
        pub fn new(key_code: u32, modifiers: u32, tx: Sender<HotkeyEvent>, ctx: egui::Context) -> Result<Registration, String> {
            let shared = Box::into_raw(Box::new(Shared { tx, ctx }));
            let types = [
                EventTypeSpec { event_class: EVENT_CLASS_KEYBOARD, event_kind: EVENT_HOT_KEY_PRESSED },
                EventTypeSpec { event_class: EVENT_CLASS_KEYBOARD, event_kind: EVENT_HOT_KEY_RELEASED },
            ];
            let mut handler_ref: EventHandlerRef = std::ptr::null_mut();
            let mut hotkey_ref: EventHotKeyRef = std::ptr::null_mut();
            // SAFETY:Carbon 的 C 接口;shared 在 RemoveEventHandler 之后才释放
            unsafe {
                let target = GetApplicationEventTarget();
                let st = InstallEventHandler(target, handler, types.len() as u32, types.as_ptr(), shared as *mut c_void, &mut handler_ref);
                if st != 0 {
                    drop(Box::from_raw(shared));
                    return Err(format!("InstallEventHandler 失败({st})"));
                }
                let id = EventHotKeyID { signature: SIGNATURE, id: 1 };
                let st = RegisterEventHotKey(key_code, modifiers, id, target, 0, &mut hotkey_ref);
                if st != 0 {
                    RemoveEventHandler(handler_ref);
                    drop(Box::from_raw(shared));
                    // -9878 = eventHotKeyExistsErr:被别的 App 占了
                    let why = if st == -9878 { "已被其他程序占用" } else { "系统拒绝" };
                    return Err(format!("注册全局热键失败:{why}({st})"));
                }
            }
            Ok(Registration { hotkey: hotkey_ref, handler: handler_ref, shared })
        }
    }

    impl Drop for Registration {
        fn drop(&mut self) {
            // SAFETY:与 new 配对;先注销再释放回调数据
            unsafe {
                UnregisterEventHotKey(self.hotkey);
                RemoveEventHandler(self.handler);
                drop(Box::from_raw(self.shared));
            }
        }
    }
}

/// 一个已注册的全局热键;drop 即注销
pub struct GlobalHotkey {
    pub spec: String,
    pub rx: Receiver<HotkeyEvent>,
    #[cfg(target_os = "macos")]
    _reg: platform::Registration,
}

impl GlobalHotkey {
    /// 按配置注册。空配置返回 Ok(None)
    pub fn register(spec: &str, ctx: &egui::Context) -> Result<Option<GlobalHotkey>, String> {
        let Some((code, mods)) = parse(spec)? else { return Ok(None) };
        let (tx, rx) = mpsc::channel();
        #[cfg(target_os = "macos")]
        {
            let reg = platform::Registration::new(code, mods, tx, ctx.clone())?;
            Ok(Some(GlobalHotkey { spec: spec.trim().to_string(), rx, _reg: reg }))
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (code, mods, tx, ctx, rx);
            Err("全局热键目前只支持 macOS".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_combos() {
        assert_eq!(parse(""), Ok(None));
        assert_eq!(parse("ctrl+alt+space"), Ok(Some((49, CTRL | ALT))));
        assert_eq!(parse("Cmd + Shift + F13"), Ok(Some((105, CMD | SHIFT))));
        assert_eq!(parse("f19"), Ok(Some((80, 0))));
        assert!(parse("space").is_err()); // 裸空格会抢走所有 App 的空格
        assert!(parse("ctrl+banana").is_err());
        assert!(parse("ctrl+a+b").is_err());
        assert!(parse("ctrl").is_err());
    }

    #[test]
    fn pretty_labels() {
        assert_eq!(pretty("ctrl+alt+space"), "⌃⌥Space");
        assert_eq!(pretty("cmd+shift+f13"), "⌘⇧F13");
        assert_eq!(pretty("f19"), "F19");
    }
}
