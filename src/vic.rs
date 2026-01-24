/// VIC-II video chip emulation
/// Supports text mode (40x25) and standard bitmap mode (320x200)

/// C64 color palette → monochrome brightness (0=dark, 1=light)
const COLOR_LUMA: [bool; 16] = [
    false, // 0: Black
    true,  // 1: White
    false, // 2: Red
    true,  // 3: Cyan
    false, // 4: Purple
    false, // 5: Green
    false, // 6: Blue
    true,  // 7: Yellow
    false, // 8: Orange
    false, // 9: Brown
    true,  // 10: Light red
    false, // 11: Dark grey
    false, // 12: Grey
    true,  // 13: Light green
    true,  // 14: Light blue
    true,  // 15: Light grey
];

pub struct Vic {
    pub registers: [u8; 64],
    pub color_ram: [u8; 1024], // $D800-$DBFF
    /// Rendered framebuffer: 320x200 monochrome (1 bit per pixel, packed into bytes)
    /// Row-major, 320/8 = 40 bytes per row, 200 rows = 8000 bytes
    pub framebuffer: [u8; 8000],
    pub dirty: bool,
}

impl Vic {
    pub fn new() -> Self {
        Vic {
            registers: [0; 64],
            color_ram: [0; 1024],
            framebuffer: [0; 8000],
            dirty: true,
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        let reg = (addr & 0x3F) as usize;
        match addr {
            0xD800..=0xDBFF => self.color_ram[(addr - 0xD800) as usize],
            _ => {
                if reg < 64 {
                    self.registers[reg]
                } else {
                    0xFF
                }
            }
        }
    }

    pub fn write(&mut self, addr: u16, val: u8) {
        match addr {
            0xD800..=0xDBFF => {
                self.color_ram[(addr - 0xD800) as usize] = val & 0x0F;
                self.dirty = true;
            }
            0xD000..=0xD03F => {
                let reg = (addr & 0x3F) as usize;
                self.registers[reg] = val;
                self.dirty = true;
            }
            _ => {}
        }
    }

    /// Is bitmap mode enabled? (VIC register $D011 bit 5)
    pub fn is_bitmap_mode(&self) -> bool {
        self.registers[0x11] & 0x20 != 0
    }

    /// Is extended color mode enabled? (VIC register $D011 bit 6)
    #[allow(dead_code)]
    pub fn is_ecm(&self) -> bool {
        self.registers[0x11] & 0x40 != 0
    }

    /// Is multicolor mode enabled? (VIC register $D016 bit 4)
    #[allow(dead_code)]
    pub fn is_multicolor(&self) -> bool {
        self.registers[0x16] & 0x10 != 0
    }

    /// Get background color (register $D021)
    pub fn bg_color(&self) -> u8 {
        self.registers[0x21] & 0x0F
    }

    /// Get border color (register $D020)
    pub fn border_color(&self) -> u8 {
        self.registers[0x20] & 0x0F
    }

    /// Get screen memory address from VIC bank and $D018
    /// Returns (screen_base, char_base) in CPU address space
    pub fn memory_pointers(&self, cia2_port_a: u8) -> (u16, u16) {
        // VIC bank from CIA2 port A bits 0-1 (inverted)
        let bank = ((!cia2_port_a) & 0x03) as u16;
        let bank_offset = bank * 0x4000;

        let d018 = self.registers[0x18];
        // Screen memory: bits 4-7 of $D018 × $0400
        let screen_offset = ((d018 >> 4) & 0x0F) as u16 * 0x0400;
        // Character memory: bits 1-3 of $D018 × $0800
        let char_offset = ((d018 >> 1) & 0x07) as u16 * 0x0800;

        (bank_offset + screen_offset, bank_offset + char_offset)
    }

    /// Render the current VIC state to the framebuffer
    pub fn render(&mut self, ram: &[u8; 65536], char_rom: &[u8], cia2_port_a: u8) {
        if !self.dirty {
            return;
        }
        self.dirty = false;

        let (screen_base, char_base) = self.memory_pointers(cia2_port_a);
        let bg_bright = COLOR_LUMA[self.bg_color() as usize];

        if self.is_bitmap_mode() {
            self.render_bitmap(ram, screen_base, char_base, bg_bright);
        } else {
            self.render_text(ram, char_rom, screen_base, char_base, bg_bright);
        }
    }

    fn render_text(
        &mut self,
        ram: &[u8; 65536],
        char_rom: &[u8],
        screen_base: u16,
        char_base: u16,
        bg_bright: bool,
    ) {
        for row in 0..25usize {
            for col in 0..40usize {
                let screen_addr = screen_base.wrapping_add((row * 40 + col) as u16);
                let char_code = ram[screen_addr as usize];
                let fg_color = self.color_ram[row * 40 + col] & 0x0F;
                let fg_bright = COLOR_LUMA[fg_color as usize];

                for cy in 0..8usize {
                    // Get character pixel row from char ROM or RAM
                    let char_data_addr = char_base.wrapping_add(char_code as u16 * 8 + cy as u16);
                    let pixel_row = if char_base == 0x1000 || char_base == 0x9000 {
                        // Character ROM is mapped at these addresses
                        let rom_offset = (char_code as usize) * 8 + cy;
                        if rom_offset < char_rom.len() {
                            char_rom[rom_offset]
                        } else {
                            0
                        }
                    } else {
                        ram[char_data_addr as usize]
                    };

                    let py = row * 8 + cy;
                    let fb_row_offset = py * 40; // 40 bytes per row

                    for cx in 0..8usize {
                        let px = col * 8 + cx;
                        let bit_set = (pixel_row >> (7 - cx)) & 1 != 0;
                        let bright = if bit_set { fg_bright } else { bg_bright };

                        // Set pixel in framebuffer
                        let byte_idx = fb_row_offset + px / 8;
                        let bit_idx = 7 - (px % 8);
                        if bright {
                            self.framebuffer[byte_idx] |= 1 << bit_idx;
                        } else {
                            self.framebuffer[byte_idx] &= !(1 << bit_idx);
                        }
                    }
                }
            }
        }
    }

    fn render_bitmap(
        &mut self,
        ram: &[u8; 65536],
        screen_base: u16,
        bitmap_base: u16,
        _bg_bright: bool,
    ) {
        // Standard hi-res bitmap mode: 320x200
        // Bitmap data at bitmap_base (8000 bytes)
        // Color info at screen_base (1000 bytes: hi-nybble=fg, lo-nybble=bg per 8x8 cell)
        for row in 0..25usize {
            for col in 0..40usize {
                let color_addr = screen_base.wrapping_add((row * 40 + col) as u16);
                let color_byte = ram[color_addr as usize];
                let fg_color = (color_byte >> 4) & 0x0F;
                let bg_color = color_byte & 0x0F;
                let fg_bright = COLOR_LUMA[fg_color as usize];
                let bg_bright = COLOR_LUMA[bg_color as usize];

                for cy in 0..8usize {
                    let bitmap_addr = bitmap_base
                        .wrapping_add((row * 40 * 8 + col * 8 + cy) as u16);
                    let pixel_row = ram[bitmap_addr as usize];

                    let py = row * 8 + cy;
                    let fb_row_offset = py * 40;

                    for cx in 0..8usize {
                        let px = col * 8 + cx;
                        let bit_set = (pixel_row >> (7 - cx)) & 1 != 0;
                        let bright = if bit_set { fg_bright } else { bg_bright };

                        let byte_idx = fb_row_offset + px / 8;
                        let bit_idx = 7 - (px % 8);
                        if bright {
                            self.framebuffer[byte_idx] |= 1 << bit_idx;
                        } else {
                            self.framebuffer[byte_idx] &= !(1 << bit_idx);
                        }
                    }
                }
            }
        }
    }

    /// Convert monochrome framebuffer color: true = set pixel (dark on precursor)
    pub fn color_to_mono(color: u8) -> bool {
        COLOR_LUMA[color as usize & 0x0F]
    }
}
