use crate::config::ClickConfig;
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub struct AppState {
    pub config: ClickConfig,
    pub is_running: bool,
    pub total_clicks: u64,
    pub session_start: Option<Instant>,
    pub stop_tx: Option<tokio::sync::watch::Sender<bool>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            config: ClickConfig::load(),
            is_running: false,
            total_clicks: 0,
            session_start: None,
            stop_tx: None,
        }
    }

    pub fn elapsed_secs(&self) -> f64 {
        self.session_start
            .map(|s| s.elapsed().as_secs_f64())
            .unwrap_or(0.0)
    }

    pub fn clicks_per_sec(&self) -> f64 {
        let e = self.elapsed_secs();
        if e > 0.1 {
            self.total_clicks as f64 / e
        } else {
            0.0
        }
    }
}

pub type SharedState = Arc<Mutex<AppState>>;
