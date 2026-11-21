//! CIA emulation.
//!
//! CIA1: keyboard matrix, timers A and B (including "B counts A underflows"),
//! a free-running time-of-day clock, and a level-triggered IRQ line that is
//! cleared by reading the interrupt control register ($DC0D).
//! CIA2: VIC bank bits only. Its timers and the NMI line are not implemented.

use core::cell::Cell;

/// PAL: 985,248 cycles/s, so one TOD tenth is about 98,525 cycles.
const TOD_TENTH_CYCLES: u32 = 98_525;

pub struct Cia1 {
    /// Port A: keyboard column select (active low)
    pub port_a: u8,
    pub ddra: u8,
    pub ddrb: u8,
    /// Keyboard matrix: 8 columns, each byte has 8 row bits, 0 = pressed.
    pub keyboard_matrix: [u8; 8],
    pub timer_a: u16,
    pub timer_a_latch: u16,
    pub timer_b: u16,
    pub timer_b_latch: u16,
    pub cra: u8,
    pub crb: u8,
    /// Pending interrupt sources (bits 0-4). Cleared by reading $DC0D.
    icr_data: Cell<u8>,
    pub icr_mask: u8,
    /// Time of day, BCD: tenths, seconds, minutes, hours.
    pub tod: [u8; 4],
    tod_cycles: u32,
}

fn bcd_inc(v: u8, limit: u8) -> (u8, bool) {
    let mut n = v;
    if n & 0x0F == 9 {
        n = (n & 0xF0) + 0x10;
    } else {
        n += 1;
    }
    if n >= limit { (0, true) } else { (n, false) }
}

impl Default for Cia1 {
    fn default() -> Self { Self::new() }
}

impl Cia1 {
    pub fn new() -> Self {
        Cia1 {
            port_a: 0xFF,
            ddra: 0xFF,
            ddrb: 0x00,
            keyboard_matrix: [0xFF; 8],
            timer_a: 0xFFFF,
            timer_a_latch: 0xFFFF,
            timer_b: 0xFFFF,
            timer_b_latch: 0xFFFF,
            cra: 0,
            crb: 0,
            icr_data: Cell::new(0),
            icr_mask: 0,
            tod: [0, 0, 0, 0x01],
            tod_cycles: 0,
        }
    }

    /// The IRQ line: asserted while any enabled source is pending.
    pub fn irq_line(&self) -> bool { self.icr_data.get() & self.icr_mask & 0x1F != 0 }

