/// Precursor rawkeys → C64 keyboard matrix position mapping
///
/// C64 keyboard is an 8×8 matrix. Each key has a (column, row) position.
/// Reference: https://www.c64-wiki.com/wiki/Keyboard#Keyboard_matrix

/// A C64 matrix key position (column, row)
#[derive(Clone, Copy)]
pub struct MatrixPos {
    pub col: u8,
    pub row: u8,
}

/// Map a Precursor rawkey character to a C64 matrix position
/// Returns None if no mapping exists
pub fn map_key(ch: char) -> Option<MatrixPos> {
    let (col, row) = match ch {
        // Letters (directly mapped to C64 keyboard matrix)
        'a' | 'A' => (1, 2),
        'b' | 'B' => (3, 4),
        'c' | 'C' => (2, 4),
        'd' | 'D' => (2, 2),
        'e' | 'E' => (1, 6),
        'f' | 'F' => (2, 5),
        'g' | 'G' => (3, 2),
        'h' | 'H' => (3, 5),
        'i' | 'I' => (4, 1),
        'j' | 'J' => (4, 2),
        'k' | 'K' => (4, 5),
        'l' | 'L' => (5, 2),
        'm' | 'M' => (4, 4),
        'n' | 'N' => (4, 7),
        'o' | 'O' => (4, 6),
        'p' | 'P' => (5, 1),
        'q' | 'Q' => (7, 6),
        'r' | 'R' => (2, 1),
        's' | 'S' => (1, 5),
        't' | 'T' => (2, 6),
        'u' | 'U' => (3, 6),
        'v' | 'V' => (3, 7),
        'w' | 'W' => (1, 1),
        'x' | 'X' => (2, 7),
        'y' | 'Y' => (3, 1),
        'z' | 'Z' => (1, 4),

        // Numbers
        '0' => (4, 3),
        '1' | '!' => (7, 0),
        '2' | '"' => (7, 3),
        '3' | '#' => (1, 0),
        '4' | '$' => (1, 3),
        '5' | '%' => (2, 0),
        '6' | '&' => (2, 3),
        '7' | '\'' => (3, 0),
        '8' | '(' => (3, 3),
        '9' | ')' => (4, 0),

        // Special characters
        ' ' => (7, 4),         // Space
        '\r' | '\n' => (0, 1), // Return
        '\u{0008}' => (0, 0),  // Backspace → DEL (INST/DEL)

        // Punctuation
        ',' | '<' => (5, 7),
        '.' | '>' => (5, 4),
        '/' | '?' => (6, 7),
        ';' | ':' => (6, 2),  // C64: semicolon
        '=' | '+' => (6, 5),  // C64: equals/plus
        '-' => (5, 3),        // C64: minus
        '*' => (6, 1),        // C64: asterisk

        // Arrow keys → C64 cursor keys
        // C64 cursor down = col 0, row 7
        // C64 cursor right = col 0, row 2
        // C64 cursor up = SHIFT + cursor down
        // C64 cursor left = SHIFT + cursor right
        '\u{2193}' => (0, 7),  // Down arrow → CRSR DOWN
        '\u{2191}' => (0, 7),  // Up arrow → CRSR DOWN (will add shift)
        '\u{2192}' => (0, 2),  // Right arrow → CRSR RIGHT
        '\u{2190}' => (0, 2),  // Left arrow → CRSR RIGHT (will add shift)

        // F-keys
        '\u{F001}' => (0, 4),  // F1
        '\u{F002}' => (0, 4),  // F2 (shift+F1 on C64)
        '\u{F003}' => (0, 5),  // F3
        '\u{F004}' => (0, 5),  // F4 (shift+F3 on C64)

        // Run/Stop → Esc on Precursor
        '\u{001B}' => (7, 7),  // ESC → RUN/STOP

        _ => return None,
    };
    Some(MatrixPos { col, row })
}

/// Does this key require the shift modifier on C64?
pub fn needs_shift(ch: char) -> bool {
    match ch {
        // Uppercase letters need shift on C64 (reversed from normal: C64 default is uppercase)
        // Actually C64 is uppercase by default, so lowercase needs shift... but in PETSCII,
        // the default charset shows uppercase, and shift gives graphics characters.
        // For text mode with shifted charset (lowercase mode), lowercase = normal, uppercase = shift
        // We'll handle this based on the current charset mode.

        // Arrow keys: up and left need shift (they're shift+down and shift+right on C64)
        '\u{2191}' => true, // Up = shift + CRSR DOWN
        '\u{2190}' => true, // Left = shift + CRSR RIGHT

        // Shifted punctuation
        '!' | '"' | '#' | '$' | '%' | '&' | '\'' | '(' | ')' => true,
        '<' | '>' | '?' => true,
        ':' | '+' => true,

        // F2, F4 need shift
        '\u{F002}' | '\u{F004}' => true,

        _ => false,
    }
}

/// C64 left shift key matrix position
pub const LSHIFT: MatrixPos = MatrixPos { col: 1, row: 7 };

/// C64 right shift key matrix position
#[allow(dead_code)]
pub const RSHIFT: MatrixPos = MatrixPos { col: 6, row: 4 };

/// C64 Commodore key matrix position (C= key)
#[allow(dead_code)]
pub const COMMODORE: MatrixPos = MatrixPos { col: 7, row: 5 };

/// C64 CTRL key matrix position
#[allow(dead_code)]
pub const CTRL: MatrixPos = MatrixPos { col: 7, row: 2 };
