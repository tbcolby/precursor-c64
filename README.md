# Precursor C64

A Commodore 64 emulator for [Precursor](https://precursor.dev), built on `precursor-appkit`. It is a hobby-grade emulator for text-mode software and BASIC, drawn in 1 bit on Precursor's monochrome display. It is not cycle-accurate and runs only a fraction of the C64 library.

No Commodore ROMs are included. Without ROMs the app boots a tiny original stand-in (banner, no BASIC, no keyboard handling). For BASIC and the real KERNAL you supply your own ROM images (see Importing).

## Features

- 6502 CPU: all 151 legal opcodes, NMOS decimal mode, stable undocumented opcodes, page-cross cycle penalties, level-triggered IRQ. Jam opcodes halt the CPU and the UI says so.
- Memory bus with the standard banking (BASIC / KERNAL / I/O / character ROM), 64 KB RAM on the heap.
- CIA1: keyboard matrix, timers A/B, jiffy IRQ, read-to-clear interrupt register. CIA2: bank select only.
- VIC-II: text mode and standard hi-res bitmap mode, both character sets, raster counter derived from the cycle count (so `LDA $D012` frame-sync loops terminate). Colours are mapped to 1-bit with a contrast guard so text stays readable.
- Frame pump: 50 ms ticks, stopped when the app is in the background or no machine is running.
- PRG launch: boots the machine, copies the program in, then either types `RUN` (BASIC programs, real ROMs) or jumps to the `SYS` target / load address (machine code).
- Stored games and ROMs live in the PDDB and survive reboots.

## Keys

Menu: Up/Down select, Enter choose, Backspace leave the app.

While running: letters, digits and punctuation go to the C64 keyboard; Enter = RETURN, Backspace = INST/DEL, arrows = cursor keys, F1/F2/F3 = C64 F1/F3/F5, backtick = RUN/STOP. **F4 stops the machine and returns to the menu** (Backspace is DEL here, so F4 is the exit). The menu key (center of the D-pad) is consumed by Xous and never reaches the app.

Import screen: Backspace or F4 cancels.

## Importing games and ROMs

Choose "Import game or ROM" on the menu. The device listens once on TCP port 6464 (unauthenticated, only while that screen is open; each read times out after 5 s and the whole transfer after 30 s; 64 KB cap):

```bash
printf 'mygame.prg\n' | cat - mygame.prg | nc <device-ip> 6464
printf 'rom:basic\n'  | cat - basic.rom   | nc <device-ip> 6464   # also rom:kernal, rom:chargen
```

Names are 1-32 characters from `A-Za-z0-9 ._-`. ROM images must be exactly 8192 bytes (BASIC, KERNAL) or 4096 (character ROM). BASIC and KERNAL are used only as a pair.

## Storage

PDDB dictionary `c64.games` (one key per PRG, written by delete-and-recreate so a shorter re-import leaves no stale tail) and `c64.roms` (`basic`, `kernal`, `chargen`). Nothing else is stored; machine state is not saved.

## Limitations

- No sprites, multicolour, extended-colour text, scroll registers, border effects or raster interrupts. Most commercial games will not work.
- No SID (no sound), no disk drive or tape, no joystick, no TOD clock, no CIA2 timers or NMI.
- Colour is reduced to two levels; the display is redrawn per changed character cell.
- Timing is approximate (about 50,000 cycles per 50 ms frame). Speed on the device is untested and may be slower than a real C64.
- Without ROMs there is no BASIC; plain BASIC programs are refused in that mode, machine-code PRGs run.
- Renode capture not yet run. `capture.py` is written but not executed; verification so far is the hosted build, clippy, lint and host tests. The earlier screenshots showed Shellchat rather than the app and were removed.

## Testing

`cd core && cargo test` runs about 90 tests: per-instruction and flag tests for the 6502 (including decimal mode, stack, JMP indirect bug, BRK/RTI), CIA, VIC, keyboard, PRG parsing and import validation, and boot/launch on the stub ROM. The Klaus Dormann functional test is included but ignored by default; run it with `KLAUS_6502_BIN=/path/6502_functional_test.bin cargo test -- --ignored`.

## Building

Builds inside a [xous-core](https://github.com/betrusted-io/xous-core) tree. With [xous-dev-toolkit](https://github.com/tbcolby/xous-dev-toolkit):

```bash
bin/integrate --xous-root ~/xous-core --grade ~/Dev/precursor/precursor-c64
```

## Layout

```
core/src/cpu.rs, cia.rs, vic.rs, memory.rs   chips and bus
core/src/keyboard.rs, prg.rs, machine.rs     key matrix, PRG/import format, frame loop and launch
core/src/rom_stub.rs                         original stand-in font and mini KERNAL
src/main.rs, app.rs, import.rs               GAM shell, screens, TCP import thread
capture.py                                   Renode capture sequence (not yet run)
```

## Author

Tyler Colby, [Colby's Data Movers, LLC](https://colbysdatamovers.com). Licensed under the Apache License, Version 2.0 (see [LICENSE](LICENSE)).
