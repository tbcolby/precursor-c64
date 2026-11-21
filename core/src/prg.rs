//! .PRG parsing and the TCP import wire format.
//!
//! Import protocol (unchanged from earlier versions): one line with a name,
//! `\n`, then the raw bytes. Names are validated; ROM images are accepted
//! under the reserved names `rom:basic`, `rom:kernal` and `rom:chargen` and
//! must have the exact size.

use crate::memory::{BASIC_LEN, CHARGEN_LEN, KERNAL_LEN};

/// Largest PRG accepted (the whole address space).
pub const MAX_PRG: usize = 65536 + 2;
/// Longest name line accepted.
pub const MAX_NAME: usize = 32;
/// Largest total import payload (name line + data).
pub const MAX_IMPORT: usize = MAX_PRG + MAX_NAME + 2;
/// Lowest load address accepted: below this are the zero page and the stack.
pub const MIN_LOAD: u16 = 0x0200;

#[derive(Debug, PartialEq, Eq)]
pub enum PrgError {
    TooShort,
    TooLarge,
    /// Load address is in the zero page or stack.
    BadLoadAddress(u16),
}

/// A parsed .PRG: load address and payload.
#[derive(Debug, PartialEq, Eq)]
pub struct Prg<'a> {
    pub load: u16,
    pub data: &'a [u8],
}

impl<'a> Prg<'a> {
    pub fn parse(file: &'a [u8]) -> Result<Prg<'a>, PrgError> {
        if file.len() < 3 {
            return Err(PrgError::TooShort);
        }
        let load = u16::from_le_bytes([file[0], file[1]]);
        let data = &file[2..];
        if load < MIN_LOAD {
            return Err(PrgError::BadLoadAddress(load));
        }
        // The program must fit below $10000 (no silent wrap or truncation).
        if load as usize + data.len() > 0x1_0000 {
            return Err(PrgError::TooLarge);
        }
        Ok(Prg { load, data })
    }

    /// First address after the program.
    pub fn end(&self) -> u32 { self.load as u32 + self.data.len() as u32 }

    /// The target of a BASIC `SYS nnnn` stub at $0801, if the program starts with one.
    /// Layout: next-line pointer, line number, token $9E (SYS), spaces, decimal digits.
    pub fn sys_target(&self) -> Option<u16> {
        if self.load != 0x0801 || self.data.len() < 6 {
            return None;
        }
        let d = self.data;
        if u16::from_le_bytes([d[0], d[1]]) == 0 || d[4] != 0x9E {
            return None;
        }
        let mut i = 5;
        while d.get(i) == Some(&b' ') {
            i += 1;
        }
        let mut value: u32 = 0;
        let mut digits = 0;
        while let Some(&b) = d.get(i) {
            if !b.is_ascii_digit() {
                break;
            }
            value = value * 10 + (b - b'0') as u32;
            digits += 1;
            if digits > 5 || value > 0xFFFF {
                return None;
            }
            i += 1;
        }
        (digits > 0).then_some(value as u16)
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum RomKind {
    Basic,
    Kernal,
    Chargen,
}

impl RomKind {
    pub fn expected_len(self) -> usize {
        match self {
            RomKind::Basic => BASIC_LEN,
            RomKind::Kernal => KERNAL_LEN,
            RomKind::Chargen => CHARGEN_LEN,
        }
    }

    /// PDDB key name in `c64.roms`.
    pub fn key(self) -> &'static str {
        match self {
            RomKind::Basic => "basic",
            RomKind::Kernal => "kernal",
            RomKind::Chargen => "chargen",
        }
    }
}

/// A validated import.
#[derive(Debug, PartialEq, Eq)]
pub enum Import {
    Game { name: String, file: Vec<u8> },
    Rom { kind: RomKind, data: Vec<u8> },
}

#[derive(Debug, PartialEq, Eq)]
pub enum ImportError {
    Empty,
    TooLarge,
    NoName,
    BadName,
    BadPrg(PrgError),
    BadRomSize { kind: RomKind, got: usize },
}

/// A game name: 1-32 characters from `[A-Za-z0-9 ._-]`, not starting with
/// `_` or a space, no trailing space. A trailing `\r` (CRLF senders) is trimmed first.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && !name.starts_with(['_', ' ', '.'])
        && !name.ends_with(' ')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '_' | '-'))
}

