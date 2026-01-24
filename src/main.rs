#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod cpu;
mod memory;
mod vic;
mod cia;
mod keyboard;
mod loader;

use cpu::Cpu;
use memory::C64Memory;
use num_traits::*;
use gam::menu::*;

const SERVER_NAME: &str = "_C64 Emulator_";

// C64 display dimensions
const C64_WIDTH: usize = 320;
const C64_HEIGHT: usize = 200;

// Precursor display dimensions
const SCREEN_WIDTH: isize = 336;

// Scaling: C64 320x200 → center on 336x536 with 1:1 pixels + border
const DISPLAY_X_OFFSET: isize = (SCREEN_WIDTH - C64_WIDTH as isize) / 2; // 8
const DISPLAY_Y_OFFSET: isize = 20; // Leave room for status bar at top

// Cycles per frame: C64 runs at ~1MHz, we render at ~20fps = 50000 cycles per frame
const CYCLES_PER_FRAME: u32 = 50_000;

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
enum AppOp {
    Redraw = 0,
    Rawkeys,
    FocusChange,
    Quit,
}

#[derive(Clone, Copy, PartialEq)]
enum AppMode {
    Menu,      // Game selection menu
    Running,   // Emulator running
    Importing, // Waiting for TCP import
}

struct App {
    mode: AppMode,
    gam: gam::Gam,
    gid: gam::Gid,
    screensize: Point,
    cpu: Cpu,
    mem: C64Memory,
    allow_redraw: bool,
    game_list: Vec<String>,
    menu_cursor: usize,
    roms_loaded: bool,
    // Key state tracking for release timing
    pending_key: Option<(u8, u8)>,     // (col, row) of last pressed key
    pending_shift: bool,                // Whether shift was pressed with key
    key_hold_frames: u8,                // Frames to hold key before release
}

impl App {
    fn new(gam: gam::Gam, gid: gam::Gid, screensize: Point) -> Self {
        let mut cpu = Cpu::new();
        let mut mem = C64Memory::new();

        // Always load minimal ROMs at startup (no PDDB access needed)
        Self::generate_minimal_chargen(&mut mem);
        Self::generate_minimal_kernal(&mut mem);
        cpu.reset(&mem);

        App {
            mode: AppMode::Menu,
            gam,
            gid,
            screensize,
            cpu,
            mem,
            allow_redraw: true,
            game_list: Vec::new(),
            menu_cursor: 0,
            roms_loaded: true,
            pending_key: None,
            pending_shift: false,
            key_hold_frames: 0,
        }
    }

    /// Try to load C64 ROMs from PDDB (called lazily, not at startup)
    fn try_load_pddb_roms(mem: &mut C64Memory) {
        use std::io::{Read, Seek, SeekFrom};

        // Load each ROM in its own scope to avoid lifetime issues
        {
            let pddb = pddb::Pddb::new();
            let data = pddb.get("c64.roms", "basic", None, false, false, None, None::<fn()>)
                .ok()
                .and_then(|mut key| {
                    let mut buf = Vec::new();
                    key.seek(SeekFrom::Start(0)).ok();
                    key.read_to_end(&mut buf).ok();
                    if buf.len() == 8192 { Some(buf) } else { None }
                });
            if let Some(d) = data {
                mem.load_basic_rom(&d);
                log::info!("Loaded BASIC ROM from PDDB");
            }
        }

        {
            let pddb = pddb::Pddb::new();
            let data = pddb.get("c64.roms", "kernal", None, false, false, None, None::<fn()>)
                .ok()
                .and_then(|mut key| {
                    let mut buf = Vec::new();
                    key.seek(SeekFrom::Start(0)).ok();
                    key.read_to_end(&mut buf).ok();
                    if buf.len() == 8192 { Some(buf) } else { None }
                });
            if let Some(d) = data {
                mem.load_kernal_rom(&d);
                log::info!("Loaded KERNAL ROM from PDDB");
            }
        }

        {
            let pddb = pddb::Pddb::new();
            let data = pddb.get("c64.roms", "chargen", None, false, false, None, None::<fn()>)
                .ok()
                .and_then(|mut key| {
                    let mut buf = Vec::new();
                    key.seek(SeekFrom::Start(0)).ok();
                    key.read_to_end(&mut buf).ok();
                    if buf.len() == 4096 { Some(buf) } else { None }
                });
            if let Some(d) = data {
                mem.load_chargen_rom(&d);
                log::info!("Loaded CHARGEN ROM from PDDB");
            }
        }
    }

