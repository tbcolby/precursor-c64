//! The whole machine: CPU + bus + key queue, a frame loop, and PRG launching.

use crate::cpu::{Cpu, MemAccess};
use crate::keyboard::{self, Edge, KeyQueue};
use crate::memory::C64Memory;
use crate::prg::{Prg, PrgError};
use crate::rom_stub;
use crate::vic;

/// Cycles per emulated frame (~50 ms of C64 time at 1 MHz).
pub const CYCLES_PER_FRAME: u32 = 50_000;

/// The KERNAL's keyboard-wait loop (`LDA $C6 / STA $CC / STA $0292 / BEQ`) in
/// the stock 901227-03 ROM: the machine is at the READY prompt when the PC is here.
const KERNAL_IDLE: core::ops::RangeInclusive<u16> = 0xE5CD..=0xE5D4;
/// Boot budget with real ROMs: the stock KERNAL reaches READY in about 2 million cycles.
pub const BOOT_BUDGET_CYCLES: u64 = 3_000_000;

/// Which ROMs are in the machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RomMode {
    /// Built-in stub: clears the screen, prints a banner, spins. No BASIC.
    Stub,
    /// User-provided BASIC + KERNAL.
    Real,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LaunchError {
    Prg(PrgError),
    /// A plain BASIC program (no `SYS` stub) needs the real BASIC ROM.
    NeedsBasicRom,
}

/// What `launch_prg` did.
#[derive(Debug, PartialEq, Eq)]
pub struct Launched {
    pub load: u16,
    /// How the program was started.
    pub how: Start,
    /// With real ROMs: false if the READY prompt was never detected within the
    /// boot budget (custom KERNAL?). The program was injected anyway.
    pub booted: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Start {
    /// Typed `RUN` into the keyboard buffer.
    Run,
    /// Jumped to this address (a `SYS` stub target, or the load address).
    Jump(u16),
}

pub struct Machine {
    pub cpu: Cpu,
    pub bus: C64Memory,
    keys: KeyQueue,
    /// Strokes currently applied to the matrix, so release removes shift too.
    shift_down: bool,
    pub mode: RomMode,
    stub_idle: u16,
    pub frames: u64,
}

/// Optional user ROMs.
#[derive(Default)]
pub struct RomSet<'a> {
    pub basic: Option<&'a [u8]>,
    pub kernal: Option<&'a [u8]>,
    pub chargen: Option<&'a [u8]>,
}

impl Machine {
    /// A machine with the built-in stub ROM and font.
    pub fn new() -> Self { Self::with_roms(&RomSet::default()) }

    /// BASIC and KERNAL are used only as a pair; the character ROM is separate
    /// (the built-in font stands in when it's absent).
    pub fn with_roms(roms: &RomSet) -> Self {
        let mut bus = C64Memory::new();
        let (stub, stub_idle) = rom_stub::stub_kernal();
        let mut mode = RomMode::Stub;
        match (roms.basic, roms.kernal) {
            (Some(b), Some(k)) if b.len() == crate::memory::BASIC_LEN && k.len() == crate::memory::KERNAL_LEN => {
                bus.load_basic_rom(b);
                bus.load_kernal_rom(k);
                mode = RomMode::Real;
            }
            _ => bus.load_kernal_rom(&stub),
        }
        match roms.chargen {
            Some(c) if c.len() == crate::memory::CHARGEN_LEN => bus.load_chargen_rom(c),
            _ => bus.load_chargen_rom(&rom_stub::builtin_font()),
        }
        let mut m = Machine { cpu: Cpu::new(), bus, keys: KeyQueue::default(), shift_down: false, mode, stub_idle, frames: 0 };
        m.cpu.reset(&m.bus);
        m
    }

    pub fn reset(&mut self) {
        self.keys.clear();
        self.bus.cia1.release_all();
        self.shift_down = false;
        self.cpu.reset(&self.bus);
    }