    pub fn read(&self, addr: u16) -> u8 {
        match addr & 0x0F {
            0x00 => self.port_a,
            0x01 => self.scan_keyboard(),
            0x02 => self.ddra,
            0x03 => self.ddrb,
            0x04 => self.timer_a as u8,
            0x05 => (self.timer_a >> 8) as u8,
            0x06 => self.timer_b as u8,
            0x07 => (self.timer_b >> 8) as u8,
            0x08..=0x0B => self.tod[(addr & 0x0F) as usize - 8],
            0x0D => {
                // Reading returns the pending flags (bit 7 = an enabled source
                // is pending) and acknowledges them.
                let data = self.icr_data.get();
                self.icr_data.set(0);
                data | if data & self.icr_mask & 0x1F != 0 { 0x80 } else { 0 }
            }
            0x0E => self.cra,
            0x0F => self.crb,
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, val: u8) {
        match addr & 0x0F {
            0x00 => self.port_a = val,
            0x01 => {}
            0x02 => self.ddra = val,
            0x03 => self.ddrb = val,
            0x04 => self.timer_a_latch = (self.timer_a_latch & 0xFF00) | val as u16,
            0x05 => {
                self.timer_a_latch = (self.timer_a_latch & 0x00FF) | ((val as u16) << 8);
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
            0x08..=0x0B => {
                // Writes set the clock (alarm mode, CRB bit 7, is not implemented).
                if self.crb & 0x80 == 0 {
                    self.tod[(addr & 0x0F) as usize - 8] = val;
                }
            }
            0x0D => {
                if val & 0x80 != 0 {
                    self.icr_mask |= val & 0x1F;
                } else {
                    self.icr_mask &= !(val & 0x1F);
                }
            }
            0x0E => {
                if val & 0x10 != 0 {
                    self.timer_a = self.timer_a_latch;
                }
                self.cra = val & 0xEF;
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

    fn scan_keyboard(&self) -> u8 {
        let col_mask = !self.port_a;
        let mut result = 0xFF;
        for col in 0..8u8 {
            if (col_mask & (1 << col)) != 0 {
                result &= self.keyboard_matrix[col as usize];
            }
        }
        result
    }

    pub fn key_press(&mut self, col: u8, row: u8) {
        if col < 8 && row < 8 {
            self.keyboard_matrix[col as usize] &= !(1 << row);
        }
    }

    pub fn key_release(&mut self, col: u8, row: u8) {
        if col < 8 && row < 8 {
            self.keyboard_matrix[col as usize] |= 1 << row;
        }
    }

    pub fn release_all(&mut self) { self.keyboard_matrix = [0xFF; 8]; }

    fn raise(&self, bit: u8) { self.icr_data.set(self.icr_data.get() | bit); }

    /// Advance a timer by `cycles` input pulses. Returns how many times it
    /// underflowed. A counter at value v underflows on the (v+1)th pulse, then
    /// reloads from the latch (the remainder carries over); one-shot mode stops.
    fn run_timer(counter: &mut u16, latch: u16, control: &mut u8, mut pulses: u32) -> u32 {
        let mut underflows = 0;
        while pulses > 0 && *control & 0x01 != 0 {
            if pulses <= *counter as u32 {
                *counter -= pulses as u16;
                break;
            }
            pulses -= *counter as u32 + 1;
            underflows += 1;
            *counter = latch;
            if *control & 0x08 != 0 {
                *control &= !0x01; // one-shot
            }
        }
        underflows
    }

    /// Advance the chip by `cycles`. Returns true if the IRQ line is asserted.
    pub fn tick(&mut self, cycles: u32) -> bool {
        let a_under = Self::run_timer(&mut self.timer_a, self.timer_a_latch, &mut self.cra, cycles);
        if a_under > 0 {
            self.raise(0x01);
        }
        let b_pulses = match (self.crb >> 5) & 3 {
            0 => cycles,
            2 | 3 => a_under,
            _ => 0, // counting CNT pulses: no external input
        };
        let b_under = Self::run_timer(&mut self.timer_b, self.timer_b_latch, &mut self.crb, b_pulses);
        if b_under > 0 {
            self.raise(0x02);
        }

        self.tod_cycles += cycles;
        while self.tod_cycles >= TOD_TENTH_CYCLES {
            self.tod_cycles -= TOD_TENTH_CYCLES;
            self.tod_step();
        }
        self.irq_line()
    }

    fn tod_step(&mut self) {
        let (t, c) = bcd_inc(self.tod[0], 0x10);
        self.tod[0] = t;
        if !c {
            return;
        }
        let (s, c) = bcd_inc(self.tod[1], 0x60);
        self.tod[1] = s;
        if !c {
            return;
        }
        let (m, c) = bcd_inc(self.tod[2], 0x60);
        self.tod[2] = m;
        if !c {
            return;
        }
        // Hours: 1-12 with the PM flag in bit 7.
        let pm = self.tod[3] & 0x80;
        let (h, pm) = match self.tod[3] & 0x1F {
            0x11 => (0x12, pm ^ 0x80),
            0x12 => (0x01, pm),
            h => (bcd_inc(h, 0x13).0, pm),
        };
        self.tod[3] = h | pm;
    }
}

pub struct Cia2 {
    /// Bits 0-1: VIC bank (inverted)
    pub port_a: u8,
    pub ddra: u8,
}

impl Default for Cia2 {
    fn default() -> Self { Self::new() }
}

impl Cia2 {
    pub fn new() -> Self { Cia2 { port_a: 0x03, ddra: 0x03 } }

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

#[cfg(test)]
mod tests {
    use super::*;

    fn running_a(latch: u16) -> Cia1 {
        let mut c = Cia1::new();
        c.write(0xDC04, latch as u8);
        c.write(0xDC05, (latch >> 8) as u8);
        c.write(0xDC0D, 0x81); // enable timer A interrupt
        c.write(0xDC0E, 0x11); // start + force load
        c
    }

    #[test]
    fn timer_underflows_after_latch_plus_one_cycles() {
        let mut c = running_a(9);
        assert!(!c.tick(9));
        assert_eq!(c.timer_a, 0);
        assert!(c.tick(1), "underflow on the 10th cycle");
        assert_eq!(c.timer_a, 9, "reloaded");
    }

    #[test]
    fn remainder_carries_across_reload() {
        let mut c = running_a(9);
        c.tick(10 + 4);
        assert_eq!(c.timer_a, 5, "14 cycles = one period (10) + 4");
    }

    // Regression for C-15: reading $DC0D acknowledges the interrupt.
    #[test]
    fn icr_read_clears_and_drops_the_line() {
        let mut c = running_a(4);
        assert!(c.tick(10));
        assert!(c.irq_line());
        let v = c.read(0xDC0D);
        assert_eq!(v & 0x81, 0x81);
        assert!(!c.irq_line());
        assert_eq!(c.read(0xDC0D), 0, "second read sees nothing");
    }

    #[test]
    fn line_stays_asserted_until_read() {
        let mut c = running_a(4);
        c.tick(10);
        // Level-triggered: still asserted on the next tick without a read.
        assert!(c.tick(1));
    }

    #[test]
    fn masked_source_does_not_assert_but_is_readable() {
        let mut c = Cia1::new();
        c.write(0xDC04, 3);
        c.write(0xDC05, 0);
        c.write(0xDC0E, 0x11);
        assert!(!c.tick(10));
        let v = c.read(0xDC0D);
        assert_eq!(v, 0x01, "flag visible, bit 7 clear");
    }

    #[test]
    fn one_shot_stops() {
        let mut c = running_a(3);
        c.write(0xDC0E, 0x19); // start, one-shot, force load
        c.tick(100);
        assert_eq!(c.cra & 1, 0, "stopped");
    }

    #[test]
    fn timer_b_counts_timer_a_underflows() {
        let mut c = Cia1::new();
        c.write(0xDC04, 1);
        c.write(0xDC05, 0);
        c.write(0xDC06, 2);
        c.write(0xDC07, 0);
        c.write(0xDC0D, 0x82); // timer B interrupt
        c.write(0xDC0F, 0x51); // start, count timer A underflows, force load
        c.write(0xDC0E, 0x11);
        // A period is 2 cycles. B needs 3 underflows (2+1) = 6 cycles.
        assert!(!c.tick(4));
        assert!(c.tick(2));
        assert_eq!(c.read(0xDC0D) & 0x02, 0x02);
    }

    #[test]
    fn tod_ticks_and_rolls_over() {
        let mut c = Cia1::new();
        c.tick(TOD_TENTH_CYCLES * 10);
        assert_eq!(c.read(0xDC09), 0x01, "one second");
        assert_eq!(c.read(0xDC08), 0x00);
        c.write(0xDC09, 0x59);
        c.tick(TOD_TENTH_CYCLES * 10);
        assert_eq!(c.read(0xDC09), 0x00);
        assert_eq!(c.read(0xDC0A), 0x01, "minute carried");
    }

    #[test]
    fn keyboard_scan_follows_column_select() {
        let mut c = Cia1::new();
        c.key_press(1, 2); // A
        c.write(0xDC00, !(1 << 1)); // select column 1
        assert_eq!(c.read(0xDC01), !(1 << 2));
        c.write(0xDC00, !(1 << 3));
        assert_eq!(c.read(0xDC01), 0xFF);
        c.write(0xDC00, 0x00); // all columns
        assert_eq!(c.read(0xDC01), !(1 << 2));
        c.key_release(1, 2);
        assert_eq!(c.read(0xDC01), 0xFF);
    }
}