    /// Generate a minimal 4KB character ROM with basic ASCII glyphs
    fn generate_minimal_chargen(mem: &mut C64Memory) {
        // PETSCII character set: uppercase + graphics (standard C64 charset 1)
        // We'll generate simple 8x8 bitmap glyphs for printable ASCII subset
        // Character ROM has 256 chars × 8 bytes = 2048 bytes per charset, 2 charsets = 4096

        let font = generate_font();
        mem.load_chargen_rom(&font);
    }

    /// Generate a minimal KERNAL ROM that initializes the screen and enters a loop
    fn generate_minimal_kernal(mem: &mut C64Memory) {
        let mut kernal = [0u8; 8192];

        // KERNAL entry point is at $FCE2 (reset vector points here)
        // We'll set up a minimal init sequence:
        // 1. Initialize VIC registers
        // 2. Clear screen
        // 3. Print "READY." at screen memory ($0400)
        // 4. Loop forever (waiting for keyboard input)

        let entry = 0xFCE2 - 0xE000; // Offset in KERNAL ROM

        // Simple machine code at entry point:
        // SEI
        kernal[entry] = 0x78;
        // CLD
        kernal[entry + 1] = 0xD8;
        // LDX #$FF
        kernal[entry + 2] = 0xA2;
        kernal[entry + 3] = 0xFF;
        // TXS (set stack pointer)
        kernal[entry + 4] = 0x9A;

        // Initialize processor port for standard config
        // LDA #$2F; STA $00; LDA #$37; STA $01
        kernal[entry + 5] = 0xA9; kernal[entry + 6] = 0x2F;
        kernal[entry + 7] = 0x85; kernal[entry + 8] = 0x00;
        kernal[entry + 9] = 0xA9; kernal[entry + 10] = 0x37;
        kernal[entry + 11] = 0x85; kernal[entry + 12] = 0x01;

        // Initialize VIC: screen on, text mode
        // LDA #$1B; STA $D011
        kernal[entry + 13] = 0xA9; kernal[entry + 14] = 0x1B;
        kernal[entry + 15] = 0x8D; kernal[entry + 16] = 0x11; kernal[entry + 17] = 0xD0;
        // LDA #$08; STA $D016
        kernal[entry + 18] = 0xA9; kernal[entry + 19] = 0x08;
        kernal[entry + 20] = 0x8D; kernal[entry + 21] = 0x16; kernal[entry + 22] = 0xD0;
        // LDA #$14; STA $D018 (screen $0400, chars $1000)
        kernal[entry + 23] = 0xA9; kernal[entry + 24] = 0x14;
        kernal[entry + 25] = 0x8D; kernal[entry + 26] = 0x18; kernal[entry + 27] = 0xD0;
        // Background/border: LDA #$00; STA $D020; STA $D021
        kernal[entry + 28] = 0xA9; kernal[entry + 29] = 0x00;
        kernal[entry + 30] = 0x8D; kernal[entry + 31] = 0x20; kernal[entry + 32] = 0xD0;
        kernal[entry + 33] = 0x8D; kernal[entry + 34] = 0x21; kernal[entry + 35] = 0xD0;

        // Clear screen: fill $0400-$07E7 with space (0x20)
        // LDA #$20; LDX #$00
        kernal[entry + 36] = 0xA9; kernal[entry + 37] = 0x20;
        kernal[entry + 38] = 0xA2; kernal[entry + 39] = 0x00;
        // loop: STA $0400,X; STA $0500,X; STA $0600,X; STA $0700,X
        let loop_start = entry + 40;
        kernal[loop_start] = 0x9D; kernal[loop_start + 1] = 0x00; kernal[loop_start + 2] = 0x04;
        kernal[loop_start + 3] = 0x9D; kernal[loop_start + 4] = 0x00; kernal[loop_start + 5] = 0x05;
        kernal[loop_start + 6] = 0x9D; kernal[loop_start + 7] = 0x00; kernal[loop_start + 8] = 0x06;
        kernal[loop_start + 9] = 0x9D; kernal[loop_start + 10] = 0x00; kernal[loop_start + 11] = 0x07;
        // INX; BNE loop
        kernal[loop_start + 12] = 0xE8;
        kernal[loop_start + 13] = 0xD0;
        kernal[loop_start + 14] = (256 - 15) as u8; // Branch back to loop start

        // Set color RAM to light green (color 13)
        // LDA #$0D; LDX #$00
        let color_init = loop_start + 15;
        kernal[color_init] = 0xA9; kernal[color_init + 1] = 0x0D;
        kernal[color_init + 2] = 0xA2; kernal[color_init + 3] = 0x00;
        // loop2: STA $D800,X; STA $D900,X; STA $DA00,X; STA $DB00,X
        let loop2_start = color_init + 4;
        kernal[loop2_start] = 0x9D; kernal[loop2_start + 1] = 0x00; kernal[loop2_start + 2] = 0xD8;
        kernal[loop2_start + 3] = 0x9D; kernal[loop2_start + 4] = 0x00; kernal[loop2_start + 5] = 0xD9;
        kernal[loop2_start + 6] = 0x9D; kernal[loop2_start + 7] = 0x00; kernal[loop2_start + 8] = 0xDA;
        kernal[loop2_start + 9] = 0x9D; kernal[loop2_start + 10] = 0x00; kernal[loop2_start + 11] = 0xDB;
        // INX; BNE loop2
        kernal[loop2_start + 12] = 0xE8;
        kernal[loop2_start + 13] = 0xD0;
        kernal[loop2_start + 14] = (256 - 15) as u8;

        // Write "**** C64 EMULATOR ****" to screen at $0400 + 40*5 + 9 = $04D1
        let msg_offset = loop2_start + 15;
        let message: &[u8] = b"**** C64 EMULATOR ****";
        let screen_pos: u16 = 0x0400 + 40 * 1 + 9; // Row 1, centered
        // LDX #0; loop: LDA msg,X; BEQ done; STA screen,X; INX; BNE loop
        // For simplicity, just store each byte directly:
        let mut offset = msg_offset;
        for (i, &ch) in message.iter().enumerate() {
            // Convert ASCII to PETSCII screen code
            let screen_code = ascii_to_petscii_screen(ch);
            let addr = screen_pos + i as u16;
            // LDA #imm
            kernal[offset] = 0xA9; kernal[offset + 1] = screen_code;
            // STA abs
            kernal[offset + 2] = 0x8D;
            kernal[offset + 3] = (addr & 0xFF) as u8;
            kernal[offset + 4] = ((addr >> 8) & 0xFF) as u8;
            offset += 5;
        }

        // Write "READY." at row 3
        let ready_msg: &[u8] = b"READY.";
        let ready_pos: u16 = 0x0400 + 40 * 3;
        for (i, &ch) in ready_msg.iter().enumerate() {
            let screen_code = ascii_to_petscii_screen(ch);
            let addr = ready_pos + i as u16;
            kernal[offset] = 0xA9; kernal[offset + 1] = screen_code;
            kernal[offset + 2] = 0x8D;
            kernal[offset + 3] = (addr & 0xFF) as u8;
            kernal[offset + 4] = ((addr >> 8) & 0xFF) as u8;
            offset += 5;
        }

        // Infinite loop: JMP to self
        let loop_addr = 0xE000 + offset as u16;
        kernal[offset] = 0x4C; // JMP
        kernal[offset + 1] = (loop_addr & 0xFF) as u8;
        kernal[offset + 2] = ((loop_addr >> 8) & 0xFF) as u8;

        // Set reset vector at $FFFC-$FFFD
        kernal[0x1FFC] = 0xE2; // Low byte of $FCE2
        kernal[0x1FFD] = 0xFC; // High byte of $FCE2

        // IRQ/BRK vector at $FFFE-$FFFF (point to RTI)
        // Put RTI at $FF48
        kernal[0x1F48] = 0x40; // RTI
        kernal[0x1FFE] = 0x48; // Low byte of $FF48
        kernal[0x1FFF] = 0xFF; // High byte of $FF48

        // NMI vector at $FFFA-$FFFB (point to RTI)
        kernal[0x1FFA] = 0x48;
        kernal[0x1FFB] = 0xFF;

        mem.load_kernal_rom(&kernal);
    }

