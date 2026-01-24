# Precursor C64 Emulator

A Commodore 64 emulator for the [Precursor](https://www.crowdsupply.com/sutajio-kosagi/precursor) hardware platform, running on the [Xous](https://github.com/betrusted-io/xous-core) microkernel OS.

## Features

- **Full 6502 CPU** — All 151 legal opcodes with proper flag handling, BCD arithmetic, and interrupt support
- **VIC-II Video** — Text mode (40x25 characters) and standard bitmap mode (320x200 hi-res)
- **CIA1 Keyboard** — 8x8 matrix scanning with Precursor key mapping
- **Bank Switching** — BASIC ROM, KERNAL ROM, Character ROM, I/O area via $0001
- **Monochrome Rendering** — C64 colors converted to 1-bit via luminance threshold
- **Game Storage** — .PRG files stored in encrypted PDDB
- **TCP Import** — Load games via `cat game.prg | nc <precursor-ip> 6464`
- **Minimal ROM** — Built-in character font and boot stub when no C64 ROMs available

## Architecture

```
src/
├── main.rs       # Xous app shell, GAM setup, emulation loop, rendering
├── cpu.rs        # MOS 6502 CPU — all legal opcodes, addressing modes
├── memory.rs     # 64KB RAM + ROM banking + I/O dispatch
├── vic.rs        # VIC-II: text mode, bitmap mode, color RAM, framebuffer
├── cia.rs        # CIA1 keyboard matrix + timers, CIA2 VIC bank select
├── keyboard.rs   # Precursor rawkeys → C64 matrix position mapping
└── loader.rs     # .PRG loading, PDDB storage, TCP import
```

## Controls

### Menu Mode

| Key | Action |
|-----|--------|
| Up/Down | Navigate game list |
| Enter | Select/launch |

### Emulator Mode

| Precursor Key | C64 Key |
|---------------|---------|
| A-Z | A-Z |
| 0-9 | 0-9 |
| Space | Space |
| Enter | Return |
| Backspace | INST/DEL |
| Arrows | Cursor keys |
| Esc | RUN/STOP |
| Menu (Home) | Back to game menu |

## Loading Games

### Via TCP (Port 6464)

From the game menu, select "Import Game (TCP :6464)", then from your computer:

```bash
# Format: filename\n followed by .PRG binary data
(echo "othello"; cat othello.prg) | nc <precursor-ip> 6464
```

### Using Original ROMs

For full C64 compatibility (BASIC interpreter, proper KERNAL), store ROM files in the PDDB:

- Dictionary: `c64.roms`
- Keys: `basic` (8KB), `kernal` (8KB), `chargen` (4KB)

Without original ROMs, the emulator uses a built-in minimal font and boot stub.

## Technical Details

### Performance

- **100 MHz RISC-V** emulating **1 MHz 6502** = ~100x clock headroom
- After overhead: ~30x effective speed margin for turn-based games
- ~50,000 CPU cycles executed per display frame (~20 fps)

### Display Mapping

- C64 display: 320x200 pixels, 16 colors
- Precursor display: 336x536 pixels, 1-bit (black/white)
- C64 output centered on Precursor screen with border
- Colors mapped to monochrome via luminance threshold

### Memory Map

| Address Range | Read | Write |
|--------------|------|-------|
| $0000-$00FF | Zero Page RAM | Zero Page RAM |
| $0100-$01FF | Stack RAM | Stack RAM |
| $0400-$07FF | Screen RAM | Screen RAM |
| $0800-$9FFF | RAM | RAM |
| $A000-$BFFF | BASIC ROM / RAM | RAM |
| $C000-$CFFF | RAM | RAM |
| $D000-$D3FF | VIC-II registers | VIC-II registers |
| $D400-$D7FF | SID (stub) | SID (ignored) |
| $D800-$DBFF | Color RAM | Color RAM |
| $DC00-$DCFF | CIA1 (keyboard) | CIA1 |
| $DD00-$DDFF | CIA2 (VIC bank) | CIA2 |
| $E000-$FFFF | KERNAL ROM / RAM | RAM |

### What's Not Implemented (v1)

- SID audio (returns 0, ignores writes)
- Sprites
- Raster interrupts (CIA timer IRQ works)
- Illegal/undocumented opcodes (treated as NOP)
- Smooth scrolling
- Multicolor bitmap mode
- Disk drive emulation (.D64)

## Building

### For Renode (emulator testing)

```bash
cd xous-core
cargo xtask renode-image c64
```

### For Hardware

```bash
cd xous-core
cargo xtask app-image c64
```

## Target Games

The emulator is designed for monochrome-compatible C64 games:

| Category | Examples |
|----------|----------|
| Board Games | Othello, Chess, Archon |
| Text Adventures | Zork, Hitchhiker's Guide |
| Roguelikes | Rogue, Nethack, Moria |
| Puzzle | Boulder Dash, Sokoban, Tetris |
| Strategy | Ultima I-V, Pirates!, M.U.L.E. |
| Wireframe | Elite, Mercenary, Star Wars |

## Dependencies

- `gam` — Graphics Abstraction Manager (display, keyboard)
- `pddb` — Encrypted storage for game files and ROMs
- No external crates beyond Xous core

## License

MIT
