# VibeClicker

A Wayland-native auto clicker for Linux, built in Rust. Injects mouse clicks through the kernel's `/dev/uinput` interface — no X11, no XWayland, no `xdotool`.

---

## Features

- Configurable click interval (hours / minutes / seconds / milliseconds) with quick presets (1/s through 100/s)
- Left, right, middle, and double-click support
- **Follow Cursor** mode — clicks wherever your cursor currently sits
- **Fixed XY** mode — teleports to a fixed screen coordinate and clicks there
- Global hotkey toggle (F1–F12, configurable) — starts/stops clicking without focusing the window
- Capture-position hotkey — press a key to snapshot the live cursor position into the X/Y fields
- Optional click limit — automatically stops after N clicks
- Live stats: clicks per second, total count, elapsed time
- Config auto-saved to `~/.config/vibeclicker/config.toml` on every start

---

## System Requirements

| Requirement | Notes |
|---|---|
| Linux, Wayland compositor | X11 is not supported |
| Kernel with `uinput` module | May need to be loaded — see setup below |
| Rust toolchain (1.70+) | Install via [rustup.rs](https://rustup.rs) |
| GTK 4.10+ development libraries | See install commands below |
| `libevdev` development headers | Required by the `evdev` crate |
| Membership of the `input` group | Required to read `/dev/input/event*` and write `/dev/uinput` |

---

## Dependencies

### Rust crates (managed by Cargo, no manual installation needed)

| Crate | Version | Purpose |
|---|---|---|
| `gtk4` | 0.9 | GUI (GTK 4.10+, Wayland-native) |
| `tokio` | 1 | Async runtime for the click loop and hotkey listeners |
| `uinput` | 0.1 | Writes synthetic mouse events to `/dev/uinput` |
| `evdev` | 0.12 | Reads raw keyboard and mouse events from `/dev/input/event*` |
| `serde` + `toml` | 1 / 0.8 | Config serialisation |
| `anyhow` | 1 | Error handling |
| `tracing` + `tracing-subscriber` | 0.1 / 0.3 | Structured logging |

### System libraries

These must be installed before running `cargo build`.

**Debian / Ubuntu / Pop!_OS / Linux Mint:**

```bash
sudo apt install libgtk-4-dev libevdev-dev
```

**Fedora / RHEL / CentOS Stream:**

```bash
sudo dnf install gtk4-devel libevdev-devel
```

**Arch Linux / Manjaro:**

```bash
sudo pacman -S gtk4 libevdev
```

**openSUSE:**

```bash
sudo zypper install gtk4-devel libevdev-devel
```

---

## System Changes

VibeClicker requires three changes to your system. All are reversible.

### 1. `uinput` kernel module + udev permissions

**What it does:** Two things:
- Loads the `uinput` kernel module so `/dev/uinput` exists at all.
- Installs a udev rule that sets the device group to `input` with mode `0660`, so members of the `input` group can open it without `sudo`.

**Why this is needed:** Even if you're in the `input` group, `/dev/uinput` is created as `root:root` by default. The udev rule fixes the ownership every time the device node is created (on boot or after `modprobe`).

**How to apply (one-time):**

```bash
echo 'KERNEL=="uinput", GROUP="input", MODE="0660"' | sudo tee /etc/udev/rules.d/99-uinput.rules
sudo udevadm control --reload-rules && sudo udevadm trigger
echo uinput | sudo tee /etc/modules-load.d/uinput.conf
```

The first two lines install the udev rule and apply it immediately. The third ensures the `uinput` module is loaded automatically on every boot.

**Verify it worked:**

```bash
stat -c "%a %G" /dev/uinput
# expected output: 660 input
```

**How to revert:**

```bash
sudo rm /etc/udev/rules.d/99-uinput.rules /etc/modules-load.d/uinput.conf
sudo udevadm control --reload-rules && sudo udevadm trigger
```

`/dev/uinput` will revert to `root:root` ownership. The module will no longer load at boot (though it stays loaded until the next reboot).

---

### 2. Group membership — `input` group

**What it does:** Adds your user account to the `input` group, which grants read access to `/dev/input/event*` (keyboard devices, for global hotkeys) and write access to `/dev/uinput` (for injecting synthetic clicks).

**Effect:** Any process running as your user can read raw input events and inject synthetic input. This is the standard method used by tools like `ydotool` and `inputplug`.

**How to apply:**

```bash
sudo usermod -aG input $USER
```

You must log out and back in (or reboot) for the group change to take effect. You can verify it worked with:

```bash
groups
# output should include: input
```

**How to revert:**

```bash
sudo gpasswd -d $USER input
```

Log out and back in afterward. Your user will no longer have access to `/dev/input/event*` or `/dev/uinput`.

---

### 3. Config file — `~/.config/vibeclicker/config.toml`

**What it does:** VibeClicker creates this file the first time you press Start. It stores your last-used settings (interval, button, hotkey, position mode, XY coordinates, click limit).

**Effect:** A single small TOML file under your home directory. No system-wide changes.

**How to revert:**

```bash
rm -rf ~/.config/vibeclicker
```

This removes the config directory entirely. VibeClicker will recreate it with defaults the next time you press Start.

---

## Build

```bash
git clone <repo-url>
cd vibeclicker
cargo build --release
```

The compiled binary will be at `./target/release/vibeclicker`.

---

## Run

```bash
./target/release/vibeclicker
```

Or directly via Cargo (includes recompilation if needed):

```bash
cargo run --release
```

With debug logging:

```bash
RUST_LOG=vibeclicker=debug cargo run
```

---

## User Guide

### First launch

1. Make sure you are in the `input` group (see [System Changes](#system-changes) above).
2. Run `./target/release/vibeclicker`.
3. If `/dev/uinput` is not accessible, the app shows an error dialog with the exact command to fix it.

### Setting the click interval

Use the **Hours / Min / Sec / Ms** spinners, or click one of the preset buttons:

| Preset | Interval |
|---|---|
| 1/s | 1000 ms |
| 10/s | 100 ms |
| 20/s | 50 ms |
| 50/s | 20 ms |
| 100/s | 10 ms |

The minimum enforced interval is 10 ms.

### Choosing a mouse button

The **Mouse Button** dropdown selects which button is clicked: Left, Right, Middle, or Double (two rapid left clicks).

### Click position

**Follow Cursor** — clicks wherever the cursor is at the moment of each click. This is the default.

**Fixed XY** — moves the cursor to the specified screen coordinate before each click. Enter pixel coordinates in the X and Y fields.

> **Note on Fixed XY under Wayland:** Wayland does not expose the absolute cursor position to applications. VibeClicker approximates the target by sending a large negative relative movement to push the cursor near the top-left corner of the screen, then sending the target deltas. This is best-effort — if the cursor is already far from the top-left the result may be off. Use **Capture** to set the coordinates from the cursor's actual position instead.

#### Capturing a position

With **Fixed XY** selected, move your cursor to the target location, then press the **Capture** hotkey (default: F7). The live coordinates shown next to "Live cursor:" will be copied into the X / Y fields.

The **↺ Reset** button zeroes the internal position accumulator. Use it if the live cursor display drifts — move your cursor to the very top-left corner of your screen, then click Reset.

### Global hotkey

The **Global Hotkey (Toggle)** dropdown (default: F6) starts and stops clicking from anywhere — no need to click the Start button. The hotkey listener watches all keyboards via `/dev/input/event*`.

Changing the dropdown takes effect immediately without restarting.

### Click limit

Enable **Limit to:** and enter a number to automatically stop after that many clicks. Leave it unchecked for unlimited clicking.

### Starting and stopping

Click **▶ Start Clicking** or press the configured toggle hotkey. The button changes to **■ Stop Clicking** while active. Press again (or the hotkey) to stop.

The stats bar at the bottom shows clicks per second, total clicks, and elapsed time.

---

## Uninstalling

1. Delete the binary: `rm ./target/release/vibeclicker` (or wherever you copied it).
2. Remove config: `rm -rf ~/.config/vibeclicker`.
3. Remove udev rule and module autoload:
   ```bash
   sudo rm /etc/udev/rules.d/99-uinput.rules /etc/modules-load.d/uinput.conf
   sudo udevadm control --reload-rules && sudo udevadm trigger
   ```
4. Remove from `input` group if no longer needed: `sudo gpasswd -d $USER input`, then log out.
5. Optionally remove system libraries installed earlier (e.g. `sudo apt remove libgtk-4-dev libevdev-dev`).

No other files are written to your system.
