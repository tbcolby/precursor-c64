//! 6502 instruction tests against known results. Flat 64 KB RAM, no devices.

use c64_core::cpu::{Cpu, MemAccess};

const C: u8 = 0x01;
const Z: u8 = 0x02;
const I: u8 = 0x04;
const D: u8 = 0x08;
const B: u8 = 0x10;
const V: u8 = 0x40;
const N: u8 = 0x80;

struct Ram(Vec<u8>);

impl MemAccess for Ram {
    fn read(&self, a: u16) -> u8 { self.0[a as usize] }
    fn write(&mut self, a: u16, v: u8) { self.0[a as usize] = v; }
}

/// Load `code` at $0200 and return a CPU positioned there.
fn setup(code: &[u8]) -> (Cpu, Ram) {
    let mut ram = Ram(vec![0; 65536]);
    ram.0[0x0200..0x0200 + code.len()].copy_from_slice(code);
    ram.0[0xFFFC] = 0x00;
    ram.0[0xFFFD] = 0x02;
    ram.0[0xFFFE] = 0x00;
    ram.0[0xFFFF] = 0x30;
    let mut cpu = Cpu::new();
    cpu.reset(&ram);
    (cpu, ram)
}

fn run(code: &[u8], steps: usize) -> (Cpu, Ram) {
    let (mut cpu, mut ram) = setup(code);
    for _ in 0..steps {
        cpu.step(&mut ram);
    }
    (cpu, ram)
}

#[test]
fn reset_state() {
    let (cpu, _) = setup(&[]);
    assert_eq!((cpu.pc, cpu.sp), (0x0200, 0xFD));
    assert_eq!(cpu.p & I, I);
}

#[test]
fn lda_sets_zero_and_negative() {
    let (cpu, _) = run(&[0xA9, 0x00], 1);
    assert_eq!((cpu.a, cpu.p & (Z | N)), (0, Z));
    let (cpu, _) = run(&[0xA9, 0x80], 1);
    assert_eq!((cpu.a, cpu.p & (Z | N)), (0x80, N));
}

#[test]
fn adc_binary_flags() {
    // 0x50 + 0x50 = 0xA0: signed overflow, negative, no carry
    let (cpu, _) = run(&[0x18, 0xA9, 0x50, 0x69, 0x50], 3);
    assert_eq!(cpu.a, 0xA0);
    assert_eq!(cpu.p & (C | V | N | Z), V | N);
    // 0xFF + 0x01 = 0x00: carry, zero
    let (cpu, _) = run(&[0x18, 0xA9, 0xFF, 0x69, 0x01], 3);
    assert_eq!((cpu.a, cpu.p & (C | V | N | Z)), (0, C | Z));
    // carry in
    let (cpu, _) = run(&[0x38, 0xA9, 0x01, 0x69, 0x01], 3);
    assert_eq!(cpu.a, 3);
}

#[test]
fn sbc_binary_flags() {
    // 0x50 - 0xB0 = 0xA0, overflow, borrow (carry clear)
    let (cpu, _) = run(&[0x38, 0xA9, 0x50, 0xE9, 0xB0], 3);
    assert_eq!(cpu.a, 0xA0);
    assert_eq!(cpu.p & (C | V | N), V | N);
    // 5 - 3 = 2, carry set
    let (cpu, _) = run(&[0x38, 0xA9, 0x05, 0xE9, 0x03], 3);
    assert_eq!((cpu.a, cpu.p & C), (2, C));
    // borrow in: 5 - 3 - 1 = 1
    let (cpu, _) = run(&[0x18, 0xA9, 0x05, 0xE9, 0x03], 3);
    assert_eq!(cpu.a, 1);
}

