//! Screens, input, storage and drawing for the C64 shell. Emulation lives in
//! `c64-core`; this file never touches CPU or chip state except through `Machine`.

use c64_core::machine::{changed_cells, LaunchError, Machine, RomMode, RomSet, Start};
use c64_core::prg::{Import, PrgError};
use c64_core::vic::{FB_LEN, HEIGHT, ROW_BYTES, WIDTH};
use gam::menu::{PixelColor, Point};
use gam::GlyphStyle;
use precursor_appkit::keys::Key;
use precursor_appkit::layout::ListState;
use precursor_appkit::screen::Screen;
use precursor_appkit::store::Store;

use crate::import::{self, Importer};

#[derive(PartialEq, Eq)]
pub enum Action {
    Stay,
    Leave,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Menu,
    Running,
    Importing,
}

/// Height of the border bands above and below the 200-line display.
const BAND: isize = 12;

pub struct App {
    screen: Screen,
    games: Store,
    roms: Store,
    importer: Importer,
    mode: Mode,
    list: ListState,
    visible_rows: usize,
    game_list: Vec<String>,
    status: String,
    machine: Option<Box<Machine>>,
    /// Last framebuffer drawn, for changed-cell drawing.
    prev_fb: Vec<u8>,
    prev_border: Option<bool>,
    foreground: bool,
    jam_shown: bool,
}

/// Where the 320x200 display sits in the canvas.
fn origin(canvas_w: isize) -> Point { Point::new((canvas_w - WIDTH as isize) / 2, BAND) }

/// (basic, kernal, chargen) as read from the PDDB.
type RomBytes = (Option<Vec<u8>>, Option<Vec<u8>>, Option<Vec<u8>>);

impl App {
    pub fn new(screen: Screen, games: Store, roms: Store, importer: Importer) -> Self {
        let mut app = Self {
            screen,
            games,
            roms,
            importer,
            mode: Mode::Menu,
            list: ListState::default(),
            visible_rows: 6,
            game_list: Vec::new(),
            status: String::new(),
            machine: None,
            prev_fb: vec![0; FB_LEN],
            prev_border: None,
            foreground: true,
            jam_shown: false,
        };
        app.reload();
        app
    }

    pub fn screen(&self) -> &Screen { &self.screen }

    /// Frames are pumped only while the machine runs in the foreground.
    pub fn wants_frames(&self) -> bool { self.mode == Mode::Running && self.foreground }

    /// Reload the game list (start, basis change, after an import).
    pub fn reload(&mut self) {
        match self.games.keys() {
            Ok(mut keys) => {
                keys.retain(|k| !k.starts_with('_'));
                keys.sort();
                self.game_list = keys;
            }
            Err(e) => {
                log::error!("couldn't list games: {:?}", e);
                self.game_list.clear();
                self.status = "Couldn't read the game list".into();
            }
        }
        let n = self.menu_len();
        self.list.clamp(n, self.visible_rows);
        self.draw(true);
    }

    pub fn on_foreground(&mut self) {
        self.foreground = true;
        self.screen.refresh_size();
        self.draw(true);
    }

    /// Stop everything that ticks; the shell stops the pump.
    pub fn on_background(&mut self) { self.foreground = false; }

    // ---- menu ------------------------------------------------------------------

    fn menu_len(&self) -> usize { self.game_list.len() + 2 }

    fn menu_labels(&self) -> Vec<String> {
        let mut v: Vec<String> = self.game_list.clone();
        v.push(format!("Import game or ROM (TCP :{})", import::PORT));
        v.push("Boot C64 (no game)".into());
        v
    }

    /// Read user ROMs from the PDDB. BASIC and KERNAL are used only as a pair.
    fn load_roms(&self) -> RomBytes {
        let read = |k: &str| self.roms.read_bytes(k).ok().flatten();
        (read("basic"), read("kernal"), read("chargen"))
    }

    fn new_machine(&self) -> Box<Machine> {
        let (b, k, c) = self.load_roms();
        Box::new(Machine::with_roms(&RomSet { basic: b.as_deref(), kernal: k.as_deref(), chargen: c.as_deref() }))
    }

    fn rom_summary(&self) -> String {
        let (b, k, c) = self.load_roms();
        let pair = b.as_ref().is_some_and(|b| b.len() == 8192) && k.as_ref().is_some_and(|k| k.len() == 8192);
        format!(
            "ROMs: {}; font: {}",
            if pair { "BASIC+KERNAL loaded" } else { "none (built-in stub, no BASIC)" },
            if c.is_some_and(|c| c.len() == 4096) { "loaded" } else { "built-in" }
        )
    }

