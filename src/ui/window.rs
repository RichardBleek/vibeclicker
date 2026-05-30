use crate::config::{MouseButton, PositionMode, HOTKEY_OPTIONS};
use crate::input::{clicker, cursor, hotkey};
use crate::ui::state::SharedState;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::watch;
use tracing::error;

// ── Inner GObject implementation ──────────────────────────────────────────────

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct MainWindow {
        // Interval spinners
        pub spin_hours: RefCell<Option<gtk4::SpinButton>>,
        pub spin_minutes: RefCell<Option<gtk4::SpinButton>>,
        pub spin_seconds: RefCell<Option<gtk4::SpinButton>>,
        pub spin_ms: RefCell<Option<gtk4::SpinButton>>,

        // Button selector
        pub btn_dropdown: RefCell<Option<gtk4::DropDown>>,

        // Position mode
        pub chk_follow: RefCell<Option<gtk4::CheckButton>>,
        pub chk_fixed: RefCell<Option<gtk4::CheckButton>>,
        pub entry_x: RefCell<Option<gtk4::Entry>>,
        pub entry_y: RefCell<Option<gtk4::Entry>>,
        pub fixed_row: RefCell<Option<gtk4::Box>>,

        // Hotkey
        pub hotkey_dropdown: RefCell<Option<gtk4::DropDown>>,

        // Limit
        pub chk_limit: RefCell<Option<gtk4::CheckButton>>,
        pub spin_limit: RefCell<Option<gtk4::SpinButton>>,
        pub limit_spin_box: RefCell<Option<gtk4::Box>>,

        // Toggle button
        pub toggle_btn: RefCell<Option<gtk4::Button>>,

        // Stats labels
        pub lbl_cps: RefCell<Option<gtk4::Label>>,
        pub lbl_total: RefCell<Option<gtk4::Label>>,
        pub lbl_time: RefCell<Option<gtk4::Label>>,

        // Shared app state
        pub shared_state: RefCell<Option<SharedState>>,
        pub stats_timer: RefCell<Option<glib::SourceId>>,

        // Cross-thread flags polled by the glib timer
        pub hotkey_flag: RefCell<Option<Arc<AtomicBool>>>,
        pub loop_done: RefCell<Option<Arc<AtomicBool>>>,

        // Live hotkey code — changing this sender updates all listener tasks instantly
        pub hotkey_tx: RefCell<Option<watch::Sender<u16>>>,

        // Live cursor position accumulated from evdev REL events
        pub cursor_x: RefCell<Option<Arc<AtomicI32>>>,
        pub cursor_y: RefCell<Option<Arc<AtomicI32>>>,
        pub lbl_cursor: RefCell<Option<gtk4::Label>>,
        pub cursor_row: RefCell<Option<gtk4::Box>>,
        pub reset_cursor_btn: RefCell<Option<gtk4::Button>>,

        // Capture-position hotkey (copies live cursor → X/Y fields)
        pub capture_flag: RefCell<Option<Arc<AtomicBool>>>,
        pub capture_hotkey_tx: RefCell<Option<watch::Sender<u16>>>,
        pub capture_hk_dropdown: RefCell<Option<gtk4::DropDown>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MainWindow {
        const NAME: &'static str = "VibeClickerWindow";
        type Type = super::MainWindow;
        type ParentType = gtk4::ApplicationWindow;
    }

    impl ObjectImpl for MainWindow {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.set_title(Some("VibeClicker"));
            obj.set_default_size(480, -1);
            obj.set_resizable(false);
        }
    }

    impl WidgetImpl for MainWindow {}
    impl WindowImpl for MainWindow {}
    impl ApplicationWindowImpl for MainWindow {}

    // ── UI / logic helpers ────────────────────────────────────────────────────

    impl MainWindow {
        pub fn setup(&self, state: SharedState) {
            *self.shared_state.borrow_mut() = Some(state.clone());

            // Shared AtomicBool flags — safe to send to background threads
            let hotkey_flag = Arc::new(AtomicBool::new(false));
            let loop_done = Arc::new(AtomicBool::new(false));
            *self.hotkey_flag.borrow_mut() = Some(hotkey_flag.clone());
            *self.loop_done.borrow_mut() = Some(loop_done.clone());

            // Cursor position accumulators — set before build_ui so connect_signals can reference them
            let cx = Arc::new(AtomicI32::new(0));
            let cy = Arc::new(AtomicI32::new(0));
            *self.cursor_x.borrow_mut() = Some(cx.clone());
            *self.cursor_y.borrow_mut() = Some(cy.clone());

            let root = self.build_ui(&state);
            self.obj().set_child(Some(&root));
            self.connect_signals();

            // Toggle hotkey listener
            let code = state.lock().unwrap().config.hotkey_code;
            let (hotkey_tx, hotkey_rx) = watch::channel(code);
            *self.hotkey_tx.borrow_mut() = Some(hotkey_tx);
            hotkey::start_hotkey_listener(code, hotkey_rx, hotkey_flag.clone());

            // Capture-position hotkey listener
            let capture_flag = Arc::new(AtomicBool::new(false));
            *self.capture_flag.borrow_mut() = Some(capture_flag.clone());
            let cap_code = state.lock().unwrap().config.capture_hotkey_code;
            let (capture_tx, capture_rx) = watch::channel(cap_code);
            *self.capture_hotkey_tx.borrow_mut() = Some(capture_tx);
            hotkey::start_hotkey_listener(cap_code, capture_rx, capture_flag.clone());

            cursor::start_cursor_tracker(cx, cy);

            // 200 ms glib timer: polls AtomicBool flags and refreshes stats.
            // timeout_add_local does NOT require Send, so WeakRef<MainWindow> is fine.
            let obj = self.obj();
            let win_weak = obj.downgrade();
            let hk = hotkey_flag;
            let ld = loop_done;
            let cf = capture_flag;
            let id = glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
                let Some(win) = win_weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                let imp = win.imp();
                if hk.swap(false, Ordering::SeqCst) {
                    imp.toggle_clicking();
                }
                if ld.swap(false, Ordering::SeqCst) {
                    imp.on_click_loop_done();
                }
                if cf.swap(false, Ordering::SeqCst) {
                    imp.capture_cursor_position();
                }
                imp.update_stats();
                glib::ControlFlow::Continue
            });
            *self.stats_timer.borrow_mut() = Some(id);
        }

        fn build_ui(&self, state: &SharedState) -> gtk4::Box {
            let cfg = state.lock().unwrap().config.clone();

            let root = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
            root.set_margin_top(12);
            root.set_margin_bottom(12);
            root.set_margin_start(12);
            root.set_margin_end(12);
            root.set_spacing(10);

            // ── Interval ─────────────────────────────────────────────────────
            let interval_frame = gtk4::Frame::new(Some("Interval"));
            let interval_box = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
            interval_box.set_margin_top(8);
            interval_box.set_margin_bottom(8);
            interval_box.set_margin_start(8);
            interval_box.set_margin_end(8);

            let total_ms = cfg.interval_ms;
            let hh = total_ms / 3_600_000;
            let mm = (total_ms % 3_600_000) / 60_000;
            let ss = (total_ms % 60_000) / 1_000;
            let ms = total_ms % 1_000;

            let sh = make_spin(0.0, 23.0, hh as f64, 2);
            let sm = make_spin(0.0, 59.0, mm as f64, 2);
            let ss_spin = make_spin(0.0, 59.0, ss as f64, 2);
            let sms = make_spin(0.0, 999.0, ms as f64, 3);

            // Grid: row 0 = field labels, row 1 = spinners + separators
            let spin_grid = gtk4::Grid::new();
            spin_grid.set_halign(gtk4::Align::Center);
            spin_grid.set_column_spacing(4);
            spin_grid.set_row_spacing(2);

            for (col, text) in [(0, "Hours"), (2, "Min"), (4, "Sec"), (6, "Ms")] {
                let lbl = gtk4::Label::new(Some(text));
                lbl.add_css_class("caption");
                lbl.set_halign(gtk4::Align::Center);
                spin_grid.attach(&lbl, col, 0, 1, 1);
            }

            for (col, text) in [(1, ":"), (3, ":"), (5, ".")] {
                let sep = gtk4::Label::new(Some(text));
                sep.set_valign(gtk4::Align::Center);
                spin_grid.attach(&sep, col, 1, 1, 1);
            }

            spin_grid.attach(&sh, 0, 1, 1, 1);
            spin_grid.attach(&sm, 2, 1, 1, 1);
            spin_grid.attach(&ss_spin, 4, 1, 1, 1);
            spin_grid.attach(&sms, 6, 1, 1, 1);

            *self.spin_hours.borrow_mut() = Some(sh);
            *self.spin_minutes.borrow_mut() = Some(sm);
            *self.spin_seconds.borrow_mut() = Some(ss_spin);
            *self.spin_ms.borrow_mut() = Some(sms);

            interval_box.append(&spin_grid);

            // Quick presets
            let preset_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
            preset_row.set_halign(gtk4::Align::Center);
            let preset_lbl = gtk4::Label::new(Some("Presets:"));
            preset_lbl.add_css_class("caption");
            preset_row.append(&preset_lbl);
            for &(label, ms_val) in &[
                ("1/s", 1000u64),
                ("10/s", 100),
                ("20/s", 50),
                ("50/s", 20),
                ("100/s", 10),
            ] {
                let btn = gtk4::Button::with_label(label);
                btn.add_css_class("pill");
                let sh2 = self.spin_hours.borrow().clone().unwrap();
                let sm2 = self.spin_minutes.borrow().clone().unwrap();
                let ss2 = self.spin_seconds.borrow().clone().unwrap();
                let sms2 = self.spin_ms.borrow().clone().unwrap();
                btn.connect_clicked(move |_| {
                    sh2.set_value(0.0);
                    sm2.set_value(0.0);
                    ss2.set_value((ms_val / 1000) as f64);
                    sms2.set_value((ms_val % 1000) as f64);
                });
                preset_row.append(&btn);
            }
            interval_box.append(&preset_row);
            interval_frame.set_child(Some(&interval_box));
            root.append(&interval_frame);

            // ── Button selector ───────────────────────────────────────────────
            let btn_frame = gtk4::Frame::new(Some("Mouse Button"));
            let btn_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
            btn_box.set_margin_top(8);
            btn_box.set_margin_bottom(8);
            btn_box.set_margin_start(8);
            btn_box.set_margin_end(8);

            let dropdown = gtk4::DropDown::from_strings(&["Left", "Right", "Middle", "Double"]);
            let sel = match cfg.button {
                MouseButton::Left => 0,
                MouseButton::Right => 1,
                MouseButton::Middle => 2,
                MouseButton::Double => 3,
            };
            dropdown.set_selected(sel);
            dropdown.set_hexpand(true);
            btn_box.append(&dropdown);
            *self.btn_dropdown.borrow_mut() = Some(dropdown);
            btn_frame.set_child(Some(&btn_box));
            root.append(&btn_frame);

            // ── Position mode ─────────────────────────────────────────────────
            let pos_frame = gtk4::Frame::new(Some("Click Position"));
            let pos_box = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
            pos_box.set_margin_top(8);
            pos_box.set_margin_bottom(8);
            pos_box.set_margin_start(8);
            pos_box.set_margin_end(8);

            let chk_follow = gtk4::CheckButton::with_label("Follow Cursor");
            let chk_fixed = gtk4::CheckButton::with_label("Fixed XY");
            chk_fixed.set_group(Some(&chk_follow));

            if cfg.position_mode == PositionMode::Fixed {
                chk_fixed.set_active(true);
            } else {
                chk_follow.set_active(true);
            }

            let mode_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 16);
            mode_row.append(&chk_follow);
            mode_row.append(&chk_fixed);
            pos_box.append(&mode_row);

            let fixed_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
            fixed_row.set_halign(gtk4::Align::Center);

            let entry_x = gtk4::Entry::new();
            entry_x.set_text(&cfg.fixed_x.to_string());
            entry_x.set_width_chars(6);
            entry_x.set_input_purpose(gtk4::InputPurpose::Digits);

            let entry_y = gtk4::Entry::new();
            entry_y.set_text(&cfg.fixed_y.to_string());
            entry_y.set_width_chars(6);
            entry_y.set_input_purpose(gtk4::InputPurpose::Digits);

            fixed_row.append(&gtk4::Label::new(Some("X:")));
            fixed_row.append(&entry_x);
            fixed_row.append(&gtk4::Label::new(Some("Y:")));
            fixed_row.append(&entry_y);
            fixed_row.set_visible(cfg.position_mode == PositionMode::Fixed);
            pos_box.append(&fixed_row);

            // Live cursor position row — visible only in Fixed mode
            let cursor_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
            cursor_row.set_halign(gtk4::Align::Center);

            let lbl_live = gtk4::Label::new(Some("Live cursor:"));
            lbl_live.add_css_class("caption");

            let lbl_cursor = gtk4::Label::new(Some("—"));
            lbl_cursor.set_width_chars(12);
            lbl_cursor.set_xalign(0.0);

            // Capture hotkey dropdown
            let lbl_capture = gtk4::Label::new(Some("Capture:"));
            lbl_capture.add_css_class("caption");

            let cap_names: Vec<&str> = HOTKEY_OPTIONS.iter().map(|(_, n)| *n).collect();
            let cap_dropdown = gtk4::DropDown::from_strings(&cap_names);
            let cap_idx = HOTKEY_OPTIONS
                .iter()
                .position(|(c, _)| *c == cfg.capture_hotkey_code)
                .unwrap_or(6) as u32; // default F7
            cap_dropdown.set_selected(cap_idx);
            cap_dropdown.set_tooltip_text(Some("Press this key to copy live cursor position into X / Y"));

            let reset_btn = gtk4::Button::with_label("↺ Reset");
            reset_btn.add_css_class("pill");
            reset_btn.set_tooltip_text(Some(
                "Move cursor to top-left corner of screen, then click to reset origin",
            ));

            cursor_row.append(&lbl_live);
            cursor_row.append(&lbl_cursor);
            cursor_row.append(&lbl_capture);
            cursor_row.append(&cap_dropdown);
            cursor_row.append(&reset_btn);
            cursor_row.set_visible(cfg.position_mode == PositionMode::Fixed);
            pos_box.append(&cursor_row);

            *self.lbl_cursor.borrow_mut() = Some(lbl_cursor);
            *self.cursor_row.borrow_mut() = Some(cursor_row);
            *self.capture_hk_dropdown.borrow_mut() = Some(cap_dropdown);
            *self.reset_cursor_btn.borrow_mut() = Some(reset_btn);

            *self.chk_follow.borrow_mut() = Some(chk_follow);
            *self.chk_fixed.borrow_mut() = Some(chk_fixed);
            *self.entry_x.borrow_mut() = Some(entry_x);
            *self.entry_y.borrow_mut() = Some(entry_y);
            *self.fixed_row.borrow_mut() = Some(fixed_row);

            pos_frame.set_child(Some(&pos_box));
            root.append(&pos_frame);

            // ── Hotkey ────────────────────────────────────────────────────────
            let hk_frame = gtk4::Frame::new(Some("Global Hotkey (Toggle)"));
            let hk_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
            hk_box.set_margin_top(8);
            hk_box.set_margin_bottom(8);
            hk_box.set_margin_start(8);
            hk_box.set_margin_end(8);

            let hk_names: Vec<&str> = HOTKEY_OPTIONS.iter().map(|(_, n)| *n).collect();
            let hk_dropdown = gtk4::DropDown::from_strings(&hk_names);
            let hk_idx = HOTKEY_OPTIONS
                .iter()
                .position(|(c, _)| *c == cfg.hotkey_code)
                .unwrap_or(5) as u32;
            hk_dropdown.set_selected(hk_idx);
            hk_dropdown.set_hexpand(true);
            hk_box.append(&hk_dropdown);
            *self.hotkey_dropdown.borrow_mut() = Some(hk_dropdown);
            hk_frame.set_child(Some(&hk_box));
            root.append(&hk_frame);

            // ── Click limit ───────────────────────────────────────────────────
            let lim_frame = gtk4::Frame::new(Some("Click Limit"));
            let lim_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
            lim_box.set_margin_top(8);
            lim_box.set_margin_bottom(8);
            lim_box.set_margin_start(8);
            lim_box.set_margin_end(8);

            let chk_limit = gtk4::CheckButton::with_label("Limit to:");
            let limit_val = cfg.click_limit.unwrap_or(1000) as f64;
            let spin_limit = gtk4::SpinButton::with_range(1.0, 999_999.0, 1.0);
            spin_limit.set_value(limit_val);
            spin_limit.set_digits(0);

            let limit_spin_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
            limit_spin_box.append(&spin_limit.clone());
            limit_spin_box.append(&gtk4::Label::new(Some("clicks")));
            limit_spin_box.set_sensitive(cfg.click_limit.is_some());
            chk_limit.set_active(cfg.click_limit.is_some());

            lim_box.append(&chk_limit);
            lim_box.append(&limit_spin_box);
            *self.chk_limit.borrow_mut() = Some(chk_limit);
            *self.spin_limit.borrow_mut() = Some(spin_limit);
            *self.limit_spin_box.borrow_mut() = Some(limit_spin_box);
            lim_frame.set_child(Some(&lim_box));
            root.append(&lim_frame);

            // ── Toggle button ─────────────────────────────────────────────────
            root.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));

            let toggle_btn = gtk4::Button::with_label("▶  Start Clicking");
            toggle_btn.add_css_class("suggested-action");
            toggle_btn.set_height_request(48);
            *self.toggle_btn.borrow_mut() = Some(toggle_btn.clone());
            root.append(&toggle_btn);

            // ── Stats ─────────────────────────────────────────────────────────
            root.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));

            let stats_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
            stats_box.set_homogeneous(true);

            let lbl_cps = gtk4::Label::new(Some("0.0 /s"));
            lbl_cps.add_css_class("caption");
            let lbl_total = gtk4::Label::new(Some("Total: 0"));
            lbl_total.add_css_class("caption");
            let lbl_time = gtk4::Label::new(Some("00:00:00"));
            lbl_time.add_css_class("caption");

            stats_box.append(&lbl_cps);
            stats_box.append(&lbl_total);
            stats_box.append(&lbl_time);

            *self.lbl_cps.borrow_mut() = Some(lbl_cps);
            *self.lbl_total.borrow_mut() = Some(lbl_total);
            *self.lbl_time.borrow_mut() = Some(lbl_time);

            root.append(&stats_box);
            root
        }

        fn connect_signals(&self) {
            // Fixed row + cursor row visibility toggle
            if let (Some(chk), Some(row), Some(cr)) = (
                self.chk_fixed.borrow().clone(),
                self.fixed_row.borrow().clone(),
                self.cursor_row.borrow().clone(),
            ) {
                chk.connect_toggled(move |c| {
                    let active = c.is_active();
                    row.set_visible(active);
                    cr.set_visible(active);
                });
            }

            // Capture hotkey dropdown → live update of the watch channel
            let obj = self.obj();
            let win_weak = obj.downgrade();
            if let Some(dropdown) = self.capture_hk_dropdown.borrow().clone() {
                dropdown.connect_selected_notify(move |d| {
                    if let Some(w) = win_weak.upgrade() {
                        if let Some(tx) = w.imp().capture_hotkey_tx.borrow().as_ref() {
                            if let Some((code, _)) = HOTKEY_OPTIONS.get(d.selected() as usize) {
                                let _ = tx.send(*code);
                            }
                        }
                    }
                });
            }

            // Reset button — zero the cursor accumulators
            let obj = self.obj();
            let win_weak = obj.downgrade();
            if let Some(btn) = self.reset_cursor_btn.borrow().clone() {
                btn.connect_clicked(move |_| {
                    if let Some(w) = win_weak.upgrade() {
                        let imp = w.imp();
                        if let Some(cx) = imp.cursor_x.borrow().as_ref() { cx.store(0, Ordering::Relaxed); }
                        if let Some(cy) = imp.cursor_y.borrow().as_ref() { cy.store(0, Ordering::Relaxed); }
                    }
                });
            }

            // Limit spin sensitivity toggle
            if let (Some(chk), Some(box_)) = (
                self.chk_limit.borrow().clone(),
                self.limit_spin_box.borrow().clone(),
            ) {
                chk.connect_toggled(move |c| box_.set_sensitive(c.is_active()));
            }

            // Hotkey dropdown → live update of the watch channel so all listener
            // tasks pick up the new key code on their next event batch
            let obj = self.obj();
            let win_weak = obj.downgrade();
            if let Some(dropdown) = self.hotkey_dropdown.borrow().clone() {
                dropdown.connect_selected_notify(move |d| {
                    if let Some(w) = win_weak.upgrade() {
                        if let Some(tx) = w.imp().hotkey_tx.borrow().as_ref() {
                            if let Some((code, _)) = HOTKEY_OPTIONS.get(d.selected() as usize) {
                                let _ = tx.send(*code);
                            }
                        }
                    }
                });
            }

            // Toggle button
            let obj = self.obj();
            let win_weak = obj.downgrade();
            if let Some(btn) = self.toggle_btn.borrow().clone() {
                btn.connect_clicked(move |_| {
                    if let Some(w) = win_weak.upgrade() {
                        w.imp().toggle_clicking();
                    }
                });
            }
        }

        pub fn toggle_clicking(&self) {
            let running = self
                .shared_state
                .borrow()
                .as_ref()
                .map(|s| s.lock().unwrap().is_running)
                .unwrap_or(false);

            if running {
                self.stop_clicking();
            } else {
                self.start_clicking();
            }
        }

        fn start_clicking(&self) {
            let Some(state) = self.shared_state.borrow().clone() else {
                return;
            };

            let config = self.read_config_from_ui(&state);
            if let Err(e) = config.save() {
                error!("Config save failed: {e}");
            }

            let device = match clicker::create_device() {
                Ok(d) => d,
                Err(e) => {
                    self.show_error_dialog(&format!(
                        "Cannot open /dev/uinput\n\n{e}\n\nMake sure you are in the 'input' group:\n  sudo usermod -aG input $USER\nThen log out and back in."
                    ));
                    return;
                }
            };

            {
                let mut s = state.lock().unwrap();
                s.config = config.clone();
                s.is_running = true;
                s.total_clicks = 0;
                s.session_start = Some(Instant::now());
            }

            let (stop_tx, stop_rx) = watch::channel(false);
            state.lock().unwrap().stop_tx = Some(stop_tx);

            if let Some(btn) = self.toggle_btn.borrow().clone() {
                btn.set_label("■  Stop Clicking");
                btn.remove_css_class("suggested-action");
                btn.add_css_class("destructive-action");
            }

            // Reset done flag and spawn click loop
            let done = self
                .loop_done
                .borrow()
                .clone()
                .expect("loop_done not initialised");
            done.store(false, Ordering::SeqCst);

            tokio::spawn(clicker::run_click_loop(
                device, config, state, stop_rx, done,
            ));
        }

        fn stop_clicking(&self) {
            let Some(state) = self.shared_state.borrow().clone() else {
                return;
            };
            let mut s = state.lock().unwrap();
            s.is_running = false;
            if let Some(tx) = s.stop_tx.take() {
                let _ = tx.send(true);
            }
            // UI reset happens when the timer detects loop_done == true
        }

        pub fn capture_cursor_position(&self) {
            let x = self.cursor_x.borrow().as_ref().map(|a| a.load(Ordering::Relaxed)).unwrap_or(0);
            let y = self.cursor_y.borrow().as_ref().map(|a| a.load(Ordering::Relaxed)).unwrap_or(0);
            if let Some(e) = self.entry_x.borrow().clone() { e.set_text(&x.to_string()); }
            if let Some(e) = self.entry_y.borrow().clone() { e.set_text(&y.to_string()); }
        }

        pub fn on_click_loop_done(&self) {
            if let Some(state) = self.shared_state.borrow().clone() {
                let mut s = state.lock().unwrap();
                s.is_running = false;
                s.stop_tx = None;
            }
            if let Some(btn) = self.toggle_btn.borrow().clone() {
                btn.set_label("▶  Start Clicking");
                btn.remove_css_class("destructive-action");
                btn.add_css_class("suggested-action");
            }
        }

        pub fn update_stats(&self) {
            let Some(state) = self.shared_state.borrow().clone() else {
                return;
            };
            let s = state.lock().unwrap();
            let cps = s.clicks_per_sec();
            let total = s.total_clicks;
            let elapsed = s.elapsed_secs();
            drop(s);

            let h = (elapsed / 3600.0) as u64;
            let m = ((elapsed % 3600.0) / 60.0) as u64;
            let sec = (elapsed % 60.0) as u64;

            if let Some(l) = self.lbl_cps.borrow().clone() {
                l.set_text(&format!("{cps:.1} /s"));
            }
            if let Some(l) = self.lbl_total.borrow().clone() {
                l.set_text(&format!("Total: {total}"));
            }
            if let Some(l) = self.lbl_time.borrow().clone() {
                l.set_text(&format!("{h:02}:{m:02}:{sec:02}"));
            }

            // Refresh live cursor label (only visible when Fixed mode is active)
            let x = self.cursor_x.borrow().as_ref().map(|a| a.load(Ordering::Relaxed)).unwrap_or(0);
            let y = self.cursor_y.borrow().as_ref().map(|a| a.load(Ordering::Relaxed)).unwrap_or(0);
            if let Some(l) = self.lbl_cursor.borrow().clone() {
                l.set_text(&format!("{x},  {y}"));
            }
        }

        fn read_config_from_ui(&self, state: &SharedState) -> crate::config::ClickConfig {
            let base = state.lock().unwrap().config.clone();

            let sh = self.spin_hours.borrow().as_ref().map_or(0, |s| s.value() as u64);
            let sm = self.spin_minutes.borrow().as_ref().map_or(0, |s| s.value() as u64);
            let ss = self.spin_seconds.borrow().as_ref().map_or(1, |s| s.value() as u64);
            let sms = self.spin_ms.borrow().as_ref().map_or(0, |s| s.value() as u64);
            let interval_ms = (sh * 3_600_000 + sm * 60_000 + ss * 1_000 + sms).max(10);

            let button = match self.btn_dropdown.borrow().as_ref().map_or(0, |d| d.selected()) {
                0 => MouseButton::Left,
                1 => MouseButton::Right,
                2 => MouseButton::Middle,
                _ => MouseButton::Double,
            };

            let fixed_mode = self.chk_fixed.borrow().as_ref().map_or(false, |c| c.is_active());
            let position_mode = if fixed_mode { PositionMode::Fixed } else { PositionMode::FollowCursor };

            let fixed_x = self.entry_x.borrow().as_ref()
                .and_then(|e| e.text().parse().ok()).unwrap_or(base.fixed_x);
            let fixed_y = self.entry_y.borrow().as_ref()
                .and_then(|e| e.text().parse().ok()).unwrap_or(base.fixed_y);

            let hotkey_code = self.hotkey_dropdown.borrow().as_ref()
                .and_then(|d| HOTKEY_OPTIONS.get(d.selected() as usize))
                .map(|(c, _)| *c)
                .unwrap_or(base.hotkey_code);

            let capture_hotkey_code = self.capture_hk_dropdown.borrow().as_ref()
                .and_then(|d| HOTKEY_OPTIONS.get(d.selected() as usize))
                .map(|(c, _)| *c)
                .unwrap_or(base.capture_hotkey_code);

            let limit_active = self.chk_limit.borrow().as_ref().map_or(false, |c| c.is_active());
            let click_limit = if limit_active {
                self.spin_limit.borrow().as_ref().map(|s| s.value() as u64)
            } else {
                None
            };

            crate::config::ClickConfig {
                interval_ms,
                button,
                position_mode,
                fixed_x,
                fixed_y,
                click_limit,
                hotkey_code,
                capture_hotkey_code,
            }
        }

        fn show_error_dialog(&self, message: &str) {
            let dialog = gtk4::AlertDialog::builder()
                .message("Permission Error")
                .detail(message)
                .build();
            dialog.show(Some(&*self.obj()));
        }
    }
}

// ── Public GObject wrapper ─────────────────────────────────────────────────────

glib::wrapper! {
    pub struct MainWindow(ObjectSubclass<imp::MainWindow>)
        @extends gtk4::ApplicationWindow, gtk4::Window, gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget,
                    gtk4::Native, gtk4::Root, gtk4::ShortcutManager;
}

impl MainWindow {
    pub fn new(app: &gtk4::Application) -> Self {
        glib::Object::builder().property("application", app).build()
    }

    pub fn setup(&self, state: SharedState) {
        self.imp().setup(state);
    }
}

// ── Helper ─────────────────────────────────────────────────────────────────────

fn make_spin(min: f64, max: f64, val: f64, digits: u32) -> gtk4::SpinButton {
    let spin = gtk4::SpinButton::with_range(min, max, 1.0);
    spin.set_value(val);
    spin.set_digits(digits);
    spin.set_width_chars((digits + 1) as i32);
    spin.set_snap_to_ticks(true);
    spin.set_numeric(true);
    spin
}
