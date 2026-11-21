//! C64 emulation core: no Xous dependencies, host-tested.
//!
//! * [`cpu`]: 6502 with the stable undocumented opcodes and NMOS decimal mode
//! * [`cia`], [`vic`], [`memory`]: the chips and the bus (heap allocated)
//! * [`keyboard`]: key matrix mapping and a press/release queue
//! * [`machine`]: frame loop, boot, PRG launch
//! * [`prg`]: .PRG parsing and the TCP import format
//! * [`rom_stub`]: original stand-in font and tiny KERNAL (no Commodore ROMs inside)

pub mod cia;
pub mod cpu;
pub mod keyboard;
pub mod machine;
pub mod memory;
pub mod prg;
pub mod rom_stub;
pub mod vic;

pub use machine::{Machine, RomSet};