#[test]
fn cmp_cpx_cpy() {
    let (cpu, _) = run(&[0xA9, 0x10, 0xC9, 0x10], 2);
    assert_eq!(cpu.p & (C | Z | N), C | Z);
    let (cpu, _) = run(&[0xA9, 0x10, 0xC9, 0x20], 2);
    assert_eq!(cpu.p & (C | Z | N), N);
    let (cpu, _) = run(&[0xA2, 0x05, 0xE0, 0x03], 2);
    assert_eq!(cpu.p & (C | Z), C);
    let (cpu, _) = run(&[0xA0, 0x05, 0xC0, 0x06], 2);
    assert_eq!(cpu.p & (C | Z | N), N);
}

#[test]
fn logic_and_bit() {
    let (cpu, _) = run(&[0xA9, 0xF0, 0x29, 0x3C], 2);
    assert_eq!(cpu.a, 0x30);
    let (cpu, _) = run(&[0xA9, 0x0F, 0x09, 0x80], 2);
    assert_eq!((cpu.a, cpu.p & N), (0x8F, N));
    let (cpu, _) = run(&[0xA9, 0xFF, 0x49, 0xFF], 2);
    assert_eq!((cpu.a, cpu.p & Z), (0, Z));
    // BIT copies bits 7 and 6 of memory, Z from A & M
    let (mut cpu, mut ram) = setup(&[0xA9, 0x01, 0x24, 0x10]);
    ram.0[0x10] = 0xC0;
    cpu.step(&mut ram);
    cpu.step(&mut ram);
    assert_eq!(cpu.p & (N | V | Z), N | V | Z);
}

#[test]
fn shifts_and_rotates() {
    let (cpu, _) = run(&[0xA9, 0x81, 0x0A], 2); // ASL A
    assert_eq!((cpu.a, cpu.p & C), (0x02, C));
    let (cpu, _) = run(&[0xA9, 0x01, 0x4A], 2); // LSR A
    assert_eq!((cpu.a, cpu.p & (C | Z)), (0, C | Z));
    let (cpu, _) = run(&[0x38, 0xA9, 0x00, 0x2A], 3); // ROL A with carry in
    assert_eq!((cpu.a, cpu.p & C), (0x01, 0));
    let (cpu, _) = run(&[0x38, 0xA9, 0x00, 0x6A], 3); // ROR A with carry in
    assert_eq!((cpu.a, cpu.p & (C | N)), (0x80, N));
    let (mut cpu, mut ram) = setup(&[0x06, 0x20]); // ASL $20
    ram.0[0x20] = 0x40;
    cpu.step(&mut ram);
    assert_eq!((ram.0[0x20], cpu.p & (N | C)), (0x80, N));
}

#[test]
fn inc_dec_wrap() {
    let (mut cpu, mut ram) = setup(&[0xE6, 0x30, 0xC6, 0x31]);
    ram.0[0x30] = 0xFF;
    ram.0[0x31] = 0x00;
    cpu.step(&mut ram);
    cpu.step(&mut ram);
    assert_eq!((ram.0[0x30], ram.0[0x31]), (0x00, 0xFF));
    let (cpu, _) = run(&[0xA2, 0xFF, 0xE8], 2);
    assert_eq!((cpu.x, cpu.p & Z), (0, Z));
    let (cpu, _) = run(&[0xA0, 0x00, 0x88], 2);
    assert_eq!((cpu.y, cpu.p & N), (0xFF, N));
}

#[test]
fn transfers() {
    let (cpu, _) = run(&[0xA9, 0x80, 0xAA, 0xA8], 3);
    assert_eq!((cpu.x, cpu.y, cpu.p & N), (0x80, 0x80, N));
    let (cpu, _) = run(&[0xA2, 0x40, 0x9A], 2); // TXS does not touch flags
    assert_eq!(cpu.sp, 0x40);
    let (cpu, _) = run(&[0xBA], 1); // TSX
    assert_eq!(cpu.x, 0xFD);
}

