use crate::config::{ClickConfig, MouseButton, PositionMode};
use crate::ui::state::SharedState;
use anyhow::{anyhow, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::watch;
use tokio::time::{sleep, Duration};
use tracing::{error, info};
use uinput::event::controller::{Controller, Mouse};
use uinput::event::relative::{Position, Relative};

pub fn create_device() -> Result<uinput::Device> {
    uinput::default()
        .map_err(|e| anyhow!("Cannot open /dev/uinput: {e}"))?
        .name("VibeClicker")
        .map_err(|e| anyhow!("{e}"))?
        .event(Controller::Mouse(Mouse::Left))
        .map_err(|e| anyhow!("{e}"))?
        .event(Controller::Mouse(Mouse::Right))
        .map_err(|e| anyhow!("{e}"))?
        .event(Controller::Mouse(Mouse::Middle))
        .map_err(|e| anyhow!("{e}"))?
        .event(Relative::Position(Position::X))
        .map_err(|e| anyhow!("{e}"))?
        .event(Relative::Position(Position::Y))
        .map_err(|e| anyhow!("{e}"))?
        .create()
        .map_err(|e| anyhow!("Failed to create uinput device: {e}"))
}

pub async fn run_click_loop(
    mut device: uinput::Device,
    config: ClickConfig,
    state: SharedState,
    mut stop_rx: watch::Receiver<bool>,
    done_flag: Arc<AtomicBool>,
) {
    let interval = Duration::from_millis(config.interval_ms);
    let mut count: u64 = 0;

    loop {
        if *stop_rx.borrow() {
            break;
        }

        if let Some(limit) = config.click_limit {
            if count >= limit {
                let mut s = state.lock().unwrap();
                s.is_running = false;
                s.stop_tx = None;
                break;
            }
        }

        if config.position_mode == PositionMode::Fixed {
            move_to_fixed(&mut device, config.fixed_x, config.fixed_y);
        }

        if let Err(e) = perform_click(&mut device, &config.button) {
            error!("Click error: {e}");
            break;
        }

        count += 1;
        state.lock().unwrap().total_clicks += 1;

        tokio::select! {
            _ = sleep(interval) => {}
            _ = stop_rx.changed() => {
                if *stop_rx.borrow() { break; }
            }
        }
    }

    info!("Click loop ended after {count} clicks");
    done_flag.store(true, Ordering::SeqCst);
}

// Resets cursor to near origin via large negative delta, then moves to target.
// Best-effort for fixed position under Wayland — we cannot read the actual cursor position.
fn move_to_fixed(device: &mut uinput::Device, x: i32, y: i32) {
    let _ = device
        .send(Relative::Position(Position::X), -32767)
        .and_then(|_| device.send(Relative::Position(Position::Y), -32767))
        .and_then(|_| device.synchronize())
        .and_then(|_| device.send(Relative::Position(Position::X), x))
        .and_then(|_| device.send(Relative::Position(Position::Y), y))
        .and_then(|_| device.synchronize());
}

fn perform_click(device: &mut uinput::Device, button: &MouseButton) -> Result<()> {
    let btn = match button {
        MouseButton::Left | MouseButton::Double => Controller::Mouse(Mouse::Left),
        MouseButton::Right => Controller::Mouse(Mouse::Right),
        MouseButton::Middle => Controller::Mouse(Mouse::Middle),
    };

    press_release(device, &btn)?;

    if matches!(button, MouseButton::Double) {
        // Short inter-click gap for double-click recognition by target apps
        std::thread::sleep(std::time::Duration::from_millis(20));
        press_release(device, &btn)?;
    }

    Ok(())
}

fn press_release(device: &mut uinput::Device, btn: &Controller) -> Result<()> {
    device.press(btn).map_err(|e| anyhow!("{e}"))?;
    device.synchronize().map_err(|e| anyhow!("{e}"))?;
    device.release(btn).map_err(|e| anyhow!("{e}"))?;
    device.synchronize().map_err(|e| anyhow!("{e}"))?;
    Ok(())
}
