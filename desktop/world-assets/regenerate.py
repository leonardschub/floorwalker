"""Regenerate every world piece: the build step before `tauri dev` and `tauri build` (ADR-0012).

Runs each piece script in headless Blender into the ignored `out/` folder beside this file, so the
folder holds `<piece>.glb` and the shared `textures/`. Skipped when the fingerprint saved with the
output matches: a hash of every script here and of the Blender version. The committed third-party
files (the font) are copied into `out/` on every run, skipped or not, since no piece is made from
them. Plain Python 3, no Blender:

  python3 desktop/world-assets/regenerate.py            # regenerate if a script changed
  python3 desktop/world-assets/regenerate.py --force    # empty out/ and rebake everything

Needs Blender 4.3.x with numpy (the minor is pinned so bakes match across machines). `BLENDER`
names the Blender command (default `blender` on PATH).
"""
import fcntl
import hashlib
import os
import re
import shutil
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "out")
FINGERPRINT = os.path.join(OUT, ".fingerprint")
LOG = os.path.join(HERE, "regenerate.log")  # beside out/, which ships whole
BLENDER_MINOR = (4, 3)
# Folders whose files no piece is made from, so they are not part of the fingerprint.
SKIP_DIRS = ("out", "__pycache__", "third-party", "tests")

# One script per piece, each writing `<piece>.glb` (README.md, "Pieces").
PIECES = (
    "desk_set",
    "robot",
    "room_kit",
    "corridor_kit",
    "sign_plate",
    "clutter_crumpled_paper",
    "clutter_rusty_cans",
    "clutter_cardboard_crates",
    "clutter_loose_sheets",
    "clutter_floor_cables",
    "clutter_bucket",
    "clutter_broken_chair",
    "cctv_room_kit",
    "cctv_monitors",
    "cctv_console",
    "cctv_sign_plate",
    "wall_camera",
    "squad_robot",
    "wreck_parts",
    "effects",
)

# Committed third-party files copied into the output as they are: (under `third-party/`, in `out/`).
THIRD_PARTY = (
    ("oswald/Oswald[wght].ttf", "fonts/oswald.ttf"),
)

INSTALL = (
    "Install Blender 4.3.x and numpy for its Python (Debian: `sudo apt install blender python3-numpy`; "
    "elsewhere the official 4.3 build from https://download.blender.org/release/Blender4.3/). "
    "Set BLENDER to its path if it is not `blender` on PATH."
)


def stop(problem):
    sys.exit(f"world assets: {problem}\n{INSTALL}")


def blender_env():
    """Headless: no display, so Blender never reaches for the compositor."""
    env = dict(os.environ)
    env.pop("WAYLAND_DISPLAY", None)
    env.pop("DISPLAY", None)
    return env


def check_blender(blender):
    """The Blender version line, after checking it is 4.3.x and its Python has numpy."""
    try:
        run = subprocess.run([blender, "--version"], capture_output=True, text=True, env=blender_env(),
                             timeout=120)
    except FileNotFoundError:
        stop(f"Blender was not found (`{blender}`).")
    except OSError as error:
        stop(f"Blender (`{blender}`) could not be run: {error}.")
    found = re.search(r"^Blender (\d+)\.(\d+)\.(\d+)", run.stdout, re.M)
    if run.returncode != 0 or not found:
        stop(f"`{blender} --version` did not report a Blender version.")
    version = found.group(0)
    if tuple(map(int, found.groups()[:2])) != BLENDER_MINOR:
        stop(f"{version} is installed, but the world assets need Blender {BLENDER_MINOR[0]}.{BLENDER_MINOR[1]}.x.")
    numpy = subprocess.run(
        [blender, "-b", "--factory-startup", "--python-exit-code", "1", "--python-expr", "import numpy"],
        capture_output=True, text=True, env=blender_env(), timeout=120)
    if numpy.returncode != 0:
        stop(f"{version}'s Python has no numpy, which its glTF exporter needs.")
    return version