#[test]
fn stack_push_pull_and_php_plp() {
    let (cpu, ram) = run(&[0xA9, 0x42, 0x48, 0xA9, 0x00, 0x68], 4);
    assert_eq!(cpu.a, 0x42);
    assert_eq!(ram.0[0x01FD], 0x42);
    // PHP pushes with B and the unused bit set
    let (cpu, ram) = run(&[0x38, 0x08], 2);
    assert_eq!(ram.0[0x01FD] & (B | 0x20 | C), B | 0x20 | C);
    assert_eq!(cpu.sp, 0xFC);
    // PLP ignores B, keeps the unused bit set
    let (cpu, _) = run(&[0xA9, 0xFF, 0x48, 0x28], 3);
    assert_eq!(cpu.p & B, 0);
    assert_eq!(cpu.p & 0x20, 0x20);
    assert_eq!(cpu.p & (C | Z | I | D | V | N), C | Z | I | D | V | N);
}

#[test]
fn jsr_rts_and_jmp() {
    // JSR $0210; LDX #1; JMP $0220 ... subroutine: LDA #$77; RTS
    let mut code = vec![0xEA; 0x30];
    code[0..3].copy_from_slice(&[0x20, 0x10, 0x02]);
    code[3..5].copy_from_slice(&[0xA2, 0x01]);
    code[0x10..0x13].copy_from_slice(&[0xA9, 0x77, 0x60]);
    let (mut cpu, mut ram) = setup(&code);
    cpu.step(&mut ram);
    assert_eq!(cpu.pc, 0x0210);
    assert_eq!((ram.0[0x01FD], ram.0[0x01FC]), (0x02, 0x02), "pushes return-1 = $0202");
    cpu.step(&mut ram);
    cpu.step(&mut ram);
    assert_eq!((cpu.pc, cpu.a), (0x0203, 0x77));
}

#[test]
fn jmp_indirect_page_wrap_bug() {
    let (mut cpu, mut ram) = setup(&[0x6C, 0xFF, 0x10]);
    ram.0[0x10FF] = 0x34;
    ram.0[0x1000] = 0x12; // high byte comes from the start of the same page
    ram.0[0x1100] = 0xEE;
    cpu.step(&mut ram);
    assert_eq!(cpu.pc, 0x1234);
}

#[test]
fn branches_and_cycle_counts() {
    // BNE taken, same page: 3 cycles; not taken: 2
    let (mut cpu, mut ram) = setup(&[0xD0, 0x02, 0xEA, 0xEA, 0xEA]);
    assert_eq!(cpu.step(&mut ram), 3);
    assert_eq!(cpu.pc, 0x0204);
    let (mut cpu, mut ram) = setup(&[0xA9, 0x00, 0xD0, 0x02]);
    cpu.step(&mut ram);
    assert_eq!(cpu.step(&mut ram), 2);
    // Taken across a page: 4 cycles. Place the branch at $02FD, target $0300+.
    let mut ram = Ram(vec![0; 65536]);
    ram.0[0x02FD] = 0xD0; // BNE +5 -> $0304
    ram.0[0x02FE] = 0x05;
    let mut cpu = Cpu::new();
    cpu.pc = 0x02FD;
    assert_eq!(cpu.step(&mut ram), 4);
    assert_eq!(cpu.pc, 0x0304);
    // Backward branch
    let mut ram = Ram(vec![0; 65536]);
    ram.0[0x0210] = 0xF0; // BEQ -4
    ram.0[0x0211] = 0xFC;
    let mut cpu = Cpu::new();
    cpu.pc = 0x0210;
    cpu.p |= Z;
    cpu.step(&mut ram);
    assert_eq!(cpu.pc, 0x020E);
}

#[test]
fn indexed_read_page_cross_costs_one_cycle() {
    // LDA $02FF,X with X=1 crosses into $0300
    let (mut cpu, mut ram) = setup(&[0xA2, 0x01, 0xBD, 0xFF, 0x02, 0xBD, 0x10, 0x02]);
    cpu.step(&mut ram);
    assert_eq!(cpu.step(&mut ram), 5);
    assert_eq!(cpu.step(&mut ram), 4, "no crossing");
    // Stores always take 5 for abs,X
    let (mut cpu, mut ram) = setup(&[0xA2, 0x01, 0x9D, 0x10, 0x02]);
    cpu.step(&mut ram);
    assert_eq!(cpu.step(&mut ram), 5);
}

