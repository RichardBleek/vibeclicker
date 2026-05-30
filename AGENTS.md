# vibeclicker — agent guide

Wayland-native auto clicker built in Rust. No X11 dependency; all input injection goes through `/dev/uinput`.

## Stack

| Layer | Crate | Notes |
|-------|-------|-------|
| GUI | gtk4-rs 0.9 | GTK 4.14+, Wayland-native, main thread only |
| Click injection | uinput 0.1 | Writes to `/dev/uinput`; requires `input` group membership |
| Global hotkeys | evdev 0.12 | Reads raw `/dev/input/event*` devices |
| Async runtime | tokio (full) | Multi-threaded; runtime kept alive via `rt.enter()` guard in `main` |
| Config | serde + toml | Persisted to `~/.config/vibeclicker/config.toml` |
| Error handling | anyhow | |

## Source layout

```
src/
  main.rs              — GTK app init; Tokio runtime setup
  config.rs            — ClickConfig, MouseButton, PositionMode, HOTKEY_OPTIONS
  input/
    mod.rs
    clicker.rs         — create_device() (uinput "VibeClicker"), run_click_loop(), move_to_fixed(), perform_click()
    hotkey.rs          — start_hotkey_listener(), find_keyboards_with_fkeys(), listen_device()
  ui/
    mod.rs
    state.rs           — AppState, SharedState (Arc<Mutex<AppState>>)
    window.rs          — MainWindow (GTK4 ObjectSubclass), all UI logic
```

## Key architectural decisions

**Tokio + GTK coexistence** — GTK runs on the main thread. A multi-threaded Tokio runtime is built in `main()` and its `EnterGuard` kept alive for the process lifetime, which makes `tokio::spawn` work from GTK callbacks without `#[tokio::main]`.

**Cross-thread signalling** — `glib::MainContext::channel` was removed in glib 0.20. Cross-thread notifications use `Arc<AtomicBool>` flags polled by `glib::timeout_add_local` (200 ms tick). This avoids the `Send` requirement that `glib::idle_add_once` would impose on `WeakRef<MainWindow>`.

**Live hotkey code** — `start_hotkey_listener` receives a `tokio::sync::watch::Receiver<u16>`. Each blocking listener task calls `*code_rx.borrow()` on every event batch so the active key updates immediately when the user changes the hotkey dropdown — no restart needed.

**Config snapshot** — All other settings (button, interval, position mode, XY, click limit) are snapshotted via `read_config_from_ui()` at the moment Start is pressed. `run_click_loop` receives a plain `ClickConfig` value; mid-run changes don't apply until the next Start.

**Fixed-position under Wayland** — Cursor position is unreadable under Wayland. `move_to_fixed()` sends `REL_X/Y = -32767` to push the cursor near the origin, then sends the target deltas. This is best-effort.

**uinput Send safety** — `uinput::Device` wraps only a `c_int` fd, so it auto-derives `Send` and can be moved into a `tokio::spawn` future.

## Permissions

The user must be in the `input` group:

```bash
sudo usermod -aG input $USER   # then log out/in
```

If `/dev/uinput` is not writable the app shows a GTK `AlertDialog` describing the fix.

## Build & run

```bash
cargo build --release
./target/release/vibeclicker
# or for debug logging:
RUST_LOG=vibeclicker=debug cargo run
```

## Constraints — do not violate

- No `xdotool`, `ydotool`, X11, or XWayland dependency.
- Click loop sleep must use `tokio::time::sleep`, not `std::thread::sleep` (except the 20 ms double-click gap inside `perform_click`, which is intentionally blocking to keep the gap tight).
- All GTK widget access must happen on the main thread.
- Do not add `glib::idle_add_once` with closures that capture `WeakRef<MainWindow>` — `WeakRef` is `!Send`.
