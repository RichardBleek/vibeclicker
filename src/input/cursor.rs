use evdev::{Device, InputEventKind, RelativeAxisType};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use tracing::warn;

/// Spawns a blocking reader per pointer device that accumulates REL_X/REL_Y
/// deltas into cursor_x / cursor_y. Clamps at 0 (no negative coordinates).
/// The "AutoClicker" virtual device is excluded to avoid feedback.
pub fn start_cursor_tracker(cursor_x: Arc<AtomicI32>, cursor_y: Arc<AtomicI32>) {
    let devices = find_pointer_devices();
    if devices.is_empty() {
        warn!("No pointer device found for cursor tracking");
        return;
    }
    for path in devices {
        let cx = cursor_x.clone();
        let cy = cursor_y.clone();
        tokio::task::spawn_blocking(move || track_device(path, cx, cy));
    }
}

fn find_pointer_devices() -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir("/dev/input") else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with("event"))
            .unwrap_or(false)
        {
            continue;
        }
        if let Ok(dev) = Device::open(&path) {
            if dev.name().map(|n| n == "VibeClicker").unwrap_or(false) {
                continue;
            }
            if dev
                .supported_relative_axes()
                .map(|a| {
                    a.contains(RelativeAxisType::REL_X) && a.contains(RelativeAxisType::REL_Y)
                })
                .unwrap_or(false)
            {
                found.push(path);
            }
        }
    }
    found
}

fn track_device(path: PathBuf, cursor_x: Arc<AtomicI32>, cursor_y: Arc<AtomicI32>) {
    let mut device = match Device::open(&path) {
        Ok(d) => d,
        Err(e) => {
            warn!("Cannot open {} for cursor tracking: {e}", path.display());
            return;
        }
    };
    loop {
        match device.fetch_events() {
            Ok(events) => {
                for event in events {
                    match event.kind() {
                        InputEventKind::RelAxis(RelativeAxisType::REL_X) => {
                            let new = cursor_x.fetch_add(event.value(), Ordering::Relaxed)
                                + event.value();
                            if new < 0 {
                                cursor_x.store(0, Ordering::Relaxed);
                            }
                        }
                        InputEventKind::RelAxis(RelativeAxisType::REL_Y) => {
                            let new = cursor_y.fetch_add(event.value(), Ordering::Relaxed)
                                + event.value();
                            if new < 0 {
                                cursor_y.store(0, Ordering::Relaxed);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                warn!("Cursor tracker error on {}: {e}", path.display());
                break;
            }
        }
    }
}
