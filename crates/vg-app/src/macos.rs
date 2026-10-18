//! macOS 专属修补。
//!
//! winit 自带的菜单把「退出」(⌘Q)直接接到 `terminate:`,进程立刻结束,窗口收不到任何关闭请求,
//! 我们的退出确认也就没机会弹出来。这里把该菜单项的动作改成 `performClose:`:它发给第一响应者
//! (也就是当前窗口),走 `windowShouldClose:` → winit 的 CloseRequested → App 里的确认框,
//! 和点窗口左上角的 ✕ 完全是同一条路。最后一个窗口关掉本来就会退出,所以语义不变。

use objc2::{sel, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSMenu};

/// 找到并改好返回 true;菜单还没建好返回 false,调用方下一帧再试
pub fn redirect_quit_to_close() -> bool {
    let Some(mtm) = MainThreadMarker::new() else { return false };
    let app = NSApplication::sharedApplication(mtm);
    let Some(menu) = app.mainMenu() else { return false };
    patch(&menu)
}

fn patch(menu: &NSMenu) -> bool {
    let mut found = false;
    for item in menu.itemArray().iter() {
        if item.action() == Some(sel!(terminate:)) {
            // SAFETY: performClose: 是 NSWindow 的标准动作,目标为 nil 时由响应链解析
            unsafe { item.setAction(Some(sel!(performClose:))) };
            found = true;
        }
        if let Some(sub) = item.submenu() {
            found |= patch(&sub);
        }
    }
    found
}
