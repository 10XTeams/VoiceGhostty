import AppKit
import SwiftTerm

/// 终端配色主题:原生前景/背景/光标 + 16 色 ANSI 调色板。
struct Theme: Identifiable, Equatable {
    let id: String          // 也是配置文件里 `theme =` 的取值
    let displayName: String
    let foreground: NSColor
    let background: NSColor
    let cursor: NSColor
    /// 必须正好 16 个(标准 ANSI 0-15),交给 SwiftTerm.installColors
    let ansi: [SwiftTerm.Color]

    static func == (lhs: Theme, rhs: Theme) -> Bool { lhs.id == rhs.id }

    // MARK: - 内置主题

    static let all: [Theme] = [dark, light, solarizedDark, dracula]

    static func named(_ id: String) -> Theme {
        all.first { $0.id == id } ?? dark
    }

    static let dark = Theme(
        id: "dark", displayName: "暗色",
        foreground: hexNS("#d4d4d4"), background: hexNS("#1e1e1e"), cursor: hexNS("#ffffff"),
        ansi: palette([
            "#1e1e1e", "#cd3131", "#0dbc79", "#e5e510", "#2472c8", "#bc3fbc", "#11a8cd", "#e5e5e5",
            "#666666", "#f14c4c", "#23d18b", "#f5f543", "#3b8eea", "#d670d6", "#29b8db", "#ffffff",
        ]))

    static let light = Theme(
        id: "light", displayName: "亮色",
        foreground: hexNS("#1a1a1a"), background: hexNS("#ffffff"), cursor: hexNS("#000000"),
        ansi: palette([
            "#000000", "#c91b00", "#00c200", "#c7c400", "#0225c7", "#c930c7", "#00c5c7", "#c7c7c7",
            "#686868", "#ff6e67", "#5ffa68", "#fffc67", "#6871ff", "#ff77ff", "#60fdff", "#ffffff",
        ]))

    static let solarizedDark = Theme(
        id: "solarized-dark", displayName: "Solarized 暗",
        foreground: hexNS("#839496"), background: hexNS("#002b36"), cursor: hexNS("#93a1a1"),
        ansi: palette([
            "#073642", "#dc322f", "#859900", "#b58900", "#268bd2", "#d33682", "#2aa198", "#eee8d5",
            "#002b36", "#cb4b16", "#586e75", "#657b83", "#839496", "#6c71c4", "#93a1a1", "#fdf6e3",
        ]))

    static let dracula = Theme(
        id: "dracula", displayName: "Dracula",
        foreground: hexNS("#f8f8f2"), background: hexNS("#282a36"), cursor: hexNS("#f8f8f2"),
        ansi: palette([
            "#21222c", "#ff5555", "#50fa7b", "#f1fa8c", "#bd93f9", "#ff79c6", "#8be9fd", "#f8f8f2",
            "#6272a4", "#ff6e6e", "#69ff94", "#ffffa5", "#d6acff", "#ff92df", "#a4ffff", "#ffffff",
        ]))

    /// 覆盖前景/背景色(配置文件里显式给了 foreground/background 时用)
    func overriding(foreground fg: NSColor?, background bg: NSColor?) -> Theme {
        Theme(id: id, displayName: displayName,
              foreground: fg ?? foreground, background: bg ?? background,
              cursor: cursor, ansi: ansi)
    }
}

// MARK: - 颜色工具

/// "#rrggbb" → NSColor(sRGB)
func hexNS(_ hex: String) -> NSColor {
    let (r, g, b) = hexComponents(hex)
    return NSColor(srgbRed: CGFloat(r) / 255, green: CGFloat(g) / 255, blue: CGFloat(b) / 255, alpha: 1)
}

/// "#rrggbb" → SwiftTerm.Color(16-bit 分量)
func hexSwiftTerm(_ hex: String) -> SwiftTerm.Color {
    let (r, g, b) = hexComponents(hex)
    // 8bit → 16bit:×257 让 0xFF 精确映射到 0xFFFF
    return SwiftTerm.Color(red: UInt16(r) * 257, green: UInt16(g) * 257, blue: UInt16(b) * 257)
}

private func palette(_ hexes: [String]) -> [SwiftTerm.Color] {
    hexes.map(hexSwiftTerm)
}

/// 解析 "#rrggbb" / "rrggbb",非法输入回落黑色
private func hexComponents(_ raw: String) -> (Int, Int, Int) {
    var s = raw.trimmingCharacters(in: .whitespaces)
    if s.hasPrefix("#") { s.removeFirst() }
    guard s.count == 6, let v = Int(s, radix: 16) else { return (0, 0, 0) }
    return ((v >> 16) & 0xFF, (v >> 8) & 0xFF, v & 0xFF)
}
