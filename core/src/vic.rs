//! VIC-II video: text mode and standard hi-res bitmap mode, rendered once per
//! frame into a 320x200 1-bit framebuffer ("bright" = 1).
//!
//! Not implemented: sprites, multicolour, extended-colour text, scrolling
//! registers, border opening, raster interrupts, light pen. The raster counter
//! ($D011 bit 7 / $D012) is derived from the CPU cycle count so frame-sync
//! loops terminate.

pub const WIDTH: usize = 320;
pub const HEIGHT: usize = 200;
/// Bytes per framebuffer row.
pub const ROW_BYTES: usize = WIDTH / 8;
pub const FB_LEN: usize = ROW_BYTES * HEIGHT;

/// Cycles per raster line and lines per frame (PAL).
const CYCLES_PER_LINE: u64 = 63;
const LINES_PER_FRAME: u64 = 312;

/// Brightness threshold per colour: true maps to a lit (bright) pixel.
const COLOR_BRIGHT: [bool; 16] = [
    false, true, false, true, false, false, false, true, false, false, true, false, false, true, true, true,
];

/// Rough relative luminance (Pepto's values, scaled) used to keep text
/// readable when foreground and background fall on the same side of the threshold.
const COLOR_LUMA: [u8; 16] = [0, 32, 10, 20, 12, 16, 8, 24, 12, 8, 16, 12, 15, 24, 15, 20];

/// Map (foreground, background) colours to (fg_lit, bg_lit). Different
/// colours never map to the same value, so text stays visible.
pub fn mono_pair(fg: u8, bg: u8) -> (bool, bool) {
    let (fg, bg) = (fg as usize & 15, bg as usize & 15);
    let (fb, bb) = (COLOR_BRIGHT[fg], COLOR_BRIGHT[bg]);
    if fb == bb && fg != bg && COLOR_LUMA[fg] != COLOR_LUMA[bg] {
        let fg_lighter = COLOR_LUMA[fg] > COLOR_LUMA[bg];
        return (fg_lighter, !fg_lighter);
    }
    (fb, bb)
}

pub struct Vic {
    pub registers: [u8; 64],
    pub color_ram: Box<[u8]>,
    /// 320x200, 1 bit per pixel, 40 bytes per row.
    pub framebuffer: Box<[u8]>,
}

impl Default for Vic {
    fn default() -> Self { Self::new() }
}

impl Vic {
    pub fn new() -> Self {
        Vic { registers: [0; 64], color_ram: vec![0; 1024].into_boxed_slice(), framebuffer: vec![0; FB_LEN].into_boxed_slice() }
    }

    /// Raster line (0..312) for a given CPU cycle count.
    pub fn raster_line(cycles: u64) -> u16 { ((cycles / CYCLES_PER_LINE) % LINES_PER_FRAME) as u16 }

    pub fn read(&self, addr: u16, cycles: u64) -> u8 {
        match addr {
            0xD800..=0xDBFF => self.color_ram[(addr - 0xD800) as usize] | 0xF0,
            _ => {
                let reg = (addr & 0x3F) as usize;
                let raster = Self::raster_line(cycles);
                match reg {
                    0x11 => (self.registers[0x11] & 0x7F) | ((raster >> 1) & 0x80) as u8,
                    0x12 => raster as u8,
                    0x2F..=0x3F => 0xFF,
                    r => self.registers[r],
                }
            }
        }
    }

    pub fn write(&mut self, addr: u16, val: u8) {
        match addr {
            0xD800..=0xDBFF => self.color_ram[(addr - 0xD800) as usize] = val & 0x0F,
            0xD000..=0xD3FF => self.registers[(addr & 0x3F) as usize] = val,
            _ => {}
        }
    }

    pub fn is_bitmap_mode(&self) -> bool { self.registers[0x11] & 0x20 != 0 }

    pub fn bg_color(&self) -> u8 { self.registers[0x21] & 0x0F }

    pub fn border_color(&self) -> u8 { self.registers[0x20] & 0x0F }

    /// Is the border colour a lit pixel in the monochrome mapping?
    pub fn border_lit(&self) -> bool { COLOR_BRIGHT[self.border_color() as usize] }

