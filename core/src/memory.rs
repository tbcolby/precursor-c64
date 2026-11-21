//! The C64 system bus: 64 KB RAM, BASIC/KERNAL/character ROM, I/O, and the
//! $0001 bank-switching port. Large arrays are heap allocated: the Xous main
//! thread has a 32 KiB stack and this struct would be ~95 KB inline.

use crate::cia::{Cia1, Cia2};
use crate::cpu::MemAccess;
use crate::vic::Vic;

pub const BASIC_LEN: usize = 8192;
pub const KERNAL_LEN: usize = 8192;
pub const CHARGEN_LEN: usize = 4096;

/// ROM images (heap allocated).
pub struct Roms {
    pub basic: Box<[u8]>,   // $A000-$BFFF
    pub kernal: Box<[u8]>,  // $E000-$FFFF
    pub chargen: Box<[u8]>, // character ROM, 2 sets
}

impl Default for Roms {
    fn default() -> Self { Self::new() }
}

impl Roms {
    pub fn new() -> Self {
        Roms {
            basic: vec![0; BASIC_LEN].into_boxed_slice(),
            kernal: vec![0; KERNAL_LEN].into_boxed_slice(),
            chargen: vec![0; CHARGEN_LEN].into_boxed_slice(),
        }
    }
}

pub struct C64Memory {
    pub ram: Box<[u8]>,
    pub roms: Roms,
    pub vic: Box<Vic>,
    pub cia1: Cia1,
    pub cia2: Cia2,
    /// CPU cycles executed; drives the VIC raster counter.
    pub cycles: u64,
}

impl Default for C64Memory {
    fn default() -> Self { Self::new() }
}

impl C64Memory {
    pub fn new() -> Self {
        let mut mem = C64Memory {
            ram: vec![0; 65536].into_boxed_slice(),
            roms: Roms::new(),
            vic: Box::new(Vic::new()),
            cia1: Cia1::new(),
            cia2: Cia2::new(),
            cycles: 0,
        };
        mem.ram[0x0000] = 0x2F;
        mem.ram[0x0001] = 0x37;
        mem.vic.registers[0x11] = 0x1B;
        mem.vic.registers[0x16] = 0x08;
        mem.vic.registers[0x18] = 0x14;
        mem.vic.registers[0x20] = 0x0E;
        mem.vic.registers[0x21] = 0x06;
        mem
    }

    pub fn load_basic_rom(&mut self, data: &[u8]) {
        let len = data.len().min(BASIC_LEN);
        self.roms.basic[..len].copy_from_slice(&data[..len]);
    }

    pub fn load_kernal_rom(&mut self, data: &[u8]) {
        let len = data.len().min(KERNAL_LEN);
        self.roms.kernal[..len].copy_from_slice(&data[..len]);
    }

    pub fn load_chargen_rom(&mut self, data: &[u8]) {
        let len = data.len().min(CHARGEN_LEN);
        self.roms.chargen[..len].copy_from_slice(&data[..len]);
    }

    /// The effective processor port: output bits come from $0001, input bits
    /// float high (pull-ups).
    fn port(&self) -> u8 {
        let ddr = self.ram[0x0000];
        (self.ram[0x0001] & ddr) | !ddr
    }

    fn loram(&self) -> bool { self.port() & 0x01 != 0 }
    fn hiram(&self) -> bool { self.port() & 0x02 != 0 }
    fn charen(&self) -> bool { self.port() & 0x04 != 0 }

    fn io_visible(&self) -> bool { (self.hiram() || self.loram()) && self.charen() }

    /// Render the VIC framebuffer from the current RAM and character ROM.
    pub fn render_vic(&mut self) {
        let port_a = self.cia2.port_a;
        self.vic.render(&self.ram, &self.roms.chargen, port_a);
    }

    /// Advance the CIA timers; returns true while the IRQ line is asserted.
    pub fn tick_cia(&mut self, cycles: u32) -> bool {
        self.cycles += cycles as u64;
        self.cia1.tick(cycles)
    }

    pub fn irq_line(&self) -> bool { self.cia1.irq_line() }
}

