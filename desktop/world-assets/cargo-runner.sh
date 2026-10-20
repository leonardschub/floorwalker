#!/bin/sh
# Tauri's runner (`build.runner` in desktop/src-tauri/tauri.conf.json): regenerates the world
# assets (ADR-0012), then hands over to cargo with Tauri's arguments. Tauri calls its runner
# for both `tauri dev` and `tauri build`, after the frontend is up or built, so a first
# regeneration's many minutes do not run out `tauri dev`'s wait for its dev server.
python3 "$(dirname "$0")/regenerate.py" || exit 1
exec cargo "$@"