    /// (bank base, screen offset, char/bitmap offset) in the VIC's 16 KB bank.
    pub fn memory_pointers(&self, cia2_port_a: u8) -> (usize, usize, usize) {
        let bank = ((!cia2_port_a) & 0x03) as usize;
        let d018 = self.registers[0x18];
        let screen = ((d018 >> 4) & 0x0F) as usize * 0x0400;
        let chars = if self.is_bitmap_mode() {
            (d018 & 0x08) as usize * 0x400 // bit 3 selects $0000 or $2000
        } else {
            ((d018 >> 1) & 0x07) as usize * 0x0800
        };
        (bank * 0x4000, screen, chars)
    }

    /// Read a byte as the VIC sees memory: the character ROM shows up at
    /// $1000-$1FFF in banks 0 and 2, everything else is RAM.
    fn vic_read(ram: &[u8], char_rom: &[u8], bank_base: usize, vic_addr: usize) -> u8 {
        let a = vic_addr & 0x3FFF;
        if (bank_base == 0x0000 || bank_base == 0x8000) && (0x1000..0x2000).contains(&a) {
            char_rom.get(a - 0x1000).copied().unwrap_or(0)
        } else {
            ram[bank_base + a]
        }
    }

    /// Render the current state. Cheap enough to do every frame: one byte
    /// operation per character cell row.
    pub fn render(&mut self, ram: &[u8], char_rom: &[u8], cia2_port_a: u8) {
        let (bank, screen, chars) = self.memory_pointers(cia2_port_a);
        let bg = self.bg_color();
        let bitmap = self.is_bitmap_mode();
        for row in 0..25usize {
            for col in 0..40usize {
                let cell = row * 40 + col;
                let sbyte = Self::vic_read(ram, char_rom, bank, screen + cell);
                let (fg_c, bg_c) = if bitmap {
                    (sbyte >> 4, sbyte & 0x0F)
                } else {
                    (self.color_ram[cell] & 0x0F, bg)
                };
                let (fg_lit, bg_lit) = mono_pair(fg_c, bg_c);
                let (fg_mask, bg_mask) = (if fg_lit { 0xFFu8 } else { 0 }, if bg_lit { 0xFFu8 } else { 0 });
                for cy in 0..8usize {
                    let bits = if bitmap {
                        Self::vic_read(ram, char_rom, bank, chars + cell * 8 + cy)
                    } else {
                        Self::vic_read(ram, char_rom, bank, chars + sbyte as usize * 8 + cy)
                    };
                    self.framebuffer[(row * 8 + cy) * ROW_BYTES + col] = (bits & fg_mask) | (!bits & bg_mask);
                }
            }
        }
    }
}