impl MemAccess for C64Memory {
    fn read(&self, addr: u16) -> u8 {
        match addr {
            0x0000 => self.ram[0],
            0x0001 => self.port(),
            0xA000..=0xBFFF if self.loram() && self.hiram() => self.roms.basic[(addr - 0xA000) as usize],
            0xD000..=0xDFFF => {
                if !(self.hiram() || self.loram()) {
                    self.ram[addr as usize]
                } else if self.charen() {
                    match addr {
                        0xD000..=0xD3FF | 0xD800..=0xDBFF => self.vic.read(addr, self.cycles),
                        0xD400..=0xD7FF => 0, // SID: not emulated
                        0xDC00..=0xDCFF => self.cia1.read(addr),
                        0xDD00..=0xDDFF => self.cia2.read(addr),
                        _ => 0xFF, // expansion area
                    }
                } else {
                    self.roms.chargen[(addr - 0xD000) as usize]
                }
            }
            0xE000..=0xFFFF if self.hiram() => self.roms.kernal[(addr - 0xE000) as usize],
            _ => self.ram[addr as usize],
        }
    }

    fn write(&mut self, addr: u16, val: u8) {
        match addr {
            // I/O area: the write goes to the chip, not to the RAM beneath it.
            0xD000..=0xDFFF if self.io_visible() => match addr {
                0xD000..=0xD3FF | 0xD800..=0xDBFF => self.vic.write(addr, val),
                0xDC00..=0xDCFF => self.cia1.write(addr, val),
                0xDD00..=0xDDFF => self.cia2.write(addr, val),
                _ => {} // SID, expansion: ignored
            },
            // Everything else (including ROM areas) writes the RAM underneath.
            _ => self.ram[addr as usize] = val,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_with_roms() -> C64Memory {
        let mut m = C64Memory::new();
        m.roms.basic.fill(0xBA);
        m.roms.kernal.fill(0xEE);
        m.roms.chargen.fill(0xCC);
        m
    }

    #[test]
    fn default_map_shows_basic_kernal_and_io() {
        let m = mem_with_roms();
        assert_eq!(m.read(0xA000), 0xBA);
        assert_eq!(m.read(0xE000), 0xEE);
        assert_eq!(m.read(0xDC00), 0xFF, "CIA1 port A (idle)");
    }

    #[test]
    fn banking_via_port() {
        let mut m = mem_with_roms();
        m.ram[0xA000] = 1;
        m.ram[0xE000] = 2;
        m.write(0x0001, 0x35); // LORAM=1 HIRAM=0 CHAREN=1
        assert_eq!(m.read(0xA000), 1, "BASIC needs LORAM and HIRAM");
        assert_eq!(m.read(0xE000), 2, "KERNAL needs HIRAM");
        m.write(0x0001, 0x33); // CHAREN=0: character ROM visible
        m.ram[0x0001] = 0x33;
        assert_eq!(m.read(0xD000), 0xCC);
        m.ram[0x0001] = 0x30; // all ROMs/IO off
        assert_eq!(m.read(0xD000), 0);
    }

    #[test]
    fn input_bits_of_the_port_float_high() {
        let mut m = C64Memory::new();
        m.ram[0] = 0x00; // all inputs
        m.ram[1] = 0x00;
        assert_eq!(m.read(1), 0xFF);
        m.ram[0] = 0x2F;
        m.ram[1] = 0x00;
        assert_eq!(m.read(1) & 0x07, 0, "output bits follow $0001");
    }

    // Regression for C-14: writes to I/O do not land in the RAM underneath.
    #[test]
    fn io_writes_do_not_corrupt_ram_beneath() {
        let mut m = mem_with_roms();
        m.ram[0xD020] = 0x55;
        m.write(0xD020, 0x03);
        assert_eq!(m.ram[0xD020], 0x55);
        assert_eq!(m.vic.border_color(), 3);
        m.write(0xDC0E, 0x01);
        assert_eq!(m.ram[0xDC0E], 0);
        assert_eq!(m.cia1.cra, 0x01);
    }

    #[test]
    fn rom_area_writes_hit_ram() {
        let mut m = mem_with_roms();
        m.write(0xA000, 0x42);
        assert_eq!(m.ram[0xA000], 0x42);
        assert_eq!(m.read(0xA000), 0xBA, "ROM still visible");
        m.write(0xE123, 0x24);
        assert_eq!(m.ram[0xE123], 0x24);
    }

    #[test]
    fn color_ram_is_four_bits() {
        let mut m = C64Memory::new();
        m.write(0xD800, 0xAB);
        assert_eq!(m.read(0xD800) & 0x0F, 0x0B);
    }

    #[test]
    fn machine_state_is_heap_allocated() {
        // The struct itself must be small (stack safety on Xous: 32 KiB).
        assert!(std::mem::size_of::<C64Memory>() < 1024);
        assert!(std::mem::size_of::<crate::machine::Machine>() < 2048);
    }
}