    fn handle_key(&mut self, key: char) {
        match self.mode {
            AppMode::Menu => self.handle_menu_key(key),
            AppMode::Running => self.handle_emulator_key(key),
            AppMode::Importing => {} // No key handling during import
        }
    }

    fn handle_menu_key(&mut self, key: char) {
        match key {
            '\u{2191}' => { // Up
                if self.menu_cursor > 0 {
                    self.menu_cursor -= 1;
                }
            }
            '\u{2193}' => { // Down
                let max = self.menu_items_count();
                if self.menu_cursor < max.saturating_sub(1) {
                    self.menu_cursor += 1;
                }
            }
            '\r' => { // Enter — select
                self.menu_select();
            }
            _ => {}
        }
    }

    fn menu_items_count(&self) -> usize {
        // Items: [games...] + "Import Game (TCP)" + "Boot C64 (no game)"
        self.game_list.len() + 2
    }

    fn menu_select(&mut self) {
        let pddb = pddb::Pddb::new();

        if self.menu_cursor < self.game_list.len() {
            // Load selected game
            let name = self.game_list[self.menu_cursor].clone();
            if let Some(data) = loader::load_game(&pddb, &name) {
                self.mem.load_prg(&data);
                self.cpu.reset(&self.mem);
                // Run enough cycles for KERNAL init, then trigger RUN
                self.mode = AppMode::Running;
            }
        } else if self.menu_cursor == self.game_list.len() {
            // Import game via TCP
            self.mode = AppMode::Importing;
            self.render_importing();
            match loader::tcp_import(&pddb) {
                Ok(name) => {
                    log::info!("Imported: {}", name);
                    self.game_list = loader::list_games(&pddb);
                }
                Err(e) => {
                    log::warn!("Import failed: {}", e);
                }
            }
            self.mode = AppMode::Menu;
        } else {
            // Boot C64 without game — try loading PDDB ROMs first
            Self::try_load_pddb_roms(&mut self.mem);
            self.cpu.reset(&self.mem);
            self.mode = AppMode::Running;
        }
    }

