#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

//! C64 emulator shell. The 6502, CIA, VIC-II, bus and PRG logic live in
//! `core/` (c64-core, host-tested); this crate registers with GAM, pumps
//! frames, draws, and talks to the PDDB and network.

mod app;
mod import;

use num_traits::{FromPrimitive, ToPrimitive};
use precursor_appkit::{app as kit, keys, pump, store};

/// xous-names server name. Any unique string; NOT the GAM app name.
const SERVER_NAME: &str = "_C64 Emulator_";

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum AppOp {
    Redraw = 0,
    Rawkeys,
    FocusChange,
    /// Frame pump tick (blocking scalar).
    Frame,
    /// The import thread has a result.
    ImportDone,
    BasisChanged,
    Quit,
}

/// Milliseconds between frames. One frame runs 50,000 emulated cycles (50 ms of C64 time).
const FRAME_MS: usize = 50;

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("c64 PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let sid = xns.register_name(SERVER_NAME, None).expect("can't register server");

    // GAM app name comes from apps/manifest.json key "c64".
    let reg = kit::register(
        &xns,
        sid,
        gam::APP_NAME_C64,
        gam::UxType::Framebuffer,
        kit::Opcodes {
            redraw: AppOp::Redraw.to_u32().unwrap(),
            rawkeys: Some(AppOp::Rawkeys.to_u32().unwrap()),
            focus_change: Some(AppOp::FocusChange.to_u32().unwrap()),
            got_input: None,
        },
    );

    let games = store::Store::open("c64.games");
    let roms = store::Store::open("c64.roms");
    store::watch_bases(xous::connect(sid).unwrap(), AppOp::BasisChanged.to_usize().unwrap());
    let pump = pump::Pump::spawn(xous::connect(sid).unwrap(), AppOp::Frame.to_usize().unwrap(), FRAME_MS);
    let importer = import::Importer::new(xous::connect(sid).unwrap(), AppOp::ImportDone.to_usize().unwrap());

    let mut app = app::App::new(reg.screen, games, roms, importer);

    loop {
        let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(AppOp::Redraw) => app.draw(true),
            Some(AppOp::Rawkeys) => xous::msg_scalar_unpack!(msg, k1, k2, k3, k4, {
                for key in keys::decode_rawkeys([k1, k2, k3, k4]) {
                    match app.on_key(key) {
                        app::Action::Stay => {}
                        app::Action::Leave => kit::leave(app.screen()),
                    }
                }
                if app.wants_frames() { pump.run() } else { pump.stop() }
                app.draw(false);
            }),
            Some(AppOp::FocusChange) => xous::msg_scalar_unpack!(msg, code, _, _, _, {
                match kit::focus_state(code) {
                    gam::FocusState::Foreground => {
                        app.on_foreground();
                        if app.wants_frames() {
                            pump.run();
                        }
                    }
                    gam::FocusState::Background => {
                        pump.stop();
                        app.on_background();
                    }
                }
            }),
            Some(AppOp::Frame) => {
                app.on_frame();
                if !app.wants_frames() {
                    pump.stop();
                }
                pump::ack(&msg);
            }
            Some(AppOp::ImportDone) => {
                app.on_import_done();
                app.draw(true);
            }
            Some(AppOp::BasisChanged) => app.reload(),
            Some(AppOp::Quit) => break,
            None => log::warn!("unknown opcode {:?}", msg.body.id()),
        }
    }
    pump.quit();
    kit::shutdown(&xns, sid)
}
