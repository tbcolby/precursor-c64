/// Game file loading: .PRG format, PDDB storage, TCP import

use pddb::Pddb;
use std::io::{Read, Write, Seek, SeekFrom};

const DICT_GAMES: &str = "c64.games";

/// List all stored game names
pub fn list_games(pddb: &Pddb) -> Vec<String> {
    match pddb.list_keys(DICT_GAMES, None) {
        Ok(keys) => keys.into_iter()
            .filter(|k| !k.starts_with('_'))
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Load a game from PDDB by name
pub fn load_game(pddb: &Pddb, name: &str) -> Option<Vec<u8>> {
    match pddb.get(DICT_GAMES, name, None, false, false, None, None::<fn()>) {
        Ok(mut key) => {
            key.seek(SeekFrom::Start(0)).ok()?;
            let mut data = Vec::new();
            key.read_to_end(&mut data).ok()?;
            if data.is_empty() { None } else { Some(data) }
        }
        Err(_) => None,
    }
}

/// Save a game to PDDB
pub fn save_game(pddb: &Pddb, name: &str, data: &[u8]) -> bool {
    match pddb.get(DICT_GAMES, name, None, true, true, Some(data.len()), None::<fn()>) {
        Ok(mut key) => {
            key.seek(SeekFrom::Start(0)).ok();
            if key.write_all(data).is_ok() {
                pddb.sync().ok();
                true
            } else {
                false
            }
        }
        Err(_) => false,
    }
}

/// Delete a game from PDDB
#[allow(dead_code)]
pub fn delete_game(pddb: &Pddb, name: &str) -> bool {
    pddb.delete_key(DICT_GAMES, name, None).is_ok()
}

/// Import a .PRG file via TCP (port 6464)
/// Protocol: first line is filename (terminated by \n), rest is binary .PRG data
pub fn tcp_import(pddb: &Pddb) -> Result<String, &'static str> {
    use std::net::TcpListener;

    let listener = TcpListener::bind("0.0.0.0:6464")
        .map_err(|_| "Failed to bind port 6464")?;

    // Accept one connection
    let (mut stream, _addr) = listener.accept()
        .map_err(|_| "Accept failed")?;

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf)
        .map_err(|_| "Read failed")?;

    if buf.is_empty() {
        return Err("Empty data received");
    }

    // Find the filename (first line)
    let newline_pos = buf.iter().position(|&b| b == b'\n')
        .ok_or("No filename found (expected name\\n followed by PRG data)")?;

    let name = String::from_utf8(buf[..newline_pos].to_vec())
        .map_err(|_| "Invalid filename")?;
    let prg_data = &buf[newline_pos + 1..];

    if prg_data.len() < 3 {
        return Err("PRG data too short (need at least 2-byte header + 1 byte)");
    }

    // Validate it looks like a PRG (load address should be reasonable)
    let load_addr = u16::from_le_bytes([prg_data[0], prg_data[1]]);
    if load_addr == 0 {
        return Err("Invalid PRG: load address is 0");
    }

    if save_game(pddb, &name, prg_data) {
        Ok(name)
    } else {
        Err("Failed to save to PDDB")
    }
}

/// Import from raw PRG bytes with a given name
#[allow(dead_code)]
pub fn import_prg(pddb: &Pddb, name: &str, data: &[u8]) -> Result<(), &'static str> {
    if data.len() < 3 {
        return Err("PRG too short");
    }
    if save_game(pddb, name, data) {
        Ok(())
    } else {
        Err("Failed to save")
    }
}