#[test]
fn indirect_addressing() {
    // LDA ($40,X) with X=2 -> pointer at $42; LDA ($50),Y
    let (mut cpu, mut ram) = setup(&[0xA2, 0x02, 0xA1, 0x40, 0xA0, 0x03, 0xB1, 0x50]);
    ram.0[0x42] = 0x00;
    ram.0[0x43] = 0x30;
    ram.0[0x3000] = 0x99;
    ram.0[0x50] = 0x00;
    ram.0[0x51] = 0x31;
    ram.0[0x3103] = 0x55;
    cpu.step(&mut ram);
    cpu.step(&mut ram);
    assert_eq!(cpu.a, 0x99);
    cpu.step(&mut ram);
    cpu.step(&mut ram);
    assert_eq!(cpu.a, 0x55);
}

#[test]
fn zero_page_indexing_wraps() {
    let (mut cpu, mut ram) = setup(&[0xA2, 0xFF, 0xB5, 0x02]);
    ram.0[0x01] = 0x5A; // $02 + $FF wraps to $01
    cpu.step(&mut ram);
    cpu.step(&mut ram);
    assert_eq!(cpu.a, 0x5A);
}

#[test]
fn brk_pushes_state_and_vectors() {
    let (mut cpu, mut ram) = setup(&[0x00, 0xEA]);
    cpu.p &= !I;
    cpu.step(&mut ram);
    assert_eq!(cpu.pc, 0x3000);
    assert_eq!(cpu.p & I, I);
    // Return address is BRK+2, status has B set
    assert_eq!((ram.0[0x01FD], ram.0[0x01FC]), (0x02, 0x02));
    assert_eq!(ram.0[0x01FB] & B, B);
    // RTI restores
    ram.0[0x3000] = 0x40;
    cpu.step(&mut ram);
    assert_eq!(cpu.pc, 0x0202);
    assert_eq!(cpu.p & I, 0);
}

#[test]
fn irq_respects_i_flag_and_pushes_b_clear() {
    let (mut cpu, mut ram) = setup(&[0xEA]);
    cpu.irq(&mut ram);
    assert_eq!(cpu.pc, 0x0200, "I set after reset: ignored");
    cpu.p &= !I;
    cpu.irq(&mut ram);
    assert_eq!(cpu.pc, 0x3000);
    assert_eq!(ram.0[0x01FB] & B, 0);
}

#[test]
fn stack_pointer_wraps() {
    let (mut cpu, mut ram) = setup(&[0xA9, 0x11, 0x48]);
    cpu.sp = 0x00;
    cpu.step(&mut ram);
    cpu.step(&mut ram);
    assert_eq!(ram.0[0x0100], 0x11);
    assert_eq!(cpu.sp, 0xFF);
}

#[test]
fn flag_instructions() {
    let (cpu, _) = run(&[0x38, 0xF8, 0x78], 3);
    assert_eq!(cpu.p & (C | D | I), C | D | I);
    let (cpu, _) = run(&[0x38, 0xF8, 0xD8, 0x18, 0x58], 5);
    assert_eq!(cpu.p & (C | D | I), 0);
    let (mut cpu, mut ram) = setup(&[0xB8]);
    cpu.p |= V;
    cpu.step(&mut ram);
    assert_eq!(cpu.p & V, 0);
}

// ---- decimal mode -----------------------------------------------------------

fn bcd(n: u8) -> u8 { ((n / 10) << 4) | (n % 10) }