    fn handle_emulator_key(&mut self, key: char) {
        // Special: Esc+q returns to menu
        if key == '\u{2234}' {
            // Menu key → back to menu
            self.mode = AppMode::Menu;
            self.mem.cia1.release_all();
            return;
        }

        // Release previous key if held
        if let Some((col, row)) = self.pending_key.take() {
            self.mem.cia1.key_release(col, row);
            if self.pending_shift {
                self.mem.cia1.key_release(keyboard::LSHIFT.col, keyboard::LSHIFT.row);
                self.pending_shift = false;
            }
        }

        // Map Precursor key to C64 matrix position
        if let Some(pos) = keyboard::map_key(key) {
            let shift = keyboard::needs_shift(key);
            if shift {
                self.mem.cia1.key_press(keyboard::LSHIFT.col, keyboard::LSHIFT.row);
            }
            self.mem.cia1.key_press(pos.col, pos.row);
            self.pending_key = Some((pos.col, pos.row));
            self.pending_shift = shift;
            self.key_hold_frames = 3; // Hold for 3 frames (~150ms at 20fps)
        }
    }

    fn run_frame(&mut self) {
        if self.mode != AppMode::Running {
            return;
        }

        // Run CPU for one frame's worth of cycles
        let mut cycles_run: u32 = 0;
        while cycles_run < CYCLES_PER_FRAME {
            let c = self.cpu.step(&mut self.mem);
            cycles_run += c;
            self.cpu.cycles += c as u64;

            // Tick CIA timers
            if self.mem.tick_cia(c) {
                self.cpu.irq(&mut self.mem);
            }
        }

        // Handle key release after hold period
        if self.key_hold_frames > 0 {
            self.key_hold_frames -= 1;
            if self.key_hold_frames == 0 {
                if let Some((col, row)) = self.pending_key.take() {
                    self.mem.cia1.key_release(col, row);
                    if self.pending_shift {
                        self.mem.cia1.key_release(keyboard::LSHIFT.col, keyboard::LSHIFT.row);
                        self.pending_shift = false;
                    }
                }
            }
        }

        // Render VIC to framebuffer
        self.mem.render_vic();
    }

    fn render(&self) {
        if !self.allow_redraw {
            return;
        }

        match self.mode {
            AppMode::Menu => self.render_menu(),
            AppMode::Running => self.render_emulator(),
            AppMode::Importing => self.render_importing(),
        }
    }