    fn start_running(&mut self, game: Option<&str>) {
        self.status.clear();
        let mut machine = self.new_machine();
        match game {
            None => {
                machine.boot();
            }
            Some(name) => {
                let data = match self.games.read_bytes(name) {
                    Ok(Some(d)) => d,
                    _ => {
                        self.status = format!("Couldn't read {name}");
                        return;
                    }
                };
                match machine.launch_prg(&data) {
                    Ok(l) => {
                        self.status = match (&l.how, l.booted, machine.mode) {
                            (_, false, RomMode::Real) => "READY prompt not detected; program started anyway".into(),
                            (Start::Jump(a), _, _) => format!("Started at ${a:04X}"),
                            (Start::Run, _, _) => "RUN typed".into(),
                        };
                    }
                    Err(LaunchError::NeedsBasicRom) => {
                        self.status = "BASIC program: needs rom:basic and rom:kernal (see README)".into();
                        return;
                    }
                    Err(LaunchError::Prg(e)) => {
                        self.status = format!("Bad PRG: {}", prg_error(&e));
                        return;
                    }
                }
            }
        }
        self.prev_fb.fill(0);
        self.prev_border = None;
        self.jam_shown = false;
        self.machine = Some(machine);
        self.mode = Mode::Running;
    }

    fn key_menu(&mut self, key: Key) -> Action {
        let n = self.menu_len();
        match key {
            Key::Up => self.list.step(-1, n, self.visible_rows),
            Key::Down => self.list.step(1, n, self.visible_rows),
            k if k.is_back() => return Action::Leave,
            Key::Enter => {
                let i = self.list.selected;
                if i < self.game_list.len() {
                    let name = self.game_list[i].clone();
                    self.start_running(Some(&name));
                } else if i == self.game_list.len() {
                    self.status.clear();
                    self.importer.arm();
                    self.mode = Mode::Importing;
                } else {
                    self.start_running(None);
                }
            }
            _ => {}
        }
        Action::Stay
    }

    // ---- running ---------------------------------------------------------------

    fn stop_running(&mut self) {
        self.mode = Mode::Menu;
        self.machine = None; // frees the ~100 KB of machine state
        self.status = self.rom_summary();
    }

    fn key_running(&mut self, key: Key) {
        // F4 leaves the emulator. Backspace is the C64's DEL key here, so this
        // screen's "back" is F4 only.
        if key == Key::F(4) {
            self.stop_running();
            return;
        }
        let Some(m) = self.machine.as_mut() else { return };
        match key {
            Key::F(n) => {
                m.press_function(n);
            }
            Key::Enter => {
                m.press_char('\r');
            }
            Key::Backspace => {
                m.press_char('\u{8}');
            }
            Key::Up => {
                m.press_char('\u{2191}');
            }
            Key::Down => {
                m.press_char('\u{2193}');
            }
            Key::Left => {
                m.press_char('\u{2190}');
            }
            Key::Right => {
                m.press_char('\u{2192}');
            }
            Key::Char(c) => {
                m.press_char(c);
            }
        }
    }

    pub fn on_frame(&mut self) {
        let Some(m) = self.machine.as_mut() else { return };
        m.frame();
        self.draw_frame(false);
    }

    // ---- import ----------------------------------------------------------------

    pub fn on_import_done(&mut self) {
        let Some(result) = self.importer.take_result() else { return };
        if self.mode != Mode::Importing {
            return; // user cancelled; discard
        }
        self.mode = Mode::Menu;
        self.status = match result {
            Err(e) => e,
            Ok(Import::Game { name, file }) => match self.games.write_bytes(&name, &file) {
                Ok(()) => {
                    self.reload();
                    format!("Imported {name} ({} bytes)", file.len())
                }
                Err(e) => {
                    log::error!("import save failed: {:?}", e);
                    "Couldn't save the game".into()
                }
            },
            Ok(Import::Rom { kind, data }) => match self.roms.write_bytes(kind.key(), &data) {
                Ok(()) => format!("Stored ROM {} ({})", kind.key(), self.rom_summary()),
                Err(e) => {
                    log::error!("rom save failed: {:?}", e);
                    "Couldn't save the ROM".into()
                }
            },
        };
    }

    // ---- input -----------------------------------------------------------------

    pub fn on_key(&mut self, key: Key) -> Action {
        match self.mode {
            Mode::Menu => return self.key_menu(key),
            Mode::Running => self.key_running(key),
            Mode::Importing => {
                if key.is_back() {
                    self.importer.disarm();
                    self.mode = Mode::Menu;
                }
            }
        }
        Action::Stay
    }

    // ---- drawing ---------------------------------------------------------------