fn adc_d(a: u8, b: u8, carry: bool) -> Cpu {
    let (mut cpu, mut ram) = setup(&[if carry { 0x38 } else { 0x18 }, 0xF8, 0xA9, a, 0x69, b]);
    for _ in 0..4 {
        cpu.step(&mut ram);
    }
    cpu
}

fn sbc_d(a: u8, b: u8, carry: bool) -> Cpu {
    let (mut cpu, mut ram) = setup(&[if carry { 0x38 } else { 0x18 }, 0xF8, 0xA9, a, 0xE9, b]);
    for _ in 0..4 {
        cpu.step(&mut ram);
    }
    cpu
}

#[test]
fn bcd_adc_known_values() {
    let c = adc_d(0x09, 0x01, false);
    assert_eq!((c.a, c.p & C), (0x10, 0));
    let c = adc_d(0x99, 0x01, false);
    assert_eq!((c.a, c.p & C), (0x00, C));
    let c = adc_d(0x58, 0x46, true);
    assert_eq!((c.a, c.p & C), (0x05, C), "58 + 46 + 1 = 105");
    let c = adc_d(0x12, 0x34, false);
    assert_eq!((c.a, c.p & (C | N | Z)), (0x46, 0));
}

#[test]
fn bcd_adc_matches_decimal_for_all_valid_inputs() {
    for a in 0..100u8 {
        for b in 0..100u8 {
            for carry in [false, true] {
                let c = adc_d(bcd(a), bcd(b), carry);
                let total = a as u32 + b as u32 + carry as u32;
                assert_eq!(c.a, bcd((total % 100) as u8), "{a}+{b}+{carry}");
                assert_eq!(c.p & C != 0, total >= 100, "carry {a}+{b}+{carry}");
            }
        }
    }
}

#[test]
fn bcd_sbc_matches_decimal_for_all_valid_inputs() {
    for a in 0..100u8 {
        for b in 0..100u8 {
            for carry in [false, true] {
                let c = sbc_d(bcd(a), bcd(b), carry);
                let diff = a as i32 - b as i32 - (!carry) as i32;
                let expect = bcd(diff.rem_euclid(100) as u8);
                assert_eq!(c.a, expect, "{a}-{b}-{}", !carry);
                assert_eq!(c.p & C != 0, diff >= 0, "borrow {a}-{b}-{}", !carry);
            }
        }
    }
}

#[test]
fn bcd_flags_follow_nmos() {
    // ADC: Z comes from the binary sum. 0x99 + 0x01 = 0x9A binary (non-zero),
    // so Z is clear although the decimal result is 00.
    let c = adc_d(0x99, 0x01, false);
    assert_eq!(c.p & Z, 0);
    // 0x80 + 0x80 in decimal: binary sum 0x100 -> Z set; N/V from the intermediate 0x00 -> 0xA0?
    let c = adc_d(0x80, 0x80, false);
    assert_eq!(c.p & Z, Z, "binary 0x100 & 0xFF == 0");
    assert_eq!(c.a, 0x60);
    assert_eq!(c.p & C, C);
    assert_eq!(c.p & V, V);
    // SBC flags are the binary ones: 0x00 - 0x01 borrows, result 0x99 decimal, N from binary 0xFF
    let c = sbc_d(0x00, 0x01, true);
    assert_eq!(c.a, 0x99);
    assert_eq!((c.p & C, c.p & N), (0, N));
}

// ---- undocumented opcodes -----------------------------------------------------

#[test]
fn lax_loads_a_and_x() {
    let (mut cpu, mut ram) = setup(&[0xA7, 0x10]); // LAX $10
    ram.0[0x10] = 0x8C;
    assert_eq!(cpu.step(&mut ram), 3);
    assert_eq!((cpu.a, cpu.x, cpu.p & N), (0x8C, 0x8C, N));
    assert_eq!(cpu.pc, 0x0202, "operand skipped");
}

#[test]
fn sax_stores_a_and_x() {
    let (mut cpu, mut ram) = setup(&[0xA9, 0xF0, 0xA2, 0x3C, 0x87, 0x20]);
    for _ in 0..3 {
        cpu.step(&mut ram);
    }
    assert_eq!(ram.0[0x20], 0x30);
}

