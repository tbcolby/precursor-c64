//! Precursor key -> C64 keyboard matrix, and a queue that gives each press a
//! full hold and release so repeated letters ("LL", "00") are not merged.
//!
//! Matrix (column = CIA1 port A bit, row = port B bit), from the C64 wiki:
//!
//! | col | row0 | row1 | row2 | row3 | row4 | row5 | row6 | row7 |
//! |---|---|---|---|---|---|---|---|---|
//! | 0 | DEL | RETURN | CRSR right | F7 | F1 | F3 | F5 | CRSR down |
//! | 1 | 3 | W | A | 4 | Z | S | E | L-SHIFT |
//! | 2 | 5 | R | D | 6 | C | F | T | X |
//! | 3 | 7 | Y | G | 8 | B | H | U | V |
//! | 4 | 9 | I | J | 0 | M | K | O | N |
//! | 5 | + | P | L | - | . | : | @ | , |
//! | 6 | GBP | * | ; | HOME | R-SHIFT | = | ^ | / |
//! | 7 | 1 | <- | CTRL | 2 | SPACE | C= | Q | RUN/STOP |

use std::collections::VecDeque;

/// A C64 matrix key position (column, row)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatrixPos {
    pub col: u8,
    pub row: u8,
}

const fn mp(col: u8, row: u8) -> MatrixPos { MatrixPos { col, row } }

/// C64 left shift key matrix position
pub const LSHIFT: MatrixPos = mp(1, 7);

/// Keys for the C64 function keys. Precursor F1-F3 map to C64 F1, F3, F5;
/// F4 leaves the emulator. C64 F2/F4/F6/F8 are shift+F1/F3/F5/F7.
pub const RUN_STOP_CHAR: char = '`';

/// A key press as the emulator sees it: the matrix position plus whether
/// left shift is held with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stroke {
    pub pos: MatrixPos,
    pub shift: bool,
}

/// Map a Precursor character to a C64 stroke. None if there is no mapping.
pub fn map_key(ch: char) -> Option<Stroke> {
    let (pos, shift) = match ch {
        'a' | 'A' => (mp(1, 2), false),
        'b' | 'B' => (mp(3, 4), false),
        'c' | 'C' => (mp(2, 4), false),
        'd' | 'D' => (mp(2, 2), false),
        'e' | 'E' => (mp(1, 6), false),
        'f' | 'F' => (mp(2, 5), false),
        'g' | 'G' => (mp(3, 2), false),
        'h' | 'H' => (mp(3, 5), false),
        'i' | 'I' => (mp(4, 1), false),
        'j' | 'J' => (mp(4, 2), false),
        'k' | 'K' => (mp(4, 5), false),
        'l' | 'L' => (mp(5, 2), false),
        'm' | 'M' => (mp(4, 4), false),
        'n' | 'N' => (mp(4, 7), false),
        'o' | 'O' => (mp(4, 6), false),
        'p' | 'P' => (mp(5, 1), false),
        'q' | 'Q' => (mp(7, 6), false),
        'r' | 'R' => (mp(2, 1), false),
        's' | 'S' => (mp(1, 5), false),
        't' | 'T' => (mp(2, 6), false),
        'u' | 'U' => (mp(3, 6), false),
        'v' | 'V' => (mp(3, 7), false),
        'w' | 'W' => (mp(1, 1), false),
        'x' | 'X' => (mp(2, 7), false),
        'y' | 'Y' => (mp(3, 1), false),
        'z' | 'Z' => (mp(1, 4), false),

        '0' => (mp(4, 3), false),
        '1' => (mp(7, 0), false),
        '2' => (mp(7, 3), false),
        '3' => (mp(1, 0), false),
        '4' => (mp(1, 3), false),
        '5' => (mp(2, 0), false),
        '6' => (mp(2, 3), false),
        '7' => (mp(3, 0), false),
        '8' => (mp(3, 3), false),
        '9' => (mp(4, 0), false),
        '!' => (mp(7, 0), true),
        '"' => (mp(7, 3), true),
        '#' => (mp(1, 0), true),
        '$' => (mp(1, 3), true),
        '%' => (mp(2, 0), true),
        '&' => (mp(2, 3), true),
        '\'' => (mp(3, 0), true),
        '(' => (mp(3, 3), true),
        ')' => (mp(4, 0), true),

        ' ' => (mp(7, 4), false),
        '\r' | '\n' => (mp(0, 1), false),
        '\u{0008}' => (mp(0, 0), false), // Backspace -> INST/DEL

        ',' => (mp(5, 7), false),
        '<' => (mp(5, 7), true),
        '.' => (mp(5, 4), false),
        '>' => (mp(5, 4), true),
        '/' => (mp(6, 7), false),
        '?' => (mp(6, 7), true),
        ';' => (mp(6, 2), false),
        ':' => (mp(5, 5), false), // its own key on a C64
        '=' => (mp(6, 5), false),
        '+' => (mp(5, 0), false), // its own key on a C64
        '-' => (mp(5, 3), false),
        '*' => (mp(6, 1), false),
        '@' => (mp(5, 6), false),
        '^' => (mp(6, 6), false),
        '\u{00A3}' => (mp(6, 0), false), // pound sign

        // Arrows: cursor right/down; up and left are shifted.
        '\u{2193}' => (mp(0, 7), false),
        '\u{2191}' => (mp(0, 7), true),
        '\u{2192}' => (mp(0, 2), false),
        '\u{2190}' => (mp(0, 2), true),

        RUN_STOP_CHAR => (mp(7, 7), false),
        _ => return None,
    };
    Some(Stroke { pos, shift })
}

