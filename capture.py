"""Renode capture sequence for the C64 emulator (not yet run).

Run from xous-dev-toolkit:
    python3 scripts/renode_capture.py --app c64 --script ../precursor-c64/capture.py

Contract: run(ctl, outdir) -> number of failed steps. The app is already launched.
"""
import os


def run(ctl, outdir) -> int:
    failures = 0

    def shot(name):
        nonlocal failures
        if not ctl.try_screenshot(os.path.join(outdir, name)):
            failures += 1

    shot("01_menu.png")
    ctl.timed_key("Down", after=1.0)   # past any stored games: "Boot C64 (no game)"
    ctl.timed_key("Down", after=1.0)
    ctl.inject_line("")                # Enter: boot the built-in stub machine
    ctl.sleep(3.0)
    shot("02_stub_banner.png")         # banner and READY. from the built-in stub
    ctl.inject_string("hello")         # the stub has no keyboard scan: screen must not change
    ctl.sleep(1.0)
    shot("03_after_keys.png")
    ctl.timed_key("F4", after=1.0)     # back to the menu
    shot("04_menu_again.png")
    ctl.timed_key("F4", after=1.0)     # leave the app
    return failures
