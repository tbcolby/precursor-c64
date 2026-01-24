/// C64 memory map with bank switching
/// 64KB RAM + BASIC ROM (8KB) + KERNAL ROM (8KB) + Character ROM (4KB)
/// I/O area: VIC-II, SID (stub), CIA1, CIA2

use crate::cpu::MemAccess;
use crate::vic::Vic;
use crate::cia::{Cia1, Cia2};

/// C64 ROM images
pub struct Roms {
    pub basic: [u8; 8192],   // $A000-$BFFF
    pub kernal: [u8; 8192],  // $E000-$FFFF
    pub chargen: [u8; 4096], // Character ROM (accessed via VIC or when I/O disabled)
}

impl Roms {
    pub fn new() -> Self {
        Roms {
            basic: [0; 8192],
            kernal: [0; 8192],
            chargen: [0; 4096],
        }
    }
}

/// The full C64 system bus
pub struct C64Memory {
    pub ram: [u8; 65536],
    pub roms: Roms,
    pub vic: Vic,
    pub cia1: Cia1,
    pub cia2: Cia2,
}

impl C64Memory {
    pub fn new() -> Self {
        let mut mem = C64Memory {
            ram: [0; 65536],
            roms: Roms::new(),
            vic: Vic::new(),
            cia1: Cia1::new(),
            cia2: Cia2::new(),
        };

        // Default processor port: BASIC + KERNAL + I/O enabled
        mem.ram[0x0000] = 0x2F; // Port direction register
        mem.ram[0x0001] = 0x37; // LORAM=1, HIRAM=1, CHAREN=1

        // Set default VIC registers for standard text mode
        mem.vic.registers[0x11] = 0x1B; // Screen on, 25 rows, text mode
        mem.vic.registers[0x16] = 0x08; // 40 columns
        mem.vic.registers[0x18] = 0x14; // Screen at $0400, chars at $1000
        mem.vic.registers[0x20] = 0x0E; // Border: light blue
        mem.vic.registers[0x21] = 0x06; // Background: blue

        mem
    }

    /// Load ROM data
    pub fn load_basic_rom(&mut self, data: &[u8]) {
        let len = data.len().min(8192);
        self.roms.basic[..len].copy_from_slice(&data[..len]);
    }

    pub fn load_kernal_rom(&mut self, data: &[u8]) {
        let len = data.len().min(8192);
        self.roms.kernal[..len].copy_from_slice(&data[..len]);
    }

    pub fn load_chargen_rom(&mut self, data: &[u8]) {
        let len = data.len().min(4096);
        self.roms.chargen[..len].copy_from_slice(&data[..len]);
    }

    /// Bank switching configuration from $0001
    /// Bits: LORAM (bit 0), HIRAM (bit 1), CHAREN (bit 2)
    fn loram(&self) -> bool { self.ram[0x0001] & 0x01 != 0 }
    fn hiram(&self) -> bool { self.ram[0x0001] & 0x02 != 0 }
    fn charen(&self) -> bool { self.ram[0x0001] & 0x04 != 0 }

    /// Load a .PRG file into RAM
    pub fn load_prg(&mut self, data: &[u8]) -> Option<u16> {
        if data.len() < 3 {
            return None;
        }
        let load_addr = u16::from_le_bytes([data[0], data[1]]);
        let program = &data[2..];

        for (i, &byte) in program.iter().enumerate() {
            let addr = load_addr as usize + i;
            if addr < 65536 {
                self.ram[addr] = byte;
            }
        }

        // Update BASIC pointers if loaded at $0801
        if load_addr == 0x0801 {
            let end_addr = load_addr + program.len() as u16;
            self.ram[0x2D] = (end_addr & 0xFF) as u8;
            self.ram[0x2E] = ((end_addr >> 8) & 0xFF) as u8;
            self.ram[0x2F] = self.ram[0x2D];
            self.ram[0x30] = self.ram[0x2E];
            self.ram[0x31] = self.ram[0x2D];
            self.ram[0x32] = self.ram[0x2E];
        }

        Some(load_addr)
    }

    /// Render the VIC framebuffer
    pub fn render_vic(&mut self) {
        let cia2_port_a = self.cia2.port_a;
        // Borrow chargen separately to avoid multiple mutable borrows
        let chargen: [u8; 4096] = self.roms.chargen;
        self.vic.render(&self.ram, &chargen, cia2_port_a);
    }

    /// Tick CIA timers, return true if IRQ pending
    pub fn tick_cia(&mut self, cycles: u32) -> bool {
        self.cia1.tick(cycles)
    }
}

impl MemAccess for C64Memory {
    fn read(&self, addr: u16) -> u8 {
        match addr {
            // Processor port
            0x0000 => self.ram[0x0000], // Data direction register
            0x0001 => self.ram[0x0001], // Processor port

            // BASIC ROM area ($A000-$BFFF)
            0xA000..=0xBFFF => {
                if self.loram() && self.hiram() {
                    self.roms.basic[(addr - 0xA000) as usize]
                } else {
                    self.ram[addr as usize]
                }
            }

            // I/O or Character ROM ($D000-$DFFF)
            0xD000..=0xDFFF => {
                if self.hiram() || self.loram() {
                    if self.charen() {
                        // I/O area
                        match addr {
                            0xD000..=0xD3FF => self.vic.read(addr),
                            0xD400..=0xD7FF => 0, // SID — stub
                            0xD800..=0xDBFF => self.vic.read(addr), // Color RAM via VIC
                            0xDC00..=0xDCFF => self.cia1.read(addr),
                            0xDD00..=0xDDFF => self.cia2.read(addr),
                            _ => self.ram[addr as usize],
                        }
                    } else {
                        // Character ROM
                        self.roms.chargen[(addr - 0xD000) as usize]
                    }
                } else {
                    self.ram[addr as usize]
                }
            }

            // KERNAL ROM ($E000-$FFFF)
            0xE000..=0xFFFF => {
                if self.hiram() {
                    self.roms.kernal[(addr - 0xE000) as usize]
                } else {
                    self.ram[addr as usize]
                }
            }

            // Regular RAM
            _ => self.ram[addr as usize],
        }
    }

    fn write(&mut self, addr: u16, val: u8) {
        // Writes always go to RAM underneath ROMs
        self.ram[addr as usize] = val;

        // But also dispatch to I/O if in I/O area
        match addr {
            0x0001 => {
                // Processor port write — already written to RAM above
            }
            0xD000..=0xDFFF if self.charen() && (self.hiram() || self.loram()) => {
                match addr {
                    0xD000..=0xD3FF => self.vic.write(addr, val),
                    0xD400..=0xD7FF => {} // SID — ignore
                    0xD800..=0xDBFF => self.vic.write(addr, val), // Color RAM
                    0xDC00..=0xDCFF => self.cia1.write(addr, val),
                    0xDD00..=0xDDFF => self.cia2.write(addr, val),
                    _ => {}
                }
            }
            _ => {}
        }
    }
}
