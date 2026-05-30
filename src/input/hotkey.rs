use evdev::{Device, InputEventKind, Key};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::watch;
use tracing::{info, warn};

/// Spawns one blocking watcher per keyboard device.
/// `code_rx` carries the live hotkey code — changing the sender's value
/// takes effect on the next event without restarting the listener.
pub fn start_hotkey_listener(
    initial_code: u16,
    code_rx: watch::Receiver<u16>,
    flag: Arc<AtomicBool>,
) {
    let keyboards = find_keyboards_with_fkeys();

    if keyboards.is_empty() {
        warn!("No keyboard device found in /dev/input");
        return;
    }

    info!("Watching {} keyboard device(s) for hotkey", keyboards.len());

    for path in keyboards {
        let rx = code_rx.clone();
        let flag_clone = flag.clone();
        tokio::task::spawn_blocking(move || {
            listen_device(path, rx, flag_clone);
        });
    }

    let _ = initial_code; // used by caller to initialise the watch channel
}

/// Find all /dev/input/event* devices that support at least one F-key.
/// All standard keyboards support the full F1-F12 range, so this discovers
/// any keyboard that can fire whatever hotkey the user picks.
fn find_keyboards_with_fkeys() -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir("/dev/input") else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_event = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with("event"))
            .unwrap_or(false);
        if !is_event {
            continue;
        }
        if let Ok(dev) = Device::open(&path) {
            // Any device that supports KEY_F6 is a full keyboard
            if dev
                .supported_keys()
                .map(|k| k.contains(Key::KEY_F6))
                .unwrap_or(false)
            {
                found.push(path);
            }
        }
    }
    found
}

fn listen_device(path: PathBuf, code_rx: watch::Receiver<u16>, flag: Arc<AtomicBool>) {
    let mut device = match Device::open(&path) {
        Ok(d) => d,
        Err(e) => {
            warn!("Cannot open {}: {e}", path.display());
            return;
        }
    };

    loop {
        match device.fetch_events() {
            Ok(events) => {
                // Read the current key code on every batch — reflects UI changes immediately
                let current = Key::new(*code_rx.borrow());
                for event in events {
                    if let InputEventKind::Key(k) = event.kind() {
                        if k == current && event.value() == 1 {
                            flag.store(true, Ordering::SeqCst);
                        }
                    }
                }
            }
            Err(e) => {
                warn!("Error reading {}: {e}", path.display());
                break;
            }
        }
    }
}