/// 8x8 cells (column, row) whose pixels differ between two framebuffers.
pub fn changed_cells<'a>(prev: &'a [u8], cur: &'a [u8]) -> impl Iterator<Item = (usize, usize)> + 'a {
    (0..25usize * 40).filter_map(move |i| {
        let (cx, cy) = (i % 40, i / 40);
        let differs = (0..8).any(|r| {
            let o = (cy * 8 + r) * ROW_BYTES + cx;
            prev.get(o) != cur.get(o)
        });
        differs.then_some((cx, cy))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (Vic, Vec<u8>, Vec<u8>) {
        let mut vic = Vic::new();
        vic.registers[0x18] = 0x14; // screen $0400, chars $1000 (ROM)
        vic.registers[0x21] = 0; // black background
        let mut rom = vec![0u8; 4096];
        for r in 0..8 {
            rom[8 + r] = 0xFF; // screen code 1: solid
            rom[0x800 + 8 + r] = 0xF0; // lower-case set, code 1: left half
        }
        (vic, vec![0u8; 65536], rom)
    }

    #[test]
    fn text_mode_uses_char_rom_at_1000() {
        let (mut vic, mut ram, rom) = setup();
        ram[0x0400] = 1;
        vic.color_ram[0] = 1; // white on black
        vic.render(&ram, &rom, 0x03);
        assert_eq!(vic.framebuffer[0], 0xFF);
        assert_eq!(vic.framebuffer[7 * ROW_BYTES], 0xFF);
        assert_eq!(vic.framebuffer[1], 0x00, "next cell is a blank code 0");
    }

    // Regression for C-7: $D018 = $17 selects the lower-case set at $1800.
    #[test]
    fn lowercase_charset_comes_from_rom_not_ram() {
        let (mut vic, mut ram, rom) = setup();
        vic.registers[0x18] = 0x17;
        ram[0x0400] = 1;
        ram[0x1808] = 0xAA; // RAM under the ROM must be ignored
        vic.color_ram[0] = 1;
        vic.render(&ram, &rom, 0x03);
        assert_eq!(vic.framebuffer[0], 0xF0);
    }

    // Regression for C-6: screen RAM writes show up without any VIC register write.
    #[test]
    fn screen_ram_change_is_rendered_every_time() {
        let (mut vic, mut ram, rom) = setup();
        vic.color_ram[0] = 1;
        vic.render(&ram, &rom, 0x03);
        assert_eq!(vic.framebuffer[0], 0);
        ram[0x0400] = 1; // POKE 1024,1
        vic.render(&ram, &rom, 0x03);
        assert_eq!(vic.framebuffer[0], 0xFF);
    }

    #[test]
    fn ram_charset_outside_rom_window() {
        let (mut vic, mut ram, rom) = setup();
        vic.registers[0x18] = 0x18; // chars at $2000 (RAM)
        ram[0x0400] = 2;
        ram[0x2000 + 16] = 0x81;
        vic.color_ram[0] = 1;
        vic.render(&ram, &rom, 0x03);
        assert_eq!(vic.framebuffer[0], 0x81);
    }

    #[test]
    fn bank_two_also_sees_char_rom() {
        let (mut vic, mut ram, rom) = setup();
        vic.registers[0x18] = 0x14;
        ram[0x8400] = 1;
        vic.color_ram[0] = 1;
        vic.render(&ram, &rom, 0x01); // CIA2 bits 01 -> bank 2
        assert_eq!(vic.framebuffer[0], 0xFF);
    }

    #[test]
    fn bitmap_mode_reads_colours_from_screen_ram() {
        let (mut vic, mut ram, rom) = setup();
        vic.registers[0x11] = 0x3B;
        vic.registers[0x18] = 0x18; // screen $0400, bitmap $2000
        ram[0x0400] = 0x10; // fg white, bg black
        ram[0x2000] = 0b1010_0000;
        vic.render(&ram, &rom, 0x03);
        assert_eq!(vic.framebuffer[0], 0b1010_0000);
        ram[0x0400] = 0x01; // fg black, bg white: inverted
        vic.render(&ram, &rom, 0x03);
        assert_eq!(vic.framebuffer[0], 0b0101_1111);
    }

    // Regression for C-11: foreground and background never collapse to one value.
    #[test]
    fn mono_pair_keeps_distinct_colours_distinct() {
        for fg in 0..16u8 {
            for bg in 0..16u8 {
                let (f, b) = mono_pair(fg, bg);
                let luma_differs = COLOR_LUMA[fg as usize] != COLOR_LUMA[bg as usize];
                if fg != bg && (COLOR_BRIGHT[fg as usize] != COLOR_BRIGHT[bg as usize] || luma_differs) {
                    assert_ne!(f, b, "fg {fg} bg {bg}");
                }
            }
        }
        // Blue text on black: both dark by threshold, but distinguishable.
        let (f, b) = mono_pair(6, 0);
        assert_ne!(f, b);
    }

    // Regression for C-9: raster advances with the cycle counter.
    #[test]
    fn raster_register_follows_cycles() {
        let vic = Vic::new();
        assert_eq!(vic.read(0xD012, 0), 0);
        assert_eq!(vic.read(0xD012, 63 * 10), 10);
        let line_300 = 63 * 300;
        assert_eq!(vic.read(0xD012, line_300), (300 & 0xFF) as u8);
        assert_eq!(vic.read(0xD011, line_300) & 0x80, 0x80, "bit 8 of the raster");
        assert_eq!(Vic::raster_line(63 * 312), 0, "wraps each frame");
    }

    #[test]
    fn registers_mirror_every_64_bytes() {
        let mut vic = Vic::new();
        vic.write(0xD040 + 0x20, 7); // $D060 mirrors $D020
        assert_eq!(vic.border_color(), 7);
    }

    #[test]
    fn changed_cells_finds_only_differences() {
        let a = vec![0u8; FB_LEN];
        let mut b = a.clone();
        b[3 * 8 * ROW_BYTES + 5] = 1; // cell (5, 3), first pixel row
        b[(24 * 8 + 7) * ROW_BYTES + 39] = 0x80; // cell (39, 24)
        let cells: Vec<_> = changed_cells(&a, &b).collect();
        assert_eq!(cells, vec![(5, 3), (39, 24)]);
        assert_eq!(changed_cells(&a, &a).count(), 0);
    }
}