    fn render_menu(&self) {
        // Clear screen
        self.gam.draw_rectangle(self.gid, Rectangle::new_with_style(
            Point::new(0, 0),
            self.screensize,
            DrawStyle { fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0 },
        )).ok();

        // Title
        let mut tv = TextView::new(
            self.gid,
            TextBounds::BoundingBox(Rectangle::new_coords(10, 10, 326, 50)),
        );
        tv.style = gam::GlyphStyle::Large;
        tv.clear_area = true;
        use std::fmt::Write;
        write!(tv.text, "C64 EMULATOR").unwrap();
        self.gam.post_textview(&mut tv).ok();

        // Game list
        let mut y = 60isize;
        for (i, name) in self.game_list.iter().enumerate() {
            let cursor = if i == self.menu_cursor { "> " } else { "  " };
            let mut tv = TextView::new(
                self.gid,
                TextBounds::BoundingBox(Rectangle::new_coords(10, y, 326, y + 22)),
            );
            tv.style = gam::GlyphStyle::Regular;
            tv.clear_area = true;
            write!(tv.text, "{}{}", cursor, name).unwrap();
            self.gam.post_textview(&mut tv).ok();
            y += 22;
        }

        // Import option
        let cursor = if self.menu_cursor == self.game_list.len() { "> " } else { "  " };
        let mut tv = TextView::new(
            self.gid,
            TextBounds::BoundingBox(Rectangle::new_coords(10, y, 326, y + 22)),
        );
        tv.style = gam::GlyphStyle::Regular;
        tv.clear_area = true;
        write!(tv.text, "{}Import Game (TCP :6464)", cursor).unwrap();
        self.gam.post_textview(&mut tv).ok();
        y += 22;

        // Boot without game
        let cursor = if self.menu_cursor == self.game_list.len() + 1 { "> " } else { "  " };
        let mut tv = TextView::new(
            self.gid,
            TextBounds::BoundingBox(Rectangle::new_coords(10, y, 326, y + 22)),
        );
        tv.style = gam::GlyphStyle::Regular;
        tv.clear_area = true;
        write!(tv.text, "{}Boot C64 (no game)", cursor).unwrap();
        self.gam.post_textview(&mut tv).ok();
        y += 30;

        // Instructions
        let mut tv = TextView::new(
            self.gid,
            TextBounds::BoundingBox(Rectangle::new_coords(10, y, 326, y + 40)),
        );
        tv.style = gam::GlyphStyle::Small;
        tv.clear_area = true;
        if self.roms_loaded {
            write!(tv.text, "Enter=select  Menu key=back to menu").unwrap();
        } else {
            write!(tv.text, "No ROMs loaded. Import via TCP.").unwrap();
        }
        self.gam.post_textview(&mut tv).ok();

        self.gam.redraw().ok();
    }

    fn render_emulator(&self) {
        // Clear screen
        self.gam.draw_rectangle(self.gid, Rectangle::new_with_style(
            Point::new(0, 0),
            self.screensize,
            DrawStyle { fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0 },
        )).ok();

        // Draw the C64 framebuffer to the Precursor display
        // VIC framebuffer is 320x200 monochrome, packed 1 bit per pixel
        // We render it as individual rectangles for filled regions (optimized: row by row)

        // Draw border around the C64 display area
        let border_bright = vic::Vic::color_to_mono(self.mem.vic.border_color());
        let border_color = if border_bright { PixelColor::Light } else { PixelColor::Dark };

        // Top border
        self.gam.draw_rectangle(self.gid, Rectangle::new_with_style(
            Point::new(0, 0),
            Point::new(SCREEN_WIDTH, DISPLAY_Y_OFFSET),
            DrawStyle { fill_color: Some(border_color), stroke_color: None, stroke_width: 0 },
        )).ok();

        // Bottom border
        let bottom_y = DISPLAY_Y_OFFSET + C64_HEIGHT as isize;
        self.gam.draw_rectangle(self.gid, Rectangle::new_with_style(
            Point::new(0, bottom_y),
            Point::new(SCREEN_WIDTH, bottom_y + DISPLAY_Y_OFFSET),
            DrawStyle { fill_color: Some(border_color), stroke_color: None, stroke_width: 0 },
        )).ok();

        // Render C64 framebuffer pixel-by-pixel using small rectangles
        // Optimization: scan for horizontal runs of same color
        for row in 0..C64_HEIGHT {
            let mut col = 0usize;
            while col < C64_WIDTH {
                let byte_idx = row * 40 + col / 8;
                let bit_idx = 7 - (col % 8);
                let pixel = (self.mem.vic.framebuffer[byte_idx] >> bit_idx) & 1 != 0;

                // Find run length of same pixel value
                let mut run_len = 1usize;
                let mut next_col = col + 1;
                while next_col < C64_WIDTH {
                    let nb = row * 40 + next_col / 8;
                    let nbit = 7 - (next_col % 8);
                    let np = (self.mem.vic.framebuffer[nb] >> nbit) & 1 != 0;
                    if np != pixel {
                        break;
                    }
                    run_len += 1;
                    next_col += 1;
                }

                // Draw the run as a rectangle
                // On Precursor: Dark = black ink, Light = white paper
                // C64 framebuffer: 1 = bright (character foreground), 0 = dark (background)
                let color = if pixel { PixelColor::Dark } else { PixelColor::Light };

                let x = DISPLAY_X_OFFSET + col as isize;
                let y = DISPLAY_Y_OFFSET + row as isize;

                self.gam.draw_rectangle(self.gid, Rectangle::new_with_style(
                    Point::new(x, y),
                    Point::new(x + run_len as isize, y + 1),
                    DrawStyle { fill_color: Some(color), stroke_color: None, stroke_width: 0 },
                )).ok();

                col = next_col;
            }
        }

        // Status bar below display
        let status_y = DISPLAY_Y_OFFSET + C64_HEIGHT as isize + DISPLAY_Y_OFFSET + 5;
        let mut tv = TextView::new(
            self.gid,
            TextBounds::BoundingBox(Rectangle::new_coords(
                10, status_y, 326, status_y + 18
            )),
        );
        tv.style = gam::GlyphStyle::Small;
        tv.clear_area = true;
        use std::fmt::Write;
        write!(tv.text, "C64  PC:${:04X}  Menu=exit", self.cpu.pc).unwrap();
        self.gam.post_textview(&mut tv).ok();

        self.gam.redraw().ok();
    }