def fingerprint(version):
    """A hash of the Blender version and every file Blender's output depends on, in a fixed order.
    `third-party/` is left out: it is copied, never baked (`copy_third_party`); so is `tests/`."""
    h = hashlib.sha256(version.encode() + b"\0")
    for root, dirs, files in os.walk(HERE):
        dirs[:] = sorted(d for d in dirs if d not in SKIP_DIRS)
        for name in sorted(files):
            if root == HERE and not name.endswith(".py"):
                continue
            path = os.path.join(root, name)
            h.update(os.path.relpath(path, HERE).encode() + b"\0")
            with open(path, "rb") as f:
                h.update(hashlib.sha256(f.read()).digest())
    return h.hexdigest()


def current(stamp):
    """Whether out/ holds every piece from this exact fingerprint."""
    try:
        with open(FINGERPRINT) as f:
            saved = f.read().strip()
    except FileNotFoundError:
        return False
    return saved == stamp and all(os.path.exists(os.path.join(OUT, f"{p}.glb")) for p in PIECES)


def copy_third_party():
    """Copies each committed third-party file into out/, rewriting only a copy that differs."""
    for source, target in THIRD_PARTY:
        source, target = os.path.join(HERE, "third-party", source), os.path.join(OUT, target)
        with open(source, "rb") as f:
            data = f.read()
        try:
            with open(target, "rb") as f:
                if f.read() == data:
                    continue
        except FileNotFoundError:
            pass
        os.makedirs(os.path.dirname(target), exist_ok=True)
        with open(target, "wb") as f:
            f.write(data)
        print(f"world assets: copied {os.path.relpath(target, OUT)}", flush=True)


def regenerate(blender, force):
    if force:
        shutil.rmtree(OUT, ignore_errors=True)
    os.makedirs(OUT, exist_ok=True)
    for name in os.listdir(OUT):  # pieces whose script is gone
        if name.endswith(".glb") and name[:-4] not in PIECES:
            os.remove(os.path.join(OUT, name))
    began = time.monotonic()
    with open(LOG, "w") as log:
        for i, piece in enumerate(PIECES, 1):
            print(f"world assets: [{i}/{len(PIECES)}] {piece}", flush=True)
            log.write(f"=== {piece}\n")
            log.flush()
            cmd = [blender, "-b", "--factory-startup", "--python-exit-code", "1",
                   "--python", os.path.join(HERE, f"{piece}.py"), "--", OUT]
            if force:
                cmd.append("--rebake")
            run = subprocess.run(cmd, stdout=log, stderr=subprocess.STDOUT, env=blender_env())
            if run.returncode != 0 or not os.path.exists(os.path.join(OUT, f"{piece}.glb")):
                log.close()
                with open(LOG) as f:
                    tail = f.read().splitlines()[-30:]
                sys.exit("world assets: {} failed (full log: {}):\n{}".format(piece, LOG, "\n".join(tail)))
    print(f"world assets: regenerated in {time.monotonic() - began:.0f} s into {OUT}")


def main():
    force = "--force" in sys.argv[1:]
    blender = os.environ.get("BLENDER") or "blender"
    version = check_blender(blender)
    # One run at a time (say `tauri dev` and `tauri build` started together): a second waits.
    lock = open(os.path.join(HERE, "regenerate.lock"), "w")
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        print("world assets: another regeneration is running; waiting for it", flush=True)
        fcntl.flock(lock, fcntl.LOCK_EX)
    stamp = fingerprint(version)
    if not force and current(stamp):
        copy_third_party()
        print(f"world assets: up to date ({version}, fingerprint {stamp[:12]})")
        return
    # An interrupted run must not look finished, so the fingerprint goes first and comes back last.
    if os.path.exists(FINGERPRINT):
        os.remove(FINGERPRINT)
    regenerate(blender, force)
    copy_third_party()
    with open(FINGERPRINT, "w") as f:
        f.write(stamp + "\n")


if __name__ == "__main__":
    main()