#[test]
fn dcp_decrements_then_compares() {
    let (mut cpu, mut ram) = setup(&[0xA9, 0x05, 0xC7, 0x10]);
    ram.0[0x10] = 0x06;
    cpu.step(&mut ram);
    cpu.step(&mut ram);
    assert_eq!(ram.0[0x10], 0x05);
    assert_eq!(cpu.p & (Z | C), Z | C);
}

#[test]
fn isc_increments_then_subtracts() {
    let (mut cpu, mut ram) = setup(&[0x38, 0xA9, 0x10, 0xE7, 0x10]);
    ram.0[0x10] = 0x04;
    for _ in 0..3 {
        cpu.step(&mut ram);
    }
    assert_eq!(ram.0[0x10], 0x05);
    assert_eq!(cpu.a, 0x0B);
}

#[test]
fn slo_rla_sre_rra() {
    // SLO: ASL mem, ORA
    let (mut cpu, mut ram) = setup(&[0xA9, 0x01, 0x07, 0x10]);
    ram.0[0x10] = 0x81;
    cpu.step(&mut ram);
    cpu.step(&mut ram);
    assert_eq!((ram.0[0x10], cpu.a, cpu.p & C), (0x02, 0x03, C));
    // RLA: ROL mem, AND
    let (mut cpu, mut ram) = setup(&[0x38, 0xA9, 0xFF, 0x27, 0x10]);
    ram.0[0x10] = 0x40;
    for _ in 0..3 {
        cpu.step(&mut ram);
    }
    assert_eq!((ram.0[0x10], cpu.a), (0x81, 0x81));
    // SRE: LSR mem, EOR
    let (mut cpu, mut ram) = setup(&[0xA9, 0xFF, 0x47, 0x10]);
    ram.0[0x10] = 0x03;
    cpu.step(&mut ram);
    cpu.step(&mut ram);
    assert_eq!((ram.0[0x10], cpu.a, cpu.p & C), (0x01, 0xFE, C));
    // RRA: ROR mem, ADC
    let (mut cpu, mut ram) = setup(&[0x18, 0xA9, 0x10, 0x67, 0x10]);
    ram.0[0x10] = 0x02;
    for _ in 0..3 {
        cpu.step(&mut ram);
    }
    assert_eq!((ram.0[0x10], cpu.a), (0x01, 0x11));
}

#[test]
fn immediate_undocumented_ops() {
    let (cpu, _) = run(&[0xA9, 0xFF, 0x0B, 0x80], 2); // ANC
    assert_eq!((cpu.a, cpu.p & (C | N)), (0x80, C | N));
    let (cpu, _) = run(&[0xA9, 0xFF, 0x4B, 0x03], 2); // ALR: (A & 3) >> 1
    assert_eq!((cpu.a, cpu.p & C), (0x01, C));
    let (cpu, _) = run(&[0xA9, 0x0F, 0xA2, 0x0C, 0xCB, 0x04], 3); // SBX: (A&X) - 4
    assert_eq!((cpu.x, cpu.p & C), (0x08, C));
    let (cpu, _) = run(&[0x38, 0xA9, 0x05, 0xEB, 0x02], 3); // SBC #2 alias
    assert_eq!(cpu.a, 0x03);
}

// Regression for C-13: operand bytes of undocumented opcodes are skipped.
#[test]
fn undocumented_nops_have_the_right_length() {
    for (code, len) in [
        (vec![0x1A], 1u16),
        (vec![0x80, 0x00], 2),
        (vec![0x04, 0x00], 2),
        (vec![0x14, 0x00], 2),
        (vec![0x0C, 0x00, 0x00], 3),
        (vec![0x1C, 0x00, 0x00], 3),
        (vec![0x03, 0x40], 2), // SLO (zp,X)
        (vec![0x0F, 0x00, 0x30], 3), // SLO abs
        (vec![0x8B, 0x00], 2), // unstable XAA
        (vec![0x9F, 0x00, 0x30], 3), // unstable SHA
    ] {
        let (mut cpu, mut ram) = setup(&code);
        cpu.step(&mut ram);
        assert_eq!(cpu.pc, 0x0200 + len, "opcode {:02X}", code[0]);
        assert!(!cpu.halted);
    }
}