    fn render_importing(&self) {
        self.gam.draw_rectangle(self.gid, Rectangle::new_with_style(
            Point::new(0, 0),
            self.screensize,
            DrawStyle { fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0 },
        )).ok();

        let mut tv = TextView::new(
            self.gid,
            TextBounds::BoundingBox(Rectangle::new_coords(10, 100, 326, 200)),
        );
        tv.style = gam::GlyphStyle::Regular;
        tv.clear_area = true;
        use std::fmt::Write;
        write!(tv.text, "Waiting for TCP connection\non port 6464...\n\ncat name.prg | nc <ip> 6464").unwrap();
        self.gam.post_textview(&mut tv).ok();

        self.gam.redraw().ok();
    }

    fn refresh_game_list(&mut self) {
        let pddb = pddb::Pddb::new();
        self.game_list = loader::list_games(&pddb);
    }
}

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("C64 emulator starting, PID {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let sid = xns.register_name(SERVER_NAME, None).expect("can't register server");

    let gam = gam::Gam::new(&xns).expect("can't connect to GAM");

    let token = gam.register_ux(gam::UxRegistration {
        app_name: String::from(SERVER_NAME),
        ux_type: gam::UxType::Chat,
        predictor: None,
        listener: sid.to_array(),
        redraw_id: AppOp::Redraw.to_u32().unwrap(),
        gotinput_id: None,
        audioframe_id: None,
        rawkeys_id: Some(AppOp::Rawkeys.to_u32().unwrap()),
        focuschange_id: Some(AppOp::FocusChange.to_u32().unwrap()),
    }).expect("couldn't register UX").unwrap();

    let content = gam.request_content_canvas(token).expect("couldn't get canvas");
    let screensize = gam.get_canvas_bounds(content).expect("couldn't get dimensions");

    let mut app = App::new(gam, content, screensize);
    // Don't access PDDB at startup — it may not be mounted yet.
    // Game list will be refreshed on first Foreground focus change.
    app.render();

    loop {
        let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(AppOp::Redraw) => {
                if app.allow_redraw {
                    if app.mode == AppMode::Running {
                        app.run_frame();
                    }
                    app.render();
                }
            }
            Some(AppOp::Rawkeys) => xous::msg_scalar_unpack!(msg, k1, k2, k3, k4, {
                let keys = [
                    core::char::from_u32(k1 as u32).unwrap_or('\u{0000}'),
                    core::char::from_u32(k2 as u32).unwrap_or('\u{0000}'),
                    core::char::from_u32(k3 as u32).unwrap_or('\u{0000}'),
                    core::char::from_u32(k4 as u32).unwrap_or('\u{0000}'),
                ];
                for &key in keys.iter() {
                    if key != '\u{0000}' {
                        app.handle_key(key);
                    }
                }
                if app.mode == AppMode::Running {
                    app.run_frame();
                }
                app.render();
            }),
            Some(AppOp::FocusChange) => xous::msg_scalar_unpack!(msg, new_state_code, _, _, _, {
                let new_state = gam::FocusState::convert_focus_change(new_state_code);
                match new_state {
                    gam::FocusState::Background => {
                        app.allow_redraw = false;
                    }
                    gam::FocusState::Foreground => {
                        app.allow_redraw = true;
                        app.refresh_game_list();
                        app.render();
                    }
                }
            }),
            Some(AppOp::Quit) => break,
            _ => log::error!("unknown opcode: {:?}", msg),
        }
    }

    xns.unregister_server(sid).unwrap();
    xous::destroy_server(sid).unwrap();
    xous::terminate_process(0)
}

