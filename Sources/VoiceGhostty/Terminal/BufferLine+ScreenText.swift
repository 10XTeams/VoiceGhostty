import Foundation
import SwiftTerm

extension BufferLine {
    /// One line of the terminal buffer as the text you actually saw.
    ///
    /// `translateToString` hands back the buffer's own spelling, which is not quite text. A cell holds a
    /// character code, and code 0 means two different things:
    ///
    /// - **a cell nothing ever wrote.** A TUI that lays its output out by moving the cursor — which is
    ///   what claude does between the words of a heading, and what any box-drawing program does around
    ///   its padding — never prints the spaces you see, so those cells come back as NUL rather than as
    ///   `" "`. Left alone they poison anything that reads the line: NUL is not `isWhitespace`, and a
    ///   file full of them is a file `grep` calls binary.
    /// - **the second cell of a wide (CJK, emoji) character**, whose glyph is already in the cell before
    ///   it. That one is a placeholder and has to be dropped, not turned into a space, or every Chinese
    ///   character in a transcript gains a space after it.
    ///
    /// Trailing NULs need no handling of their own: `trimRight` ends the line at its last written cell.
    func screenText() -> String {
        // `skipNullCellsFollowingWide` takes care of the placeholders; what is left is the unwritten cells.
        let text = translateToString(trimRight: true, skipNullCellsFollowingWide: true)
        guard text.contains("\0") else { return text }
        return String(text.map { $0 == "\0" ? " " : $0 })
    }
}