/// C64 function key for Precursor F1..F3 (U+0011..U+0013). F4 is the exit key.
pub fn map_function_key(n: u8) -> Option<Stroke> {
    match n {
        1 => Some(Stroke { pos: mp(0, 4), shift: false }), // F1
        2 => Some(Stroke { pos: mp(0, 5), shift: false }), // F3
        3 => Some(Stroke { pos: mp(0, 6), shift: false }), // F5
        _ => None,
    }
}

/// Frames a key stays down, and frames of released keyboard between keys.
/// At 50,000 cycles per frame (about 2.5 PAL jiffies) one frame is enough for
/// the KERNAL's 50 Hz scan to see each state.
pub const HOLD_FRAMES: u8 = 1;
pub const GAP_FRAMES: u8 = 1;
const MAX_QUEUED: usize = 32;

#[derive(Default)]
pub struct KeyQueue {
    queue: VecDeque<Stroke>,
    /// Currently pressed stroke and frames left to hold it.
    down: Option<(Stroke, u8)>,
    gap: u8,
}

/// What the matrix should do after a frame tick.
#[derive(Debug, PartialEq, Eq)]
pub enum Edge {
    None,
    Press(Stroke),
    Release(Stroke),
}

impl KeyQueue {
    pub fn push(&mut self, s: Stroke) {
        if self.queue.len() < MAX_QUEUED {
            self.queue.push_back(s);
        }
    }

    pub fn clear(&mut self) {
        self.queue.clear();
        self.down = None;
        self.gap = 0;
    }

    pub fn is_idle(&self) -> bool { self.queue.is_empty() && self.down.is_none() }