    /// Run at least `cycles` cycles. The CIA IRQ line is level-triggered: the
    /// CPU takes it whenever it is asserted and the I flag is clear.
    pub fn run_cycles(&mut self, cycles: u32) {
        let mut done = 0u32;
        while done < cycles {
            if self.cpu.halted {
                break;
            }
            let mut c = self.cpu.step(&mut self.bus);
            if self.bus.irq_line() && self.cpu.p & 0x04 == 0 {
                self.cpu.irq(&mut self.bus);
                c += 7;
            }
            done += c;
            self.cpu.cycles += c as u64;
            self.bus.tick_cia(c);
        }
    }

    /// One frame: run the CPU, advance the key queue, render the VIC.
    pub fn frame(&mut self) {
        self.run_cycles(CYCLES_PER_FRAME);
        self.apply_key_edge();
        self.bus.render_vic();
        self.frames += 1;
    }

    pub fn framebuffer(&self) -> &[u8] { &self.bus.vic.framebuffer }

    fn apply_key_edge(&mut self) {
        match self.keys.tick() {
            Edge::None => {}
            Edge::Press(s) => {
                if s.shift {
                    self.bus.cia1.key_press(keyboard::LSHIFT.col, keyboard::LSHIFT.row);
                    self.shift_down = true;
                }
                self.bus.cia1.key_press(s.pos.col, s.pos.row);
            }
            Edge::Release(s) => {
                self.bus.cia1.key_release(s.pos.col, s.pos.row);
                if self.shift_down {
                    self.bus.cia1.key_release(keyboard::LSHIFT.col, keyboard::LSHIFT.row);
                    self.shift_down = false;
                }
            }
        }
    }

    /// Queue a key press from a Precursor character. Returns false if it has no mapping.
    pub fn press_char(&mut self, ch: char) -> bool {
        match keyboard::map_key(ch) {
            Some(s) => {
                self.keys.push(s);
                true
            }
            None => false,
        }
    }

    /// Queue a C64 function key (1..3 = F1, F3, F5).
    pub fn press_function(&mut self, n: u8) -> bool {
        match keyboard::map_function_key(n) {
            Some(s) => {
                self.keys.push(s);
                true
            }
            None => false,
        }
    }

    /// Run until the KERNAL is waiting for a key (real ROMs) or the stub has
    /// finished initialising. Returns false if that wasn't seen in the budget.
    pub fn boot(&mut self) -> bool {
        self.reset();
        let start = self.cpu.cycles;
        while self.cpu.cycles - start < BOOT_BUDGET_CYCLES && !self.cpu.halted {
            self.run_cycles(2_000);
            let pc = self.cpu.pc;
            let idle = match self.mode {
                RomMode::Real => KERNAL_IDLE.contains(&pc),
                RomMode::Stub => pc == self.stub_idle,
            };
            if idle {
                self.bus.render_vic();
                return true;
            }
        }
        self.bus.render_vic();
        false
    }

    /// Boot, copy the program in and start it. See [`Start`].
    ///
    /// * Real ROMs, BASIC program: set the BASIC pointers and type `RUN`.
    /// * Programs with a `SYS` stub, or machine code at another address: jump
    ///   to the target. (With real ROMs the program is loaded as if by
    ///   `LOAD"x",8,1`, then `SYS`.)
    /// * Stub ROM, plain BASIC program: refused (`NeedsBasicRom`).
    pub fn launch_prg(&mut self, file: &[u8]) -> Result<Launched, LaunchError> {
        let prg = Prg::parse(file).map_err(LaunchError::Prg)?;
        let sys = prg.sys_target();
        if self.mode == RomMode::Stub && prg.load == 0x0801 && sys.is_none() {
            return Err(LaunchError::NeedsBasicRom);
        }
        let booted = self.boot();

        for (i, &b) in prg.data.iter().enumerate() {
            self.bus.ram[prg.load as usize + i] = b;
        }

        let how = if let Some(target) = sys.or_else(|| (prg.load != 0x0801).then_some(prg.load)) {
            self.cpu.pc = target;
            self.cpu.p &= !0x04; // the KERNAL leaves interrupts enabled
            Start::Jump(target)
        } else {
            // BASIC program: end-of-program pointers, then RUN.
            let end = prg.end() as u16;
            for (lo, hi) in [(0x2D, 0x2E), (0x2F, 0x30), (0x31, 0x32), (0xAE, 0xAF)] {
                self.bus.ram[lo] = end as u8;
                self.bus.ram[hi] = (end >> 8) as u8;
            }
            self.type_into_buffer(b"RUN\r");
            Start::Run
        };
        Ok(Launched { load: prg.load, how, booted })
    }