/// Parse the raw bytes received on the import socket.
pub fn parse_import(buf: &[u8]) -> Result<Import, ImportError> {
    if buf.is_empty() {
        return Err(ImportError::Empty);
    }
    if buf.len() > MAX_IMPORT {
        return Err(ImportError::TooLarge);
    }
    let nl = buf.iter().take(MAX_NAME + 2).position(|&b| b == b'\n').ok_or(ImportError::NoName)?;
    let name = core::str::from_utf8(&buf[..nl]).map_err(|_| ImportError::BadName)?;
    let name = name.strip_suffix('\r').unwrap_or(name);
    let data = &buf[nl + 1..];

    let rom = match name {
        "rom:basic" => Some(RomKind::Basic),
        "rom:kernal" => Some(RomKind::Kernal),
        "rom:chargen" => Some(RomKind::Chargen),
        _ => None,
    };
    if let Some(kind) = rom {
        if data.len() != kind.expected_len() {
            return Err(ImportError::BadRomSize { kind, got: data.len() });
        }
        return Ok(Import::Rom { kind, data: data.to_vec() });
    }
    if !valid_name(name) {
        return Err(ImportError::BadName);
    }
    Prg::parse(data).map_err(ImportError::BadPrg)?;
    Ok(Import::Game { name: name.to_string(), file: data.to_vec() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn basic_sys(addr: &str) -> Vec<u8> {
        // 10 SYS addr
        let mut v = vec![0x01, 0x08, 0x0B, 0x08, 0x0A, 0x00, 0x9E];
        v.extend_from_slice(addr.as_bytes());
        v.extend_from_slice(&[0, 0, 0]);
        v
    }

    #[test]
    fn parses_load_address_and_payload() {
        let p = Prg::parse(&[0x00, 0xC0, 1, 2, 3]).unwrap();
        assert_eq!((p.load, p.data), (0xC000, &[1u8, 2, 3][..]));
        assert_eq!(p.end(), 0xC003);
    }

    #[test]
    fn rejects_bad_prgs() {
        assert_eq!(Prg::parse(&[1, 2]), Err(PrgError::TooShort));
        assert_eq!(Prg::parse(&[0x00, 0x00, 1]), Err(PrgError::BadLoadAddress(0)));
        assert_eq!(Prg::parse(&[0xFF, 0x01, 1]), Err(PrgError::BadLoadAddress(0x01FF)));
        // Regression for C-18: a program reaching past $FFFF is refused, not wrapped.
        assert_eq!(Prg::parse(&[0xFF, 0xFF, 1, 2]), Err(PrgError::TooLarge));
        assert!(Prg::parse(&[0xFF, 0xFF, 1]).is_ok(), "exactly fits");
    }

    #[test]
    fn finds_sys_target() {
        let f = basic_sys("2061");
        assert_eq!(Prg::parse(&f).unwrap().sys_target(), Some(2061));
        let f = basic_sys(" 49152");
        assert_eq!(Prg::parse(&f).unwrap().sys_target(), Some(49152));
        assert_eq!(Prg::parse(&basic_sys("99999")).unwrap().sys_target(), None, "out of range");
        assert_eq!(Prg::parse(&basic_sys("")).unwrap().sys_target(), None);
        // Not at $0801: no stub.
        let mut f = basic_sys("2061");
        f[0] = 0x00;
        f[1] = 0x20;
        assert_eq!(Prg::parse(&f).unwrap().sys_target(), None);
        // A line that isn't SYS (e.g. PRINT = $99).
        let mut f = basic_sys("2061");
        f[6] = 0x99;
        assert_eq!(Prg::parse(&f).unwrap().sys_target(), None);
    }

    #[test]
    fn name_rules() {
        assert!(valid_name("elite.prg"));
        assert!(valid_name("My Game-2"));
        assert!(!valid_name(""));
        assert!(!valid_name("_hidden"));
        assert!(!valid_name(" lead"));
        assert!(!valid_name("trail "));
        assert!(!valid_name("a/b"));
        assert!(!valid_name("naïve"));
        assert!(!valid_name(&"x".repeat(33)));
        assert!(valid_name(&"x".repeat(32)));
    }

    #[test]
    fn import_game_with_crlf_name() {
        let mut buf = b"game.prg\r\n".to_vec();
        buf.extend_from_slice(&[0x00, 0xC0, 0xEA]);
        match parse_import(&buf).unwrap() {
            Import::Game { name, file } => {
                assert_eq!(name, "game.prg");
                assert_eq!(file, vec![0x00, 0xC0, 0xEA]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn import_rejects_bad_input() {
        assert_eq!(parse_import(b""), Err(ImportError::Empty));
        assert_eq!(parse_import(b"noname"), Err(ImportError::NoName));
        assert_eq!(parse_import(b"\n\x00\xC0\x00"), Err(ImportError::BadName));
        assert_eq!(parse_import(b"_x\n\x00\xC0\x00"), Err(ImportError::BadName));
        assert_eq!(parse_import(b"x\n\x00\x00\x00"), Err(ImportError::BadPrg(PrgError::BadLoadAddress(0))));
        let big = vec![b'a'; MAX_IMPORT + 1];
        assert_eq!(parse_import(&big), Err(ImportError::TooLarge));
        // No newline within the name limit.
        let mut long = vec![b'a'; 100];
        long.push(b'\n');
        assert_eq!(parse_import(&long), Err(ImportError::NoName));
    }

    #[test]
    fn import_rom_needs_exact_size() {
        let mut buf = b"rom:chargen\n".to_vec();
        buf.extend(vec![0u8; CHARGEN_LEN]);
        assert!(matches!(parse_import(&buf).unwrap(), Import::Rom { kind: RomKind::Chargen, .. }));
        let mut short = b"rom:kernal\n".to_vec();
        short.extend(vec![0u8; 100]);
        assert_eq!(parse_import(&short), Err(ImportError::BadRomSize { kind: RomKind::Kernal, got: 100 }));
    }
}
