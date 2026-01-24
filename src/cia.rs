/// CIA (Complex Interface Adapter) emulation
/// CIA1: Keyboard matrix scanning + Timer A/B
/// CIA2: VIC bank selection + serial port (stub)

pub struct Cia1 {
    /// Port A: keyboard column select (directly written/read)
    pub port_a: u8,
    /// Port A data direction register
    pub ddra: u8,
    /// Port B data direction register
    pub ddrb: u8,
    /// Keyboard matrix state: 8 columns, each byte has 8 row bits
    /// Active LOW: 0 = key pressed, 1 = key released
    pub keyboard_matrix: [u8; 8],
    /// Timer A
    pub timer_a: u16,
    pub timer_a_latch: u16,
    /// Timer B
    pub timer_b: u16,
    pub timer_b_latch: u16,
    /// Control registers
    pub cra: u8,
    pub crb: u8,
    /// Interrupt control/status
    pub icr: u8,
    pub icr_mask: u8,
}

impl Cia1 {
    pub fn new() -> Self {
        Cia1 {
            port_a: 0xFF,
            ddra: 0xFF,
            ddrb: 0x00,
            keyboard_matrix: [0xFF; 8], // All keys released
            timer_a: 0xFFFF,
            timer_a_latch: 0xFFFF,
            timer_b: 0xFFFF,
            timer_b_latch: 0xFFFF,
            cra: 0,
            crb: 0,
            icr: 0,
            icr_mask: 0,
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr & 0x0F {
            0x00 => self.port_a, // Port A (column select)
            0x01 => self.scan_keyboard(), // Port B (row result)
            0x02 => self.ddra,
            0x03 => self.ddrb,
            0x04 => self.timer_a as u8,       // Timer A lo
            0x05 => (self.timer_a >> 8) as u8, // Timer A hi
            0x06 => self.timer_b as u8,       // Timer B lo
            0x07 => (self.timer_b >> 8) as u8, // Timer B hi
            0x0D => {
                let val = self.icr;
                val
            }
            0x0E => self.cra,
            0x0F => self.crb,
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, val: u8) {
        match addr & 0x0F {
            0x00 => self.port_a = val,
            0x01 => {} // Port B is read-only for keyboard
            0x02 => self.ddra = val,
            0x03 => self.ddrb = val,
            0x04 => self.timer_a_latch = (self.timer_a_latch & 0xFF00) | val as u16,
            0x05 => {
                self.timer_a_latch = (self.timer_a_latch & 0x00FF) | ((val as u16) << 8);
                // Writing hi byte loads latch into timer if timer stopped
                if self.cra & 0x01 == 0 {
                    self.timer_a = self.timer_a_latch;
                }
            }
            0x06 => self.timer_b_latch = (self.timer_b_latch & 0xFF00) | val as u16,
            0x07 => {
                self.timer_b_latch = (self.timer_b_latch & 0x00FF) | ((val as u16) << 8);
                if self.crb & 0x01 == 0 {
                    self.timer_b = self.timer_b_latch;
                }
            }
            0x0D => {
                // Interrupt control: bit 7 = set/clear, bits 0-4 = mask
                if val & 0x80 != 0 {
                    self.icr_mask |= val & 0x1F;
                } else {
                    self.icr_mask &= !(val & 0x1F);
                }
            }
            0x0E => {
                // Force load timer A if bit 4 set
                if val & 0x10 != 0 {
                    self.timer_a = self.timer_a_latch;
                }
                self.cra = val & 0xEF; // Clear force-load bit
            }
            0x0F => {
                if val & 0x10 != 0 {
                    self.timer_b = self.timer_b_latch;
                }
                self.crb = val & 0xEF;
            }
            _ => {}
        }
    }

    /// Scan the keyboard matrix based on current port_a column selection
    fn scan_keyboard(&self) -> u8 {
        let col_mask = !self.port_a; // Active LOW: 0 in port_a means column selected
        let mut result = 0xFF;

        for col in 0..8u8 {
            if (col_mask & (1 << col)) != 0 {
                result &= self.keyboard_matrix[col as usize];
            }
        }
        result
    }

    /// Press a key at the given matrix position (col, row)
    pub fn key_press(&mut self, col: u8, row: u8) {
        if col < 8 && row < 8 {
            self.keyboard_matrix[col as usize] &= !(1 << row); // Active LOW
        }
    }

    /// Release a key at the given matrix position (col, row)
    pub fn key_release(&mut self, col: u8, row: u8) {
        if col < 8 && row < 8 {
            self.keyboard_matrix[col as usize] |= 1 << row;
        }
    }

    /// Release all keys
    pub fn release_all(&mut self) {
        self.keyboard_matrix = [0xFF; 8];
    }

    /// Tick timers by N cycles, return true if IRQ should fire
    pub fn tick(&mut self, cycles: u32) -> bool {
        let mut irq = false;

        // Timer A
        if self.cra & 0x01 != 0 {
            // Timer running
            let (new_val, underflow) = self.timer_a.overflowing_sub(cycles as u16);
            if underflow || new_val == 0 {
                self.icr |= 0x01; // Timer A underflow flag
                if self.cra & 0x08 != 0 {
                    // One-shot: stop timer
                    self.cra &= !0x01;
                }
                self.timer_a = self.timer_a_latch;
                if self.icr_mask & 0x01 != 0 {
                    self.icr |= 0x80;
                    irq = true;
                }
            } else {
                self.timer_a = new_val;
            }
        }

        // Timer B
        if self.crb & 0x01 != 0 {
            let (new_val, underflow) = self.timer_b.overflowing_sub(cycles as u16);
            if underflow || new_val == 0 {
                self.icr |= 0x02;
                if self.crb & 0x08 != 0 {
                    self.crb &= !0x01;
                }
                self.timer_b = self.timer_b_latch;
                if self.icr_mask & 0x02 != 0 {
                    self.icr |= 0x80;
                    irq = true;
                }
            } else {
                self.timer_b = new_val;
            }
        }

        irq
    }
}

pub struct Cia2 {
    pub port_a: u8, // Bits 0-1: VIC bank (inverted)
    pub ddra: u8,
}

impl Cia2 {
    pub fn new() -> Self {
        Cia2 {
            port_a: 0x03, // Default: bank 0 (bits inverted: 11 → bank 0)
            ddra: 0x03,
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr & 0x0F {
            0x00 => self.port_a,
            0x02 => self.ddra,
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, val: u8) {
        match addr & 0x0F {
            0x00 => self.port_a = val,
            0x02 => self.ddra = val,
            _ => {}
        }
    }
}