    /// Put PETSCII text straight into the KERNAL keyboard buffer ($0277, count at $C6; 10 max).
    pub fn type_into_buffer(&mut self, text: &[u8]) {
        let n = text.len().min(10);
        self.bus.ram[0x0277..0x0277 + n].copy_from_slice(&text[..n]);
        self.bus.ram[0xC6] = n as u8;
    }

    /// Why the CPU stopped, if it did.
    pub fn halt_reason(&self) -> Option<String> {
        self.cpu
            .halted
            .then(|| format!("CPU jammed: opcode ${:02X} at ${:04X}", self.cpu.last_opcode, self.cpu.halt_pc))
    }

    /// Read a byte as the CPU would.
    pub fn peek(&self, addr: u16) -> u8 { self.bus.read(addr) }
}

impl Default for Machine {
    fn default() -> Self { Self::new() }
}

/// Re-exported for the shell's drawing code.
pub use vic::{changed_cells, HEIGHT, WIDTH};

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic KERNAL (original code, not Commodore's): reset sets up the
    /// stack and VIC bank, then loops in the same place the stock ROM waits for
    /// keys. When the keyboard buffer is non-empty it copies the first byte to
    /// $07F8 and empties the buffer, so tests can see what was typed.
    fn fake_kernal() -> Vec<u8> {
        let mut k = vec![0xEAu8; 8192];
        let code: &[(u16, &[u8])] = &[
            // reset: SEI, CLD, LDX #$FF, TXS, CLI, JMP $E5CD
            (0xE000, &[0x78, 0xD8, 0xA2, 0xFF, 0x9A, 0x58, 0x4C, 0xCD, 0xE5]),
            // E5CD: LDA $C6; STA $CC; STA $0292; BEQ $E5CD
            (0xE5CD, &[0xA5, 0xC6, 0x85, 0xCC, 0x8D, 0x92, 0x02, 0xF0, 0xF7]),
            // E5D6: LDA $0277; STA $07F8; LDA #0; STA $C6; JMP $E5CD
            (0xE5D6, &[0xAD, 0x77, 0x02, 0x8D, 0xF8, 0x07, 0xA9, 0x00, 0x85, 0xC6, 0x4C, 0xCD, 0xE5]),
            // IRQ handler: RTI
            (0xFF48, &[0x40]),
        ];
        for (addr, bytes) in code {
            let o = (*addr - 0xE000) as usize;
            k[o..o + bytes.len()].copy_from_slice(bytes);
        }
        k[0x1FFC] = 0x00; // reset -> $E000
        k[0x1FFD] = 0xE0;
        k[0x1FFE] = 0x48; // IRQ -> $FF48
        k[0x1FFF] = 0xFF;
        k
    }

    fn real_machine() -> Machine {
        let basic = vec![0u8; 8192];
        let kernal = fake_kernal();
        Machine::with_roms(&RomSet { basic: Some(&basic), kernal: Some(&kernal), chargen: None })
    }

    fn basic_prg(body_len: usize) -> Vec<u8> {
        // 10 PRINT-ish: not a SYS line (token $99).
        let mut v = vec![0x01, 0x08, 0x0B, 0x08, 0x0A, 0x00, 0x99, 0x22, 0x41, 0x22, 0x00, 0x00, 0x00];
        v.resize(2 + body_len.max(11), 0);
        v
    }

    #[test]
    fn heap_allocation_keeps_struct_small() {
        assert!(std::mem::size_of::<Machine>() < 4096);
    }

    #[test]
    fn stub_machine_boots_and_draws_a_banner() {
        let mut m = Machine::new();
        assert!(m.boot(), "stub finishes initialisation");
        // Row 1 holds "**** C64 EMULATOR ****": screen RAM is non-blank there.
        assert_ne!(m.bus.ram[0x0400 + 40 + 9], 0x20);
        assert!(m.framebuffer().iter().any(|&b| b != 0));
    }

    #[test]
    fn boots_real_rom_to_idle_loop() {
        let mut m = real_machine();
        assert_eq!(m.mode, RomMode::Real);
        assert!(m.boot());
        assert!(KERNAL_IDLE.contains(&m.cpu.pc));
    }

    // Regression for C-2: a BASIC PRG sets pointers, and RUN reaches the keyboard buffer
    // after the machine has booted (so BASIC's cold start can't discard the program).
    #[test]
    fn basic_prg_gets_pointers_and_run_in_keyboard_buffer() {
        let mut m = real_machine();
        let prg = basic_prg(20);
        let l = m.launch_prg(&prg).unwrap();
        assert_eq!(l.how, Start::Run);
        assert!(l.booted);
        let end = 0x0801u16 + (prg.len() - 2) as u16;
        assert_eq!((m.bus.ram[0x2D], m.bus.ram[0x2E]), (end as u8, (end >> 8) as u8));
        assert_eq!((m.bus.ram[0xAE], m.bus.ram[0xAF]), (end as u8, (end >> 8) as u8));
        assert_eq!(m.bus.ram[0xC6], 4);
        assert_eq!(&m.bus.ram[0x0277..0x027B], b"RUN\r");
        assert_eq!(m.bus.ram[0x0801], 0x0B, "program bytes copied");
        // The (fake) KERNAL consumes the buffer: the first typed byte lands at $07F8.
        m.run_cycles(5_000);
        assert_eq!(m.bus.ram[0x07F8], b'R');
        assert_eq!(m.bus.ram[0xC6], 0);
    }

    #[test]
    fn machine_code_prg_is_entered_at_its_load_address() {
        let mut m = real_machine();
        // LDA #$42; STA $0400; JMP *
        let prg = [0x00, 0xC0, 0xA9, 0x42, 0x8D, 0x00, 0x04, 0x4C, 0x05, 0xC0];
        let l = m.launch_prg(&prg).unwrap();
        assert_eq!(l.how, Start::Jump(0xC000));
        m.run_cycles(100);
        assert_eq!(m.bus.ram[0x0400], 0x42);
    }

    #[test]
    fn sys_stub_prg_jumps_to_its_target_without_basic() {
        // Stub machine, BASIC loader "10 SYS 2061" followed by code at $080D = 2061:
        // LDA #$07; STA $D020; JMP *
        let mut prg = vec![0x01, 0x08, 0x0B, 0x08, 0x0A, 0x00, 0x9E];
        prg.extend_from_slice(b"2061");
        prg.extend_from_slice(&[0x00, 0x00, 0x00]);
        assert_eq!(0x0801 + prg.len() - 2, 0x080D);
        prg.extend_from_slice(&[0xA9, 0x07, 0x8D, 0x20, 0xD0, 0x4C, 0x12, 0x08]);
        let mut m = Machine::new();
        let l = m.launch_prg(&prg).unwrap();
        assert_eq!(l.how, Start::Jump(2061));
        m.run_cycles(100);
        assert_eq!(m.bus.vic.border_color(), 7);
    }

    #[test]
    fn plain_basic_needs_the_basic_rom() {
        let mut m = Machine::new();
        assert_eq!(m.launch_prg(&basic_prg(20)), Err(LaunchError::NeedsBasicRom));
    }

    #[test]
    fn bad_prg_is_reported() {
        let mut m = Machine::new();
        assert_eq!(m.launch_prg(&[1]), Err(LaunchError::Prg(PrgError::TooShort)));
    }

    #[test]
    fn partial_rom_set_falls_back_to_stub() {
        let basic = vec![0u8; 8192];
        let m = Machine::with_roms(&RomSet { basic: Some(&basic), kernal: None, chargen: None });
        assert_eq!(m.mode, RomMode::Stub);
        let m = Machine::with_roms(&RomSet { basic: Some(&basic[..100]), kernal: Some(&fake_kernal()), chargen: None });
        assert_eq!(m.mode, RomMode::Stub, "wrong size is rejected");
    }

    // The CIA timer interrupt is delivered level-triggered and acknowledged by reading $DC0D.
    #[test]
    fn timer_irq_is_delivered_and_acknowledged() {
        let mut m = real_machine();
        // Install an IRQ handler that bumps $0500 then reads $DC0D and returns:
        // INC $0500; LDA $DC0D; RTI
        let h = [0xEE, 0x00, 0x05, 0xAD, 0x0D, 0xDC, 0x40];
        let o = 0xFF48 - 0xE000;
        m.bus.roms.kernal[o..o + h.len()].copy_from_slice(&h);
        m.boot();
        m.bus.cia1.write(0xDC04, 200);
        m.bus.cia1.write(0xDC05, 0);
        m.bus.cia1.write(0xDC0D, 0x81);
        m.bus.cia1.write(0xDC0E, 0x11);
        m.run_cycles(1000);
        let n = m.bus.ram[0x0500];
        assert!((3..=6).contains(&n), "about 1000/201 interrupts, got {n}");
    }

    #[test]
    fn raster_wait_loop_terminates() {
        // Regression for C-9: LDA $D012; CMP #$80; BNE loop
        let mut m = Machine::new();
        m.boot();
        let code = [0xAD, 0x12, 0xD0, 0xC9, 0x80, 0xD0, 0xF9, 0x8D, 0x00, 0x06, 0x4C, 0x0A, 0xC0];
        m.bus.ram[0xC000..0xC000 + code.len()].copy_from_slice(&code);
        // STA $0600 stores A (== $80) once the raster reaches line 128.
        m.cpu.pc = 0xC000;
        m.cpu.p &= !0x04;
        m.run_cycles(20_000);
        assert_eq!(m.bus.ram[0x0600], 0x80);
    }

    #[test]
    fn jam_halts_with_a_reason() {
        let mut m = Machine::new();
        m.boot();
        m.bus.ram[0xC000] = 0x02;
        m.cpu.pc = 0xC000;
        m.run_cycles(100);
        assert!(m.cpu.halted);
        assert_eq!(m.halt_reason().unwrap(), "CPU jammed: opcode $02 at $C000");
        // Further frames don't run or panic.
        m.frame();
    }

    #[test]
    fn typed_keys_reach_the_matrix_with_release() {
        let mut m = Machine::new();
        assert!(m.press_char('a'));
        assert!(m.press_char('a'));
        assert!(!m.press_char('\u{F001}'));
        let mut transitions = 0;
        let mut last = m.bus.cia1.keyboard_matrix[1];
        for _ in 0..6 {
            m.apply_key_edge();
            if m.bus.cia1.keyboard_matrix[1] != last {
                transitions += 1;
                last = m.bus.cia1.keyboard_matrix[1];
            }
        }
        assert_eq!(transitions, 4, "down, up, down, up");
    }

    #[test]
    fn shifted_key_presses_left_shift_too() {
        let mut m = Machine::new();
        m.press_char('"');
        m.apply_key_edge();
        assert_eq!(m.bus.cia1.keyboard_matrix[1] & 0x80, 0, "left shift held");
        m.apply_key_edge();
        assert_eq!(m.bus.cia1.keyboard_matrix[1] & 0x80, 0x80, "released");
    }

    #[test]
    fn frame_renders_and_counts() {
        let mut m = Machine::new();
        m.frame();
        assert_eq!(m.frames, 1);
        assert_eq!(m.framebuffer().len(), vic::FB_LEN);
    }
}
