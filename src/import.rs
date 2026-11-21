//! TCP import: `printf 'name.prg\n' | cat - game.prg | nc <device-ip> 6464`.
//!
//! One listener thread, started the first time the Import screen is opened. It
//! blocks in `accept` (never on the UI thread), and only reads from a
//! connection while the UI has *armed* it; otherwise the connection is dropped
//! immediately. Reads have a per-read timeout and a total deadline, and are
//! capped at `MAX_IMPORT` bytes. Validation is in `c64_core::prg`.

use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use c64_core::prg::{parse_import, Import, MAX_IMPORT};
use xous::{Message, CID};

pub const PORT: u16 = 6464;
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const TOTAL_DEADLINE: Duration = Duration::from_secs(30);

pub type Outcome = Result<Import, String>;

pub struct Importer {
    cid: CID,
    opcode: usize,
    armed: Arc<AtomicBool>,
    slot: Arc<Mutex<Option<Outcome>>>,
    started: bool,
}

impl Importer {
    pub fn new(cid: CID, opcode: usize) -> Self {
        Self { cid, opcode, armed: Arc::new(AtomicBool::new(false)), slot: Arc::new(Mutex::new(None)), started: false }
    }

    /// Accept one import. Starts the listener thread on first use.
    pub fn arm(&mut self) {
        self.slot.lock().unwrap().take();
        self.armed.store(true, Ordering::SeqCst);
        if !self.started {
            self.started = true;
            let (cid, op, armed, slot) = (self.cid, self.opcode, self.armed.clone(), self.slot.clone());
            std::thread::Builder::new()
                .name("c64-import".into())
                .spawn(move || listen(cid, op, armed, slot))
                .map_err(|e| log::error!("couldn't start import thread: {e}"))
                .ok();
        }
    }

    /// Stop accepting. A connection that arrives later is dropped unread.
    pub fn disarm(&self) { self.armed.store(false, Ordering::SeqCst); }

    pub fn take_result(&self) -> Option<Outcome> { self.slot.lock().unwrap().take() }
}

fn post(cid: CID, op: usize, slot: &Mutex<Option<Outcome>>, result: Outcome) {
    *slot.lock().unwrap() = Some(result);
    let _ = xous::send_message(cid, Message::new_scalar(op, 0, 0, 0, 0));
}

fn listen(cid: CID, op: usize, armed: Arc<AtomicBool>, slot: Arc<Mutex<Option<Outcome>>>) {
    let listener = match TcpListener::bind(("0.0.0.0", PORT)) {
        Ok(l) => l,
        Err(e) => {
            log::error!("import bind failed: {e}");
            post(cid, op, &slot, Err(format!("Couldn't listen on port {PORT}")));
            return;
        }
    };
    loop {
        let Ok((stream, _peer)) = listener.accept() else { continue };
        // Disarm first: exactly one import per arming.
        if !armed.swap(false, Ordering::SeqCst) {
            continue; // not importing: drop the connection unread
        }
        let result = read_import(stream);
        post(cid, op, &slot, result);
    }
}

fn read_import(mut stream: TcpStream) -> Outcome {
    stream.set_read_timeout(Some(READ_TIMEOUT)).ok();
    let start = Instant::now();
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    loop {
        if start.elapsed() > TOTAL_DEADLINE {
            return Err("Import timed out".into());
        }
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.len() > MAX_IMPORT {
                    return Err("Import too large".into());
                }
            }
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                return Err("Import timed out".into());
            }
            Err(_) => return Err("Read failed".into()),
        }
    }
    parse_import(&buf).map_err(|e| format!("Rejected: {e:?}"))
}