    /// Advance one frame. The caller applies the returned edge to the matrix.
    pub fn tick(&mut self) -> Edge {
        if let Some((s, left)) = self.down {
            if left <= 1 {
                self.down = None;
                self.gap = GAP_FRAMES;
                return Edge::Release(s);
            }
            self.down = Some((s, left - 1));
            return Edge::None;
        }
        if self.gap > 0 {
            self.gap -= 1;
            return Edge::None;
        }
        if let Some(s) = self.queue.pop_front() {
            self.down = Some((s, HOLD_FRAMES));
            return Edge::Press(s);
        }
        Edge::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_match_the_wiki_matrix() {
        assert_eq!(map_key('a').unwrap().pos, mp(1, 2));
        assert_eq!(map_key('Q').unwrap().pos, mp(7, 6));
        assert_eq!(map_key('n').unwrap().pos, mp(4, 7));
        assert_eq!(map_key(' ').unwrap().pos, mp(7, 4));
        assert_eq!(map_key('\r').unwrap().pos, mp(0, 1));
    }

    // Regression for C-5: ':' and '+' are unshifted keys of their own.
    #[test]
    fn colon_plus_at_are_their_own_keys() {
        assert_eq!(map_key(':'), Some(Stroke { pos: mp(5, 5), shift: false }));
        assert_eq!(map_key('+'), Some(Stroke { pos: mp(5, 0), shift: false }));
        assert_eq!(map_key('@'), Some(Stroke { pos: mp(5, 6), shift: false }));
        assert_ne!(map_key(':'), map_key(';'));
    }

    #[test]
    fn shifted_digits_and_cursor_up_left() {
        assert_eq!(map_key('"'), Some(Stroke { pos: mp(7, 3), shift: true }));
        assert_eq!(map_key('\u{2191}'), Some(Stroke { pos: mp(0, 7), shift: true }));
        assert_eq!(map_key('\u{2192}'), Some(Stroke { pos: mp(0, 2), shift: false }));
    }

    // Regression for C-20: Xous delivers F1..F3 as U+0011.. (via the shell's
    // Key::F); unmapped codes and the old PUA values are not keys.
    #[test]
    fn function_keys_and_dead_codes() {
        assert_eq!(map_function_key(1).unwrap().pos, mp(0, 4));
        assert_eq!(map_function_key(3).unwrap().pos, mp(0, 6));
        assert!(map_function_key(4).is_none());
        assert!(map_key('\u{F001}').is_none());
        assert!(map_key('\u{001B}').is_none());
        assert_eq!(map_key(RUN_STOP_CHAR).unwrap().pos, mp(7, 7));
    }

    #[test]
    fn no_two_chars_collide_unintentionally_on_unshifted_keys() {
        // Every unshifted stroke from the table maps to one matrix cell: no duplicates
        // except upper/lower case and the shifted variants of the same key.
        use std::collections::HashMap;
        let mut seen: HashMap<(u8, u8), char> = HashMap::new();
        for c in "abcdefghijklmnopqrstuvwxyz0123456789 ,.-/;:=+*@^".chars() {
            let s = map_key(c).unwrap();
            assert!(!s.shift);
            if let Some(prev) = seen.insert((s.pos.col, s.pos.row), c) {
                panic!("{prev} and {c} share a matrix cell");
            }
        }
    }

    // Regression for C-4: the same key twice gets a release in between.
    #[test]
    fn repeated_key_is_released_between_presses() {
        let l = map_key('l').unwrap();
        let mut q = KeyQueue::default();
        q.push(l);
        q.push(l);
        let mut edges = Vec::new();
        for _ in 0..8 {
            edges.push(q.tick());
        }
        let presses = edges.iter().filter(|e| matches!(e, Edge::Press(_))).count();
        let releases = edges.iter().filter(|e| matches!(e, Edge::Release(_))).count();
        assert_eq!((presses, releases), (2, 2));
        // Press, (hold), Release, (gap), Press ... never two presses in a row.
        let order: Vec<_> = edges.iter().filter(|e| **e != Edge::None).collect();
        assert!(matches!(order[0], Edge::Press(_)) && matches!(order[1], Edge::Release(_)));
        assert!(matches!(order[2], Edge::Press(_)) && matches!(order[3], Edge::Release(_)));
        assert!(q.is_idle());
    }

    #[test]
    fn at_least_one_idle_frame_between_keys() {
        let a = map_key('a').unwrap();
        let mut q = KeyQueue::default();
        q.push(a);
        q.push(a);
        let mut frames_between = None;
        let mut since_release: Option<u32> = None;
        for _ in 0..10 {
            match q.tick() {
                Edge::Release(_) => since_release = Some(0),
                Edge::Press(_) => {
                    if let Some(n) = since_release {
                        frames_between = Some(n);
                    }
                }
                Edge::None => {
                    if let Some(n) = since_release.as_mut() {
                        *n += 1;
                    }
                }
            }
        }
        assert!(frames_between.unwrap() >= 1);
    }

    #[test]
    fn queue_is_bounded() {
        let mut q = KeyQueue::default();
        for _ in 0..100 {
            q.push(map_key('a').unwrap());
        }
        let mut presses = 0;
        for _ in 0..400 {
            if matches!(q.tick(), Edge::Press(_)) {
                presses += 1;
            }
        }
        assert_eq!(presses, MAX_QUEUED);
    }
}
