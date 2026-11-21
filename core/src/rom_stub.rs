//! Built-in, original replacements used when the user has not provided
//! Commodore ROMs: an 8x8 font (a subset of the PETSCII screen codes) and a
//! tiny "KERNAL" that clears the screen, prints a banner and `READY.`, then
//! spins. It has no keyboard scan, no cursor and no BASIC. No Commodore code
//! or data is included.

/// Entry point the stub's reset vector points to.
pub const STUB_ENTRY: u16 = 0xFCE2;

/// An 8 KB image for $E000-$FFFF.
/// Also returns the address of its final `JMP *` loop (initialisation is done once the PC is there).
pub fn stub_kernal() -> (Box<[u8]>, u16) {
    let mut kernal = vec![0u8; 8192];

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
        let screen_pos: u16 = 0x0400 + 40 + 9; // Row 1, centered
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
    (kernal.into_boxed_slice(), loop_addr)
}

/// Convert ASCII byte to PETSCII screen code
pub fn ascii_to_petscii_screen(ch: u8) -> u8 {
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
pub fn builtin_font() -> Box<[u8]> {
    let mut font = vec![0u8; 4096];

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

    font.into_boxed_slice()
}