    pub fn draw(&mut self, full: bool) {
        if !self.foreground {
            return;
        }
        match self.mode {
            Mode::Menu => self.draw_menu(),
            Mode::Importing => self.draw_importing(),
            Mode::Running => self.draw_frame(full),
        }
    }

    fn draw_menu(&mut self) {
        self.screen.clear();
        let top = self.screen.header("C64");
        let bottom = self.screen.footer("↑↓ select · ⏎ run · ⌫ leave");
        let status_h = if self.status.is_empty() { 0 } else { 3 * self.screen.line_height(GlyphStyle::Small) };
        let labels = self.menu_labels();
        self.visible_rows = self.screen.list(top, bottom - status_h, &labels, &self.list);
        if !self.status.is_empty() {
            self.screen.para(bottom - status_h, bottom, GlyphStyle::Small, &self.status);
        }
        self.screen.flush();
    }

    fn draw_importing(&mut self) {
        self.screen.clear();
        let top = self.screen.header("Import");
        let bottom = self.screen.footer("⌫ cancel");
        let text = format!(
            "Waiting for a connection on TCP port {}.\n\nFrom a computer on the same network:\n\
             printf 'name.prg\\n' | cat - game.prg | nc <device-ip> {}\n\n\
             ROMs: use the name rom:basic, rom:kernal or rom:chargen (exact sizes 8192, 8192, 4096 bytes).\n\
             Limit 64 KB; the connection is read for at most 30 s.",
            import::PORT,
            import::PORT
        );
        self.screen.para(top, bottom, GlyphStyle::Small, &text);
        self.screen.flush();
    }

    /// Draw the emulator screen. `full` repaints everything; otherwise only
    /// 8x8 cells that changed since the last frame are redrawn.
    fn draw_frame(&mut self, full: bool) {
        if !self.foreground {
            return;
        }
        let Some(m) = self.machine.as_ref() else { return };
        let size = self.screen.size();
        let o = origin(size.x);
        let border_lit = m.bus.vic.border_lit();
        let full = full || self.prev_border != Some(border_lit);

        if full {
            self.screen.clear();
            // Border bands (and side margins) in the border colour, lit = white.
            let colour = if border_lit { PixelColor::Light } else { PixelColor::Dark };
            let bottom = BAND + HEIGHT as isize + BAND;
            self.screen.fill(Point::new(0, 0), Point::new(size.x - 1, bottom - 1), colour);
        }

        let fb = m.framebuffer();
        let mut darks: Vec<(Point, Point)> = Vec::new();
        let mut lights: Vec<(Point, Point)> = Vec::new();
        let cells: Vec<(usize, usize)> = if full {
            darks.push((o, Point::new(o.x + WIDTH as isize - 1, o.y + HEIGHT as isize - 1)));
            (0..25 * 40).map(|i| (i % 40, i / 40)).collect()
        } else {
            changed_cells(&self.prev_fb, fb).collect()
        };
        for &(cx, cy) in &cells {
            let (x, y) = (o.x + cx as isize * 8, o.y + cy as isize * 8);
            if !full {
                darks.push((Point::new(x, y), Point::new(x + 7, y + 7)));
            }
            for r in 0..8usize {
                let byte = fb[(cy * 8 + r) * ROW_BYTES + cx];
                // Horizontal runs of lit pixels.
                let mut bit = 0u32;
                while bit < 8 {
                    if byte & (0x80 >> bit) == 0 {
                        bit += 1;
                        continue;
                    }
                    let start = bit;
                    while bit < 8 && byte & (0x80 >> bit) != 0 {
                        bit += 1;
                    }
                    let py = y + r as isize;
                    lights.push((Point::new(x + start as isize, py), Point::new(x + bit as isize - 1, py)));
                }
            }
        }
        self.screen.fill_rects(darks, PixelColor::Dark);
        self.screen.fill_rects(lights, PixelColor::Light);

        // Status line: drawn on full repaints and when the CPU jams.
        let jam = m.halt_reason();
        if full || (jam.is_some() && !self.jam_shown) {
            let y = BAND + HEIGHT as isize + BAND + 6;
            let line = jam.clone().unwrap_or_else(|| "F4 menu · Backspace = DEL · ` = RUN/STOP".into());
            self.screen.text(y, GlyphStyle::Small, &line);
            self.jam_shown = jam.is_some();
        }
        self.prev_fb.copy_from_slice(fb);
        self.prev_border = Some(border_lit);
        self.screen.flush();
    }
}

fn prg_error(e: &PrgError) -> String {
    match e {
        PrgError::TooShort => "too short".into(),
        PrgError::TooLarge => "runs past $FFFF".into(),
        PrgError::BadLoadAddress(a) => format!("load address ${a:04X} is in the zero page or stack"),
    }
}