/// Convert ASCII byte to PETSCII screen code
fn ascii_to_petscii_screen(ch: u8) -> u8 {
    match ch {
        b'@' => 0,
        b'A'..=b'Z' => ch - b'A' + 1,
        b'a'..=b'z' => ch - b'a' + 1, // Same as uppercase in standard charset
        b'0'..=b'9' => ch - b'0' + 48,
        b' ' => 32,
        b'!' => 33,
        b'"' => 34,
        b'#' => 35,
        b'$' => 36,
        b'%' => 37,
        b'&' => 38,
        b'\'' => 39,
        b'(' => 40,
        b')' => 41,
        b'*' => 42,
        b'+' => 43,
        b',' => 44,
        b'-' => 45,
        b'.' => 46,
        b'/' => 47,
        b':' => 58,
        b';' => 59,
        b'<' => 60,
        b'=' => 61,
        b'>' => 62,
        b'?' => 63,
        _ => 32, // Default to space
    }
}

/// Generate a minimal 4KB bitmap font for PETSCII characters
fn generate_font() -> [u8; 4096] {
    let mut font = [0u8; 4096];

    // Each character is 8 bytes (8x8 pixels)
    // We'll define glyphs for the most common characters

    // Helper: define a character glyph
    macro_rules! glyph {
        ($code:expr, $r0:expr, $r1:expr, $r2:expr, $r3:expr, $r4:expr, $r5:expr, $r6:expr, $r7:expr) => {
            let base = $code as usize * 8;
            font[base] = $r0;
            font[base+1] = $r1;
            font[base+2] = $r2;
            font[base+3] = $r3;
            font[base+4] = $r4;
            font[base+5] = $r5;
            font[base+6] = $r6;
            font[base+7] = $r7;
        };
    }

    // Space (code 32)
    glyph!(32, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00);

    // A (code 1 in PETSCII screen codes)
    glyph!(1,  0x18, 0x3C, 0x66, 0x66, 0x7E, 0x66, 0x66, 0x00);
    // B
    glyph!(2,  0x7C, 0x66, 0x66, 0x7C, 0x66, 0x66, 0x7C, 0x00);
    // C
    glyph!(3,  0x3C, 0x66, 0x60, 0x60, 0x60, 0x66, 0x3C, 0x00);
    // D
    glyph!(4,  0x78, 0x6C, 0x66, 0x66, 0x66, 0x6C, 0x78, 0x00);
    // E
    glyph!(5,  0x7E, 0x60, 0x60, 0x7C, 0x60, 0x60, 0x7E, 0x00);
    // F
    glyph!(6,  0x7E, 0x60, 0x60, 0x7C, 0x60, 0x60, 0x60, 0x00);
    // G
    glyph!(7,  0x3C, 0x66, 0x60, 0x6E, 0x66, 0x66, 0x3E, 0x00);
    // H
    glyph!(8,  0x66, 0x66, 0x66, 0x7E, 0x66, 0x66, 0x66, 0x00);
    // I
    glyph!(9,  0x3C, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00);
    // J
    glyph!(10, 0x1E, 0x0C, 0x0C, 0x0C, 0x0C, 0x6C, 0x38, 0x00);
    // K
    glyph!(11, 0x66, 0x6C, 0x78, 0x70, 0x78, 0x6C, 0x66, 0x00);
    // L
    glyph!(12, 0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x7E, 0x00);
    // M
    glyph!(13, 0x63, 0x77, 0x7F, 0x6B, 0x63, 0x63, 0x63, 0x00);
    // N
    glyph!(14, 0x66, 0x76, 0x7E, 0x7E, 0x6E, 0x66, 0x66, 0x00);
    // O
    glyph!(15, 0x3C, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x00);
    // P
    glyph!(16, 0x7C, 0x66, 0x66, 0x7C, 0x60, 0x60, 0x60, 0x00);
    // Q
    glyph!(17, 0x3C, 0x66, 0x66, 0x66, 0x6A, 0x6C, 0x36, 0x00);
    // R
    glyph!(18, 0x7C, 0x66, 0x66, 0x7C, 0x6C, 0x66, 0x66, 0x00);
    // S
    glyph!(19, 0x3C, 0x66, 0x60, 0x3C, 0x06, 0x66, 0x3C, 0x00);
    // T
    glyph!(20, 0x7E, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00);
    // U
    glyph!(21, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x00);
    // V
    glyph!(22, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x18, 0x00);
    // W
    glyph!(23, 0x63, 0x63, 0x63, 0x6B, 0x7F, 0x77, 0x63, 0x00);
    // X
    glyph!(24, 0x66, 0x66, 0x3C, 0x18, 0x3C, 0x66, 0x66, 0x00);
    // Y
    glyph!(25, 0x66, 0x66, 0x66, 0x3C, 0x18, 0x18, 0x18, 0x00);
    // Z
    glyph!(26, 0x7E, 0x06, 0x0C, 0x18, 0x30, 0x60, 0x7E, 0x00);

    // Numbers 0-9 (screen codes 48-57)
    glyph!(48, 0x3C, 0x66, 0x6E, 0x76, 0x66, 0x66, 0x3C, 0x00); // 0
    glyph!(49, 0x18, 0x38, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00); // 1
    glyph!(50, 0x3C, 0x66, 0x06, 0x1C, 0x30, 0x60, 0x7E, 0x00); // 2
    glyph!(51, 0x3C, 0x66, 0x06, 0x1C, 0x06, 0x66, 0x3C, 0x00); // 3
    glyph!(52, 0x0C, 0x1C, 0x3C, 0x6C, 0x7E, 0x0C, 0x0C, 0x00); // 4
    glyph!(53, 0x7E, 0x60, 0x7C, 0x06, 0x06, 0x66, 0x3C, 0x00); // 5
    glyph!(54, 0x1C, 0x30, 0x60, 0x7C, 0x66, 0x66, 0x3C, 0x00); // 6
    glyph!(55, 0x7E, 0x06, 0x0C, 0x18, 0x30, 0x30, 0x30, 0x00); // 7
    glyph!(56, 0x3C, 0x66, 0x66, 0x3C, 0x66, 0x66, 0x3C, 0x00); // 8
    glyph!(57, 0x3C, 0x66, 0x66, 0x3E, 0x06, 0x0C, 0x38, 0x00); // 9

    // Punctuation
    glyph!(33, 0x18, 0x18, 0x18, 0x18, 0x00, 0x00, 0x18, 0x00); // !
    glyph!(34, 0x66, 0x66, 0x24, 0x00, 0x00, 0x00, 0x00, 0x00); // "
    glyph!(40, 0x0C, 0x18, 0x30, 0x30, 0x30, 0x18, 0x0C, 0x00); // (
    glyph!(41, 0x30, 0x18, 0x0C, 0x0C, 0x0C, 0x18, 0x30, 0x00); // )
    glyph!(42, 0x00, 0x66, 0x3C, 0xFF, 0x3C, 0x66, 0x00, 0x00); // *
    glyph!(43, 0x00, 0x18, 0x18, 0x7E, 0x18, 0x18, 0x00, 0x00); // +
    glyph!(44, 0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x30); // ,
    glyph!(45, 0x00, 0x00, 0x00, 0x7E, 0x00, 0x00, 0x00, 0x00); // -
    glyph!(46, 0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00); // .
    glyph!(47, 0x02, 0x06, 0x0C, 0x18, 0x30, 0x60, 0x40, 0x00); // /
    glyph!(58, 0x00, 0x18, 0x18, 0x00, 0x18, 0x18, 0x00, 0x00); // :
    glyph!(59, 0x00, 0x18, 0x18, 0x00, 0x18, 0x18, 0x30, 0x00); // ;
    glyph!(61, 0x00, 0x00, 0x7E, 0x00, 0x7E, 0x00, 0x00, 0x00); // =
    glyph!(63, 0x3C, 0x66, 0x06, 0x0C, 0x18, 0x00, 0x18, 0x00); // ?

    // @ (code 0)
    glyph!(0,  0x3C, 0x66, 0x6E, 0x6A, 0x6E, 0x60, 0x3C, 0x00);

    // Cursor/block character (code 160 = reverse space, used for cursor)
    glyph!(160, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF);

    font
}
