# C64

A Commodore 64 emulator for the [Precursor](https://www.crowdsupply.com/sutajio-kosagi/precursor) device — play monochrome-compatible C64 games on a 1-bit display.

## Features

- **Full 6502 CPU** — All 151 legal opcodes with proper flag handling, BCD arithmetic, and interrupt support
- **VIC-II Video** — Text mode (40x25 characters) and standard bitmap mode (320x200 hi-res)
- **CIA1 Keyboard** — 8x8 matrix scanning with Precursor key mapping
- **Bank Switching** — BASIC ROM, KERNAL ROM, Character ROM, I/O area via $0001
- **Monochrome Rendering** — C64 colors converted to 1-bit via luminance threshold
- **Game Storage** — .PRG files stored in encrypted PDDB
- **TCP Import** — Load games via `cat game.prg | nc <precursor-ip> 6464`
- **Minimal ROM** — Built-in character font and boot stub when no C64 ROMs available

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

## Installation

Clone into your [xous-core](https://github.com/betrusted-io/xous-core) apps directory:

```bash
cd xous-core/apps
git clone https://github.com/tbcolby/precursor-c64.git c64
```

Register the app in your workspace. Add to `xous-core/Cargo.toml` members:

```toml
"apps/c64",
```

Register in `apps/manifest.json`:

```json
"c64": {
    "context_name": "C64",
    "menu_name": {
        "appmenu.c64": {
            "en": "C64 Emulator",
            "en-tts": "C 64 Emulator"
        }
    }
}
```

Build and run:

```bash
cargo xtask renode-image c64
```

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

### Key Design Decisions

1. **Luminance threshold** — C64's 16 colors mapped to black/white by brightness. Games designed around high-contrast visuals (board games, wireframe, text) render cleanly.

2. **Minimal built-in ROM** — No dependency on copyrighted C64 ROMs. Boots with a generated character set and stub KERNAL. Original ROMs optional for full BASIC compatibility.

3. **50K cycles per frame** — Balances emulation accuracy with Precursor's 100 MHz RISC-V. Sufficient for turn-based and puzzle games at ~20 fps.

4. **PDDB storage** — Games stored encrypted alongside other Precursor data. TCP import avoids needing USB mass storage or SD card access.

5. **No SID audio (v1)** — Precursor has a piezo speaker only. Focus on playable visuals first.

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

### PDDB Storage

| Dictionary | Key Pattern | Format |
|-----------|-------------|--------|
| c64.games | `{game_name}` | Raw .PRG binary (2-byte load addr + program) |
| c64.roms | `basic` | 8KB BASIC ROM |
| c64.roms | `kernal` | 8KB KERNAL ROM |
| c64.roms | `chargen` | 4KB Character ROM |

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

## Testing

```bash
# Build for Renode emulation
cargo xtask renode-image c64

# Build for Precursor hardware
cargo xtask app-image c64
```

## Development

This app was developed using the methodology described in [xous-dev-toolkit](https://github.com/tbcolby/xous-dev-toolkit) — an LLM-assisted approach to Precursor app development on macOS ARM64.

## Author

Made by Tyler Colby — [Colby's Data Movers, LLC](https://colbysdatamovers.com)

Contact: [tyler@colbysdatamovers.com](mailto:tyler@colbysdatamovers.com) | [GitHub Issues](https://github.com/tbcolby/precursor-c64/issues)

## License

Licensed under the Apache License, Version 2.0.