#[test]
fn every_opcode_advances_pc_by_a_sane_length_or_halts() {
    // No opcode may panic, and every non-JAM one consumes 1..=3 bytes (unless it jumps).
    for op in 0..=255u8 {
        let (mut cpu, mut ram) = setup(&[op, 0x10, 0x20]);
        ram.0[0x10] = 1;
        ram.0[0x2010] = 1;
        cpu.x = 0;
        cpu.y = 0;
        let cycles = cpu.step(&mut ram);
        assert!((1..=8).contains(&cycles), "opcode {op:02X} cycles {cycles}");
        let jumps = matches!(op, 0x00 | 0x20 | 0x40 | 0x4C | 0x60 | 0x6C | 0x10 | 0x30 | 0x50 | 0x70 | 0x90 | 0xB0 | 0xD0 | 0xF0);
        if !cpu.halted && !jumps {
            let advanced = cpu.pc.wrapping_sub(0x0200);
            assert!((1..=3).contains(&advanced), "opcode {op:02X} advanced {advanced}");
        }
    }
}

#[test]
fn jam_stops_the_cpu_on_the_jam_opcode() {
    for op in [0x02u8, 0x12, 0x22, 0x32, 0x42, 0x52, 0x62, 0x72, 0x92, 0xB2, 0xD2, 0xF2] {
        let (mut cpu, mut ram) = setup(&[op, 0xEA]);
        cpu.step(&mut ram);
        assert!(cpu.halted, "{op:02X}");
        assert_eq!((cpu.halt_pc, cpu.pc, cpu.last_opcode), (0x0200, 0x0200, op));
        let before = cpu.cycles;
        cpu.step(&mut ram);
        assert_eq!(cpu.pc, 0x0200, "stays put");
        assert_eq!(cpu.cycles, before);
    }
}

#[test]
fn addresses_wrap_instead_of_panicking() {
    // JMP ($FFFF) and a load whose indexed address wraps $FFFF -> $0000
    let (mut cpu, mut ram) = setup(&[0x6C, 0xFF, 0xFF]);
    cpu.step(&mut ram);
    let (mut cpu, mut ram) = setup(&[0xA2, 0x02, 0xBD, 0xFF, 0xFF]);
    ram.0[0x0001] = 0x77;
    cpu.step(&mut ram);
    cpu.step(&mut ram);
    assert_eq!(cpu.a, 0x77);
}

// ---- optional: Klaus Dormann's functional test ------------------------------------
//
// The test binary is GPL and is not shipped here. To run it:
//   KLAUS_6502_BIN=/path/to/6502_functional_test.bin cargo test --release -- --ignored klaus
// (the 64 KB image loads at $0000, start at $0400, success is a trap at $3469).

#[test]
#[ignore]
fn klaus_functional_test() {
    let Ok(path) = std::env::var("KLAUS_6502_BIN") else {
        eprintln!("KLAUS_6502_BIN not set; skipping");
        return;
    };
    let image = std::fs::read(path).expect("read test binary");
    assert_eq!(image.len(), 65536);
    let mut ram = Ram(image);
    let mut cpu = Cpu::new();
    cpu.pc = 0x0400;
    let mut last = 0xFFFF;
    for _ in 0..100_000_000u64 {
        let pc = cpu.pc;
        cpu.step(&mut ram);
        if cpu.pc == pc {
            last = pc;
            break;
        }
    }
    assert_eq!(last, 0x3469, "trapped at ${last:04X}");
}
