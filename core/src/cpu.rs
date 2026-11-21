//! MOS 6502 CPU: all 151 legal opcodes, the stable undocumented ones
//! (LAX, SAX, DCP, ISC, SLO, RLA, SRE, RRA, ANC, ALR, ARR, SBX, SBC #) and
//! correct-length NOPs. JAM opcodes halt the CPU. NMOS decimal mode.
//!
//! Reference: http://www.6502.org/tutorials/6502opcodes.html
//!
//! Not modelled: the dummy reads/writes of read-modify-write instructions, and
//! the unstable undocumented opcodes (they are skipped with the right length).

/// Processor status flags
const FLAG_C: u8 = 0x01; // Carry
const FLAG_Z: u8 = 0x02; // Zero
const FLAG_I: u8 = 0x04; // Interrupt disable
const FLAG_D: u8 = 0x08; // Decimal mode
const FLAG_B: u8 = 0x10; // Break (not a real flag, only pushed)
const FLAG_U: u8 = 0x20; // Unused (always 1)
const FLAG_V: u8 = 0x40; // Overflow
const FLAG_N: u8 = 0x80; // Negative

/// Addressing modes
#[derive(Clone, Copy)]
#[allow(dead_code)]
enum AddrMode {
    Imp,    // Implied / Accumulator
    Imm,    // Immediate #$nn
    Zp,     // Zero Page $nn
    ZpX,    // Zero Page,X $nn,X
    ZpY,    // Zero Page,Y $nn,Y
    Abs,    // Absolute $nnnn
    AbsX,   // Absolute,X $nnnn,X
    AbsY,   // Absolute,Y $nnnn,Y
    Ind,    // Indirect ($nnnn) — JMP only
    IndX,   // (Indirect,X) ($nn,X)
    IndY,   // (Indirect),Y ($nn),Y
    Rel,    // Relative (branches)
}

pub struct Cpu {
    pub a: u8,      // Accumulator
    pub x: u8,      // Index X
    pub y: u8,      // Index Y
    pub sp: u8,     // Stack pointer
    pub pc: u16,    // Program counter
    pub p: u8,      // Processor status
    pub cycles: u64, // Total cycles executed
    /// Set by a JAM opcode (or by the shell); `step` then does nothing.
    pub halted: bool,
    /// Last executed opcode (shown when halted).
    pub last_opcode: u8,
    /// Address of the instruction that halted the CPU.
    pub halt_pc: u16,
    /// An indexed read crossed a page during this instruction (+1 cycle).
    crossed: bool,
    extra_cycles: u32,
}

impl Default for Cpu {
    fn default() -> Self { Self::new() }
}

impl Cpu {
    pub fn new() -> Self {
        Cpu {
            a: 0,
            x: 0,
            y: 0,
            sp: 0xFD,
            pc: 0,
            p: FLAG_U | FLAG_I,
            cycles: 0,
            halted: false,
            last_opcode: 0,
            halt_pc: 0,
            crossed: false,
            extra_cycles: 0,
        }
    }

    pub fn reset(&mut self, mem: &dyn MemAccess) {
        self.a = 0;
        self.x = 0;
        self.y = 0;
        self.sp = 0xFD;
        self.p = FLAG_U | FLAG_I;
        // Read reset vector
        let lo = mem.read(0xFFFC) as u16;
        let hi = mem.read(0xFFFD) as u16;
        self.pc = (hi << 8) | lo;
        self.cycles = 0;
        self.halted = false;
    }

    /// Execute one instruction, return cycles consumed
    pub fn step(&mut self, mem: &mut dyn MemAccess) -> u32 {
        if self.halted {
            return 1;
        }
        self.crossed = false;
        self.extra_cycles = 0;
        let start = self.pc;
        let opcode = self.fetch(mem);
        self.last_opcode = opcode;
        let c = self.execute(opcode, mem);
        if self.halted {
            self.halt_pc = start;
            self.pc = start; // stay on the JAM
        }
        c + self.extra_cycles
    }

    fn fetch(&mut self, mem: &dyn MemAccess) -> u8 {
        let val = mem.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        val
    }

    fn fetch16(&mut self, mem: &dyn MemAccess) -> u16 {
        let lo = self.fetch(mem) as u16;
        let hi = self.fetch(mem) as u16;
        (hi << 8) | lo
    }

    fn push(&mut self, mem: &mut dyn MemAccess, val: u8) {
        mem.write(0x0100 | self.sp as u16, val);
        self.sp = self.sp.wrapping_sub(1);
    }

    fn push16(&mut self, mem: &mut dyn MemAccess, val: u16) {
        self.push(mem, (val >> 8) as u8);
        self.push(mem, val as u8);
    }

    fn pull(&mut self, mem: &dyn MemAccess) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        mem.read(0x0100 | self.sp as u16)
    }

    fn pull16(&mut self, mem: &dyn MemAccess) -> u16 {
        let lo = self.pull(mem) as u16;
        let hi = self.pull(mem) as u16;
        (hi << 8) | lo
    }

    fn set_flag(&mut self, flag: u8, cond: bool) {
        if cond {
            self.p |= flag;
        } else {
            self.p &= !flag;
        }
    }

    fn get_flag(&self, flag: u8) -> bool {
        self.p & flag != 0
    }

    fn update_nz(&mut self, val: u8) {
        self.set_flag(FLAG_Z, val == 0);
        self.set_flag(FLAG_N, val & 0x80 != 0);
    }

    /// Resolve effective address for the given addressing mode
    fn resolve_addr(&mut self, mode: AddrMode, mem: &dyn MemAccess) -> u16 {
        match mode {
            AddrMode::Imm => {
                let addr = self.pc;
                self.pc = self.pc.wrapping_add(1);
                addr
            }
            AddrMode::Zp => self.fetch(mem) as u16,
            AddrMode::ZpX => (self.fetch(mem).wrapping_add(self.x)) as u16,
            AddrMode::ZpY => (self.fetch(mem).wrapping_add(self.y)) as u16,
            AddrMode::Abs => self.fetch16(mem),
            AddrMode::AbsX => {
                let base = self.fetch16(mem);
                let addr = base.wrapping_add(self.x as u16);
                self.crossed = (base ^ addr) & 0xFF00 != 0;
                addr
            }
            AddrMode::AbsY => {
                let base = self.fetch16(mem);
                let addr = base.wrapping_add(self.y as u16);
                self.crossed = (base ^ addr) & 0xFF00 != 0;
                addr
            }
            AddrMode::Ind => {
                let ptr = self.fetch16(mem);
                // 6502 bug: wraps within page for indirect JMP
                let lo = mem.read(ptr) as u16;
                let hi_addr = (ptr & 0xFF00) | (ptr.wrapping_add(1) & 0x00FF);
                let hi = mem.read(hi_addr) as u16;
                (hi << 8) | lo
            }
            AddrMode::IndX => {
                let base = self.fetch(mem);
                let ptr = base.wrapping_add(self.x);
                let lo = mem.read(ptr as u16) as u16;
                let hi = mem.read(ptr.wrapping_add(1) as u16) as u16;
                (hi << 8) | lo
            }
            AddrMode::IndY => {
                let ptr = self.fetch(mem);
                let lo = mem.read(ptr as u16) as u16;
                let hi = mem.read(ptr.wrapping_add(1) as u16) as u16;
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.y as u16);
                self.crossed = (base ^ addr) & 0xFF00 != 0;
                addr
            }
            AddrMode::Rel => {
                let offset = self.fetch(mem) as i8;
                self.pc.wrapping_add(offset as u16)
            }
            AddrMode::Imp => 0, // Not used for address resolution
        }
    }

    /// Read-type access: indexed modes cost one extra cycle on a page crossing.
    fn read_operand(&mut self, mode: AddrMode, mem: &dyn MemAccess) -> u8 {
        let addr = self.resolve_addr(mode, mem);
        if self.crossed {
            self.extra_cycles += 1;
        }
        mem.read(addr)
    }

    fn branch(&mut self, mem: &dyn MemAccess, cond: bool) -> u32 {
        let offset = self.fetch(mem) as i8;
        if cond {
            let old_pc = self.pc;
            self.pc = self.pc.wrapping_add(offset as u16);
            // Extra cycle for branch taken, +1 more if page crossed
            if (old_pc & 0xFF00) != (self.pc & 0xFF00) { 4 } else { 3 }
        } else {
            2
        }
    }

    fn adc(&mut self, val: u8) {
        let carry = self.get_flag(FLAG_C) as u16;
        let a = self.a as u16;
        let v = val as u16;
        let bin = a + v + carry;

        if self.get_flag(FLAG_D) {
            // NMOS decimal mode (6502.org "Decimal Mode" appendix A): Z comes
            // from the binary sum, N and V from the intermediate result.
            let mut lo = (a & 0x0F) + (v & 0x0F) + carry;
            if lo >= 0x0A {
                lo = ((lo + 0x06) & 0x0F) + 0x10;
            }
            let mut tmp = (a & 0xF0) + (v & 0xF0) + lo;
            self.set_flag(FLAG_Z, bin & 0xFF == 0);
            self.set_flag(FLAG_N, tmp & 0x80 != 0);
            self.set_flag(FLAG_V, (!(a ^ v) & (a ^ tmp) & 0x80) != 0);
            if tmp >= 0xA0 {
                tmp += 0x60;
            }
            self.set_flag(FLAG_C, tmp > 0xFF);
            self.a = tmp as u8;
        } else {
            self.set_flag(FLAG_C, bin > 0xFF);
            self.set_flag(FLAG_V, (!(a ^ v) & (a ^ bin) & 0x80) != 0);
            self.a = bin as u8;
            self.update_nz(self.a);
        }
    }

    fn sbc(&mut self, val: u8) {
        let borrow = !self.get_flag(FLAG_C) as i16;
        let a = self.a as i16;
        let v = val as i16;
        let bin = a - v - borrow;
        // All SBC flags come from the binary result, also in decimal mode (NMOS).
        self.set_flag(FLAG_C, bin >= 0);
        self.set_flag(FLAG_V, ((a ^ v) & (a ^ bin) & 0x80) != 0);
        self.update_nz(bin as u8);

        if self.get_flag(FLAG_D) {
            let mut lo = (a & 0x0F) - (v & 0x0F) - borrow;
            if lo < 0 {
                lo = ((lo - 0x06) & 0x0F) - 0x10;
            }
            let mut tmp = (a & 0xF0) - (v & 0xF0) + lo;
            if tmp < 0 {
                tmp -= 0x60;
            }
            self.a = tmp as u8;
        } else {
            self.a = bin as u8;
        }
    }

    fn compare(&mut self, reg: u8, val: u8) {
        let result = reg.wrapping_sub(val);
        self.set_flag(FLAG_C, reg >= val);
        self.update_nz(result);
    }

    fn execute(&mut self, opcode: u8, mem: &mut dyn MemAccess) -> u32 {
        match opcode {
            // BRK
            0x00 => {
                self.pc = self.pc.wrapping_add(1); // Skip padding byte
                self.push16(mem, self.pc);
                self.push(mem, self.p | FLAG_B | FLAG_U);
                self.p |= FLAG_I;
                let lo = mem.read(0xFFFE) as u16;
                let hi = mem.read(0xFFFF) as u16;
                self.pc = (hi << 8) | lo;
                7
            }

            // ORA
            0x01 => { let v = self.read_operand(AddrMode::IndX, mem); self.a |= v; self.update_nz(self.a); 6 }
            0x05 => { let v = self.read_operand(AddrMode::Zp, mem); self.a |= v; self.update_nz(self.a); 3 }
            0x09 => { let v = self.read_operand(AddrMode::Imm, mem); self.a |= v; self.update_nz(self.a); 2 }
            0x0D => { let v = self.read_operand(AddrMode::Abs, mem); self.a |= v; self.update_nz(self.a); 4 }
            0x11 => { let v = self.read_operand(AddrMode::IndY, mem); self.a |= v; self.update_nz(self.a); 5 }
            0x15 => { let v = self.read_operand(AddrMode::ZpX, mem); self.a |= v; self.update_nz(self.a); 4 }
            0x19 => { let v = self.read_operand(AddrMode::AbsY, mem); self.a |= v; self.update_nz(self.a); 4 }
            0x1D => { let v = self.read_operand(AddrMode::AbsX, mem); self.a |= v; self.update_nz(self.a); 4 }

            // AND
            0x21 => { let v = self.read_operand(AddrMode::IndX, mem); self.a &= v; self.update_nz(self.a); 6 }
            0x25 => { let v = self.read_operand(AddrMode::Zp, mem); self.a &= v; self.update_nz(self.a); 3 }
            0x29 => { let v = self.read_operand(AddrMode::Imm, mem); self.a &= v; self.update_nz(self.a); 2 }
            0x2D => { let v = self.read_operand(AddrMode::Abs, mem); self.a &= v; self.update_nz(self.a); 4 }
            0x31 => { let v = self.read_operand(AddrMode::IndY, mem); self.a &= v; self.update_nz(self.a); 5 }
            0x35 => { let v = self.read_operand(AddrMode::ZpX, mem); self.a &= v; self.update_nz(self.a); 4 }
            0x39 => { let v = self.read_operand(AddrMode::AbsY, mem); self.a &= v; self.update_nz(self.a); 4 }
            0x3D => { let v = self.read_operand(AddrMode::AbsX, mem); self.a &= v; self.update_nz(self.a); 4 }

            // EOR
            0x41 => { let v = self.read_operand(AddrMode::IndX, mem); self.a ^= v; self.update_nz(self.a); 6 }
            0x45 => { let v = self.read_operand(AddrMode::Zp, mem); self.a ^= v; self.update_nz(self.a); 3 }
            0x49 => { let v = self.read_operand(AddrMode::Imm, mem); self.a ^= v; self.update_nz(self.a); 2 }
            0x4D => { let v = self.read_operand(AddrMode::Abs, mem); self.a ^= v; self.update_nz(self.a); 4 }
            0x51 => { let v = self.read_operand(AddrMode::IndY, mem); self.a ^= v; self.update_nz(self.a); 5 }
            0x55 => { let v = self.read_operand(AddrMode::ZpX, mem); self.a ^= v; self.update_nz(self.a); 4 }
            0x59 => { let v = self.read_operand(AddrMode::AbsY, mem); self.a ^= v; self.update_nz(self.a); 4 }
            0x5D => { let v = self.read_operand(AddrMode::AbsX, mem); self.a ^= v; self.update_nz(self.a); 4 }

            // ADC
            0x61 => { let v = self.read_operand(AddrMode::IndX, mem); self.adc(v); 6 }
            0x65 => { let v = self.read_operand(AddrMode::Zp, mem); self.adc(v); 3 }
            0x69 => { let v = self.read_operand(AddrMode::Imm, mem); self.adc(v); 2 }
            0x6D => { let v = self.read_operand(AddrMode::Abs, mem); self.adc(v); 4 }
            0x71 => { let v = self.read_operand(AddrMode::IndY, mem); self.adc(v); 5 }
            0x75 => { let v = self.read_operand(AddrMode::ZpX, mem); self.adc(v); 4 }
            0x79 => { let v = self.read_operand(AddrMode::AbsY, mem); self.adc(v); 4 }
            0x7D => { let v = self.read_operand(AddrMode::AbsX, mem); self.adc(v); 4 }

            // SBC
            0xE1 => { let v = self.read_operand(AddrMode::IndX, mem); self.sbc(v); 6 }
            0xE5 => { let v = self.read_operand(AddrMode::Zp, mem); self.sbc(v); 3 }
            0xE9 => { let v = self.read_operand(AddrMode::Imm, mem); self.sbc(v); 2 }
            0xED => { let v = self.read_operand(AddrMode::Abs, mem); self.sbc(v); 4 }
            0xF1 => { let v = self.read_operand(AddrMode::IndY, mem); self.sbc(v); 5 }
            0xF5 => { let v = self.read_operand(AddrMode::ZpX, mem); self.sbc(v); 4 }
            0xF9 => { let v = self.read_operand(AddrMode::AbsY, mem); self.sbc(v); 4 }
            0xFD => { let v = self.read_operand(AddrMode::AbsX, mem); self.sbc(v); 4 }

            // CMP
            0xC1 => { let v = self.read_operand(AddrMode::IndX, mem); self.compare(self.a, v); 6 }
            0xC5 => { let v = self.read_operand(AddrMode::Zp, mem); self.compare(self.a, v); 3 }
            0xC9 => { let v = self.read_operand(AddrMode::Imm, mem); self.compare(self.a, v); 2 }
            0xCD => { let v = self.read_operand(AddrMode::Abs, mem); self.compare(self.a, v); 4 }
            0xD1 => { let v = self.read_operand(AddrMode::IndY, mem); self.compare(self.a, v); 5 }
            0xD5 => { let v = self.read_operand(AddrMode::ZpX, mem); self.compare(self.a, v); 4 }
            0xD9 => { let v = self.read_operand(AddrMode::AbsY, mem); self.compare(self.a, v); 4 }
            0xDD => { let v = self.read_operand(AddrMode::AbsX, mem); self.compare(self.a, v); 4 }

            // CPX
            0xE0 => { let v = self.read_operand(AddrMode::Imm, mem); self.compare(self.x, v); 2 }
            0xE4 => { let v = self.read_operand(AddrMode::Zp, mem); self.compare(self.x, v); 3 }
            0xEC => { let v = self.read_operand(AddrMode::Abs, mem); self.compare(self.x, v); 4 }

            // CPY
            0xC0 => { let v = self.read_operand(AddrMode::Imm, mem); self.compare(self.y, v); 2 }
            0xC4 => { let v = self.read_operand(AddrMode::Zp, mem); self.compare(self.y, v); 3 }
            0xCC => { let v = self.read_operand(AddrMode::Abs, mem); self.compare(self.y, v); 4 }

            // LDA
            0xA1 => { self.a = self.read_operand(AddrMode::IndX, mem); self.update_nz(self.a); 6 }
            0xA5 => { self.a = self.read_operand(AddrMode::Zp, mem); self.update_nz(self.a); 3 }
            0xA9 => { self.a = self.read_operand(AddrMode::Imm, mem); self.update_nz(self.a); 2 }
            0xAD => { self.a = self.read_operand(AddrMode::Abs, mem); self.update_nz(self.a); 4 }
            0xB1 => { self.a = self.read_operand(AddrMode::IndY, mem); self.update_nz(self.a); 5 }
            0xB5 => { self.a = self.read_operand(AddrMode::ZpX, mem); self.update_nz(self.a); 4 }
            0xB9 => { self.a = self.read_operand(AddrMode::AbsY, mem); self.update_nz(self.a); 4 }
            0xBD => { self.a = self.read_operand(AddrMode::AbsX, mem); self.update_nz(self.a); 4 }

            // LDX
            0xA2 => { self.x = self.read_operand(AddrMode::Imm, mem); self.update_nz(self.x); 2 }
            0xA6 => { self.x = self.read_operand(AddrMode::Zp, mem); self.update_nz(self.x); 3 }
            0xAE => { self.x = self.read_operand(AddrMode::Abs, mem); self.update_nz(self.x); 4 }
            0xB6 => { self.x = self.read_operand(AddrMode::ZpY, mem); self.update_nz(self.x); 4 }
            0xBE => { self.x = self.read_operand(AddrMode::AbsY, mem); self.update_nz(self.x); 4 }

            // LDY
            0xA0 => { self.y = self.read_operand(AddrMode::Imm, mem); self.update_nz(self.y); 2 }
            0xA4 => { self.y = self.read_operand(AddrMode::Zp, mem); self.update_nz(self.y); 3 }
            0xAC => { self.y = self.read_operand(AddrMode::Abs, mem); self.update_nz(self.y); 4 }
            0xB4 => { self.y = self.read_operand(AddrMode::ZpX, mem); self.update_nz(self.y); 4 }
            0xBC => { self.y = self.read_operand(AddrMode::AbsX, mem); self.update_nz(self.y); 4 }

            // STA
            0x81 => { let a = self.resolve_addr(AddrMode::IndX, mem); mem.write(a, self.a); 6 }
            0x85 => { let a = self.resolve_addr(AddrMode::Zp, mem); mem.write(a, self.a); 3 }
            0x8D => { let a = self.resolve_addr(AddrMode::Abs, mem); mem.write(a, self.a); 4 }
            0x91 => { let a = self.resolve_addr(AddrMode::IndY, mem); mem.write(a, self.a); 6 }
            0x95 => { let a = self.resolve_addr(AddrMode::ZpX, mem); mem.write(a, self.a); 4 }
            0x99 => { let a = self.resolve_addr(AddrMode::AbsY, mem); mem.write(a, self.a); 5 }
            0x9D => { let a = self.resolve_addr(AddrMode::AbsX, mem); mem.write(a, self.a); 5 }

            // STX
            0x86 => { let a = self.resolve_addr(AddrMode::Zp, mem); mem.write(a, self.x); 3 }
            0x8E => { let a = self.resolve_addr(AddrMode::Abs, mem); mem.write(a, self.x); 4 }
            0x96 => { let a = self.resolve_addr(AddrMode::ZpY, mem); mem.write(a, self.x); 4 }

            // STY
            0x84 => { let a = self.resolve_addr(AddrMode::Zp, mem); mem.write(a, self.y); 3 }
            0x8C => { let a = self.resolve_addr(AddrMode::Abs, mem); mem.write(a, self.y); 4 }
            0x94 => { let a = self.resolve_addr(AddrMode::ZpX, mem); mem.write(a, self.y); 4 }

            // Transfer
            0xAA => { self.x = self.a; self.update_nz(self.x); 2 } // TAX
            0xA8 => { self.y = self.a; self.update_nz(self.y); 2 } // TAY
            0x8A => { self.a = self.x; self.update_nz(self.a); 2 } // TXA
            0x98 => { self.a = self.y; self.update_nz(self.a); 2 } // TYA
            0xBA => { self.x = self.sp; self.update_nz(self.x); 2 } // TSX
            0x9A => { self.sp = self.x; 2 }                         // TXS

            // Stack
            0x48 => { self.push(mem, self.a); 3 }                    // PHA
            0x08 => { self.push(mem, self.p | FLAG_B | FLAG_U); 3 } // PHP
            0x68 => { self.a = self.pull(mem); self.update_nz(self.a); 4 } // PLA
            0x28 => { self.p = (self.pull(mem) & !FLAG_B) | FLAG_U; 4 }    // PLP

            // ASL
            0x0A => { // ASL A
                self.set_flag(FLAG_C, self.a & 0x80 != 0);
                self.a <<= 1;
                self.update_nz(self.a);
                2
            }
            0x06 => { self.asl_mem(AddrMode::Zp, mem); 5 }
            0x0E => { self.asl_mem(AddrMode::Abs, mem); 6 }
            0x16 => { self.asl_mem(AddrMode::ZpX, mem); 6 }
            0x1E => { self.asl_mem(AddrMode::AbsX, mem); 7 }

            // LSR
            0x4A => { // LSR A
                self.set_flag(FLAG_C, self.a & 0x01 != 0);
                self.a >>= 1;
                self.update_nz(self.a);
                2
            }
            0x46 => { self.lsr_mem(AddrMode::Zp, mem); 5 }
            0x4E => { self.lsr_mem(AddrMode::Abs, mem); 6 }
            0x56 => { self.lsr_mem(AddrMode::ZpX, mem); 6 }
            0x5E => { self.lsr_mem(AddrMode::AbsX, mem); 7 }

            // ROL
            0x2A => { // ROL A
                let carry = if self.get_flag(FLAG_C) { 1 } else { 0 };
                self.set_flag(FLAG_C, self.a & 0x80 != 0);
                self.a = (self.a << 1) | carry;
                self.update_nz(self.a);
                2
            }
            0x26 => { self.rol_mem(AddrMode::Zp, mem); 5 }
            0x2E => { self.rol_mem(AddrMode::Abs, mem); 6 }
            0x36 => { self.rol_mem(AddrMode::ZpX, mem); 6 }
            0x3E => { self.rol_mem(AddrMode::AbsX, mem); 7 }

            // ROR
            0x6A => { // ROR A
                let carry = if self.get_flag(FLAG_C) { 0x80 } else { 0 };
                self.set_flag(FLAG_C, self.a & 0x01 != 0);
                self.a = (self.a >> 1) | carry;
                self.update_nz(self.a);
                2
            }
            0x66 => { self.ror_mem(AddrMode::Zp, mem); 5 }
            0x6E => { self.ror_mem(AddrMode::Abs, mem); 6 }
            0x76 => { self.ror_mem(AddrMode::ZpX, mem); 6 }
            0x7E => { self.ror_mem(AddrMode::AbsX, mem); 7 }

            // INC
            0xE6 => { self.inc_mem(AddrMode::Zp, mem); 5 }
            0xEE => { self.inc_mem(AddrMode::Abs, mem); 6 }
            0xF6 => { self.inc_mem(AddrMode::ZpX, mem); 6 }
            0xFE => { self.inc_mem(AddrMode::AbsX, mem); 7 }

            // DEC
            0xC6 => { self.dec_mem(AddrMode::Zp, mem); 5 }
            0xCE => { self.dec_mem(AddrMode::Abs, mem); 6 }
            0xD6 => { self.dec_mem(AddrMode::ZpX, mem); 6 }
            0xDE => { self.dec_mem(AddrMode::AbsX, mem); 7 }

            // INX, INY, DEX, DEY
            0xE8 => { self.x = self.x.wrapping_add(1); self.update_nz(self.x); 2 } // INX
            0xC8 => { self.y = self.y.wrapping_add(1); self.update_nz(self.y); 2 } // INY
            0xCA => { self.x = self.x.wrapping_sub(1); self.update_nz(self.x); 2 } // DEX
            0x88 => { self.y = self.y.wrapping_sub(1); self.update_nz(self.y); 2 } // DEY

            // BIT
            0x24 => { let v = self.read_operand(AddrMode::Zp, mem); self.bit_test(v); 3 }
            0x2C => { let v = self.read_operand(AddrMode::Abs, mem); self.bit_test(v); 4 }

            // Branches
            0x10 => self.branch(mem, !self.get_flag(FLAG_N)), // BPL
            0x30 => self.branch(mem, self.get_flag(FLAG_N)),  // BMI
            0x50 => self.branch(mem, !self.get_flag(FLAG_V)), // BVC
            0x70 => self.branch(mem, self.get_flag(FLAG_V)),  // BVS
            0x90 => self.branch(mem, !self.get_flag(FLAG_C)), // BCC
            0xB0 => self.branch(mem, self.get_flag(FLAG_C)),  // BCS
            0xD0 => self.branch(mem, !self.get_flag(FLAG_Z)), // BNE
            0xF0 => self.branch(mem, self.get_flag(FLAG_Z)),  // BEQ

            // JMP
            0x4C => { self.pc = self.resolve_addr(AddrMode::Abs, mem); 3 }
            0x6C => { self.pc = self.resolve_addr(AddrMode::Ind, mem); 5 }

            // JSR
            0x20 => {
                let target = self.fetch16(mem);
                self.push16(mem, self.pc.wrapping_sub(1));
                self.pc = target;
                6
            }

            // RTS
            0x60 => {
                self.pc = self.pull16(mem).wrapping_add(1);
                6
            }

            // RTI
            0x40 => {
                self.p = (self.pull(mem) & !FLAG_B) | FLAG_U;
                self.pc = self.pull16(mem);
                6
            }

            // Flag instructions
            0x18 => { self.p &= !FLAG_C; 2 } // CLC
            0x38 => { self.p |= FLAG_C; 2 }  // SEC
            0x58 => { self.p &= !FLAG_I; 2 } // CLI
            0x78 => { self.p |= FLAG_I; 2 }  // SEI
            0xB8 => { self.p &= !FLAG_V; 2 } // CLV
            0xD8 => { self.p &= !FLAG_D; 2 } // CLD
            0xF8 => { self.p |= FLAG_D; 2 }  // SED

            // NOP
            0xEA => 2,

            // Undocumented opcodes
            _ => self.undocumented(opcode, mem),
        }
    }

    /// Undocumented opcodes. The stable ones are implemented; the unstable
    /// ones are skipped with the correct operand length and no effect.
    fn undocumented(&mut self, opcode: u8, mem: &mut dyn MemAccess) -> u32 {
        match opcode {
            // JAM / KIL: the CPU locks up.
            0x02 | 0x12 | 0x22 | 0x32 | 0x42 | 0x52 | 0x62 | 0x72 | 0x92 | 0xB2 | 0xD2 | 0xF2 => {
                self.halted = true;
                2
            }
            // NOPs (implied)
            0x1A | 0x3A | 0x5A | 0x7A | 0xDA | 0xFA => 2,
            // NOP #imm
            0x80 | 0x82 | 0x89 | 0xC2 | 0xE2 => {
                self.pc = self.pc.wrapping_add(1);
                2
            }
            // NOP zp / zp,X
            0x04 | 0x44 | 0x64 => {
                self.pc = self.pc.wrapping_add(1);
                3
            }
            0x14 | 0x34 | 0x54 | 0x74 | 0xD4 | 0xF4 => {
                self.pc = self.pc.wrapping_add(1);
                4
            }
            // NOP abs / abs,X
            0x0C => {
                self.pc = self.pc.wrapping_add(2);
                4
            }
            0x1C | 0x3C | 0x5C | 0x7C | 0xDC | 0xFC => {
                self.read_operand(AddrMode::AbsX, mem);
                4
            }
            // Immediate-mode oddities
            0x0B | 0x2B => {
                // ANC
                let v = self.read_operand(AddrMode::Imm, mem);
                self.a &= v;
                self.update_nz(self.a);
                self.set_flag(FLAG_C, self.a & 0x80 != 0);
                2
            }
            0x4B => {
                // ALR: AND then LSR A
                let v = self.read_operand(AddrMode::Imm, mem);
                self.a &= v;
                self.set_flag(FLAG_C, self.a & 1 != 0);
                self.a >>= 1;
                self.update_nz(self.a);
                2
            }
            0x6B => {
                // ARR (binary mode behaviour)
                let v = self.read_operand(AddrMode::Imm, mem);
                self.a &= v;
                self.a = (self.a >> 1) | if self.get_flag(FLAG_C) { 0x80 } else { 0 };
                self.update_nz(self.a);
                self.set_flag(FLAG_C, self.a & 0x40 != 0);
                self.set_flag(FLAG_V, ((self.a >> 6) ^ (self.a >> 5)) & 1 != 0);
                2
            }
            0xCB => {
                // SBX: X = (A & X) - imm, flags like CMP
                let v = self.read_operand(AddrMode::Imm, mem);
                let ax = self.a & self.x;
                self.set_flag(FLAG_C, ax >= v);
                self.x = ax.wrapping_sub(v);
                self.update_nz(self.x);
                2
            }
            0xEB => {
                let v = self.read_operand(AddrMode::Imm, mem);
                self.sbc(v);
                2
            }
            // Unstable: skip operand bytes only.
            0x8B | 0xAB | 0x93 => {
                self.pc = self.pc.wrapping_add(1);
                if opcode == 0x93 { 6 } else { 2 }
            }
            0x9B | 0x9F | 0xBB | 0x9C | 0x9E => {
                self.pc = self.pc.wrapping_add(2);
                5
            }
            // The regular combination group (xxxxxx11).
            _ if opcode & 0x03 == 0x03 => {
                let (mode, base_cycles) = match (opcode >> 2) & 7 {
                    0 => (AddrMode::IndX, 8),
                    1 => (AddrMode::Zp, 5),
                    3 => (AddrMode::Abs, 6),
                    4 => (AddrMode::IndY, 8),
                    5 => (if matches!(opcode, 0x97 | 0xB7) { AddrMode::ZpY } else { AddrMode::ZpX }, 6),
                    6 => (AddrMode::AbsY, 7),
                    _ => (if matches!(opcode, 0x9F | 0xBF) { AddrMode::AbsY } else { AddrMode::AbsX }, 7),
                };
                let addr = self.resolve_addr(mode, mem);
                match opcode >> 5 {
                    0 => {
                        // SLO
                        let v = mem.read(addr);
                        self.set_flag(FLAG_C, v & 0x80 != 0);
                        let r = v << 1;
                        mem.write(addr, r);
                        self.a |= r;
                        self.update_nz(self.a);
                        base_cycles
                    }
                    1 => {
                        // RLA
                        let v = mem.read(addr);
                        let c = self.get_flag(FLAG_C) as u8;
                        self.set_flag(FLAG_C, v & 0x80 != 0);
                        let r = (v << 1) | c;
                        mem.write(addr, r);
                        self.a &= r;
                        self.update_nz(self.a);
                        base_cycles
                    }
                    2 => {
                        // SRE
                        let v = mem.read(addr);
                        self.set_flag(FLAG_C, v & 1 != 0);
                        let r = v >> 1;
                        mem.write(addr, r);
                        self.a ^= r;
                        self.update_nz(self.a);
                        base_cycles
                    }
                    3 => {
                        // RRA
                        let v = mem.read(addr);
                        let c = if self.get_flag(FLAG_C) { 0x80 } else { 0 };
                        self.set_flag(FLAG_C, v & 1 != 0);
                        let r = (v >> 1) | c;
                        mem.write(addr, r);
                        self.adc(r);
                        base_cycles
                    }
                    4 => {
                        // SAX
                        mem.write(addr, self.a & self.x);
                        base_cycles - 2
                    }
                    5 => {
                        // LAX
                        let v = mem.read(addr);
                        self.a = v;
                        self.x = v;
                        self.update_nz(v);
                        base_cycles - 2
                    }
                    6 => {
                        // DCP
                        let r = mem.read(addr).wrapping_sub(1);
                        mem.write(addr, r);
                        self.compare(self.a, r);
                        base_cycles
                    }
                    _ => {
                        // ISC
                        let r = mem.read(addr).wrapping_add(1);
                        mem.write(addr, r);
                        self.sbc(r);
                        base_cycles
                    }
                }
            }
            // Anything left is a 1-byte NOP.
            _ => 2,
        }
    }

    fn bit_test(&mut self, val: u8) {
        self.set_flag(FLAG_Z, self.a & val == 0);
        self.set_flag(FLAG_N, val & 0x80 != 0);
        self.set_flag(FLAG_V, val & 0x40 != 0);
    }

    fn asl_mem(&mut self, mode: AddrMode, mem: &mut dyn MemAccess) {
        let addr = self.resolve_addr(mode, mem);
        let mut val = mem.read(addr);
        self.set_flag(FLAG_C, val & 0x80 != 0);
        val <<= 1;
        mem.write(addr, val);
        self.update_nz(val);
    }

    fn lsr_mem(&mut self, mode: AddrMode, mem: &mut dyn MemAccess) {
        let addr = self.resolve_addr(mode, mem);
        let mut val = mem.read(addr);
        self.set_flag(FLAG_C, val & 0x01 != 0);
        val >>= 1;
        mem.write(addr, val);
        self.update_nz(val);
    }

    fn rol_mem(&mut self, mode: AddrMode, mem: &mut dyn MemAccess) {
        let addr = self.resolve_addr(mode, mem);
        let mut val = mem.read(addr);
        let carry = if self.get_flag(FLAG_C) { 1 } else { 0 };
        self.set_flag(FLAG_C, val & 0x80 != 0);
        val = (val << 1) | carry;
        mem.write(addr, val);
        self.update_nz(val);
    }

    fn ror_mem(&mut self, mode: AddrMode, mem: &mut dyn MemAccess) {
        let addr = self.resolve_addr(mode, mem);
        let mut val = mem.read(addr);
        let carry = if self.get_flag(FLAG_C) { 0x80 } else { 0 };
        self.set_flag(FLAG_C, val & 0x01 != 0);
        val = (val >> 1) | carry;
        mem.write(addr, val);
        self.update_nz(val);
    }

    fn inc_mem(&mut self, mode: AddrMode, mem: &mut dyn MemAccess) {
        let addr = self.resolve_addr(mode, mem);
        let val = mem.read(addr).wrapping_add(1);
        mem.write(addr, val);
        self.update_nz(val);
    }

    fn dec_mem(&mut self, mode: AddrMode, mem: &mut dyn MemAccess) {
        let addr = self.resolve_addr(mode, mem);
        let val = mem.read(addr).wrapping_sub(1);
        mem.write(addr, val);
        self.update_nz(val);
    }

    /// Trigger a non-maskable interrupt
    #[allow(dead_code)]
    pub fn nmi(&mut self, mem: &mut dyn MemAccess) {
        self.push16(mem, self.pc);
        self.push(mem, (self.p | FLAG_U) & !FLAG_B);
        self.p |= FLAG_I;
        let lo = mem.read(0xFFFA) as u16;
        let hi = mem.read(0xFFFB) as u16;
        self.pc = (hi << 8) | lo;
    }

    /// Trigger a maskable interrupt (if I flag clear)
    pub fn irq(&mut self, mem: &mut dyn MemAccess) {
        if !self.get_flag(FLAG_I) {
            self.push16(mem, self.pc);
            self.push(mem, (self.p | FLAG_U) & !FLAG_B);
            self.p |= FLAG_I;
            let lo = mem.read(0xFFFE) as u16;
            let hi = mem.read(0xFFFF) as u16;
            self.pc = (hi << 8) | lo;
        }
    }
}

/// Memory access trait — implemented by the C64 system bus
pub trait MemAccess {
    fn read(&self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, val: u8);
}
