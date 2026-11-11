//! The settings window.

use crate::protocol::*;
use crate::worker::{self, Event, Link, Request, Worker};
use crate::Options;
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Margin, RichText, Stroke, StrokeKind, Vec2};
use std::time::Instant;

const BG: Color32 = Color32::from_rgb(0x12, 0x12, 0x15);
const PANEL: Color32 = Color32::from_rgb(0x1B, 0x1B, 0x20);
const CARD: Color32 = Color32::from_rgb(0x23, 0x23, 0x2A);
const LINE: Color32 = Color32::from_rgb(0x34, 0x34, 0x3D);
const TEXT: Color32 = Color32::from_rgb(0xE8, 0xE8, 0xEC);
const DIM: Color32 = Color32::from_rgb(0x9A, 0x9A, 0xA6);
const ACCENT: Color32 = Color32::from_rgb(0xF2, 0xB8, 0x4B);
const GOOD: Color32 = Color32::from_rgb(0x5C, 0xD6, 0x8B);
const WARN: Color32 = Color32::from_rgb(0xF0, 0x8C, 0x3C);

const RULE_FILE: &str = "/etc/udev/rules.d/70-am-infinity-mouse.rules";
const RULE_TEXT: &str = "# AngryMiao AM Infinity mouse: let the signed-in user change its settings\n\
KERNEL==\"hidraw*\", ATTRS{idVendor}==\"0e8d\", ATTRS{idProduct}==\"0703\", MODE=\"0660\", GROUP=\"plugdev\", TAG+=\"uaccess\"\n\
KERNEL==\"hidraw*\", ATTRS{idVendor}==\"0e8d\", ATTRS{idProduct}==\"0880\", MODE=\"0660\", GROUP=\"plugdev\", TAG+=\"uaccess\"\n";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Resolution,
    Buttons,
    Performance,
    About,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pick {
    Mouse,
    Dpi,
    Media,
    Keyboard,
}

pub struct App {
    opts: Options,
    worker: Worker,
    link: Link,
    busy: bool,
    page: Page,
    dpi: Dpi,
    sensor: Sensor,
    buttons: [Option<Action>; 7],
    have_dpi: bool,
    have_buttons: bool,
    /// A full read from the mouse succeeded, so every value on screen is real.
    have_sensor: bool,
    version: Option<String>,
    log: Vec<(Instant, String, bool)>,
    selected_button: usize,
    pick: Pick,
    key_usage: u8,
    key_mods: u8,
    confirm_reset: bool,
    access_running: bool,
    access_rx: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    frame_no: u64,
    shot_requested: bool,
    sync_cache: Option<bool>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, opts: Options) -> App {
        style(&cc.egui_ctx);
        let ctx = cc.egui_ctx.clone();
        let demo = opts.demo;
        let worker = worker::start(demo, move || ctx.request_repaint());
        let start_page = match opts.page.as_deref() {
            Some("buttons") => Page::Buttons,
            Some("performance") => Page::Performance,
            Some("help") => Page::About,
            _ => Page::Resolution,
        };
        App {
            opts,
            worker,
            link: Link::Searching,
            busy: false,
            page: start_page,
            dpi: Dpi::default(),
            sensor: Sensor::default(),
            buttons: DEFAULT_ACTIONS.map(Some),
            have_dpi: false,
            have_buttons: false,
            have_sensor: false,
            version: None,
            log: vec![],
            selected_button: 3,
            pick: Pick::Mouse,
            key_usage: 0x04,
            key_mods: 0,
            confirm_reset: false,
            access_running: false,
            access_rx: None,
            frame_no: 0,
            shot_requested: false,
            sync_cache: None,
        }
    }

    fn send(&self, r: Request) {
        let _ = self.worker.tx.send(r);
    }

    /// Settings can be changed only once they were really read from the mouse.
    fn ready(&self) -> bool {
        self.connected() && self.have_dpi && self.have_sensor
    }

    fn connected(&self) -> bool {
        matches!(self.link, Link::Connected { .. } | Link::Demo)
    }

    fn say(&mut self, text: impl Into<String>, ok: bool) {
        self.log.push((Instant::now(), text.into(), ok));
        if self.log.len() > 50 {
            self.log.remove(0);
        }
    }

    fn pump(&mut self) {
        while let Ok(ev) = self.worker.rx.try_recv() {
            match ev {
                Event::Link(l) => {
                    if l != self.link {
                        self.have_dpi = false;
                        self.have_sensor = false;
                        self.have_buttons = false;
                    }
                    self.link = l;
                }
                Event::Dpi(d) => {
                    self.dpi = d;
                    self.have_dpi = true;
                }
                Event::Sensor(s) => {
                    self.sensor = s;
                    self.have_sensor = true;
                }
                Event::Buttons(b) => {
                    for (slot, v) in self.buttons.iter_mut().zip(b) {
                        if v.is_some() {
                            *slot = v;
                        }
                    }
                    self.have_buttons = true;
                }
                Event::Version(v) => self.version = Some(v),
                Event::Saved(m) => {
                    self.say(m, true)
                }
                Event::Failed(m) => self.say(m, false),
                Event::Busy(b) => self.busy = b,
            }
        }
        if let Some(rx) = &self.access_rx {
            if let Ok(res) = rx.try_recv() {
                self.access_running = false;
                self.access_rx = None;
                match res {
                    Ok(()) => {
                        self.say("Access allowed. Connecting to the mouse…", true);
                        self.send(Request::Connect);
                    }
                    Err(e) => self.say(e, false),
                }
            }
        }
    }

    fn allow_access(&mut self) {
        self.access_running = true;
        let (tx, rx) = std::sync::mpsc::channel();
        self.access_rx = Some(rx);
        let script = format!(
            "cat > {RULE_FILE} <<'EOF'\n{RULE_TEXT}EOF\nudevadm control --reload-rules && udevadm trigger --action=change --subsystem-match=hidraw && sleep 1"
        );
        std::thread::spawn(move || {
            let res = std::process::Command::new("pkexec").args(["sh", "-c", &script]).status();
            let _ = tx.send(match res {
                Ok(s) if s.success() => Ok(()),
                Ok(_) => Err("Access wasn't changed (the password window was closed or refused).".into()),
                Err(e) => Err(format!("Couldn't ask for your password ({e}). Use the command shown instead.")),
            });
        });
    }

    // ---- changes made from the window ----

    fn apply_dpi_level(&mut self, i: usize) {
        if self.dpi_synced() {
            self.dpi.y[i] = self.dpi.x[i];
        }
        self.send(Request::SetDpiLevel { level: i, x: self.dpi.x[i], y: self.dpi.y[i] });
    }

    fn dpi_synced(&self) -> bool {
        self.sync_cache.unwrap_or(true)
    }

    fn set_button(&mut self, i: usize, action: Action) {
        self.buttons[i] = Some(action);
        self.send(Request::SetButton { button: i as u8 + 1, action });
    }

    fn set_rate(&mut self, hz: u16) {
        let step = debounce_step(self.sensor.rate, self.sensor.debounce[0]);
        self.sensor.rate = hz;
        self.sensor.debounce = debounce_triple(hz, step);
        self.send(Request::SetRate(hz));
        self.send(Request::SetDebounce(self.sensor.debounce));
    }
}

impl App {
    /// Same-X-and-Y is a view choice (the mouse only stores X and Y), remembered in egui's memory.
    fn sync_state(ctx: &egui::Context) -> bool {
        ctx.data(|d| d.get_temp::<bool>(egui::Id::new("dpi-sync"))).unwrap_or(true)
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.frame_no += 1;
        self.pump();
        self.sync_cache = Some(App::sync_state(&ctx));

        egui::Panel::top("top")
            .frame(egui::Frame::new().fill(PANEL).inner_margin(Margin::symmetric(18, 12)))
            .show(ui, |ui| self.header(ui));
        egui::Panel::bottom("status")
            .frame(egui::Frame::new().fill(PANEL).inner_margin(Margin::symmetric(18, 8)))
            .show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("nav")
            .resizable(false)
            .exact_size(210.0)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(Margin::symmetric(12, 16)))
            .show(ui, |ui| self.nav(ui));
        egui::CentralPanel::default_margins()
            .frame(egui::Frame::new().fill(BG).inner_margin(Margin::symmetric(26, 18)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    self.banner(ui);
                    match self.page {
                        Page::Resolution => self.page_resolution(ui),
                        Page::Buttons => self.page_buttons(ui),
                        Page::Performance => self.page_performance(ui),
                        Page::About => self.page_about(ui),
                    }
                });
            });

        self.screenshot(&ctx);
        if self.opts.screenshot.is_some() || self.busy || self.access_running {
            ctx.request_repaint();
        }
    }
}

impl App {
    // ---------------- header, nav, status ----------------

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            mouse_glyph(ui, 26.0);
            ui.vertical(|ui| {
                ui.label(RichText::new("Infinity Mouse Settings").size(20.0).strong().color(TEXT));
                let (text, colour) = match &self.link {
                    Link::Searching => ("Looking for the mouse…".to_string(), DIM),
                    Link::NotFound => ("Mouse not found — plug it in with its cable".to_string(), WARN),
                    Link::NoPermission { product, .. } => (format!("{product} found — Ubuntu needs your OK to change it"), WARN),
                    Link::Connected { product, .. } => (format!("Connected: {product}"), GOOD),
                    Link::Demo => ("Demo mode: a pretend mouse, nothing is sent to hardware".to_string(), ACCENT),
                };
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover());
                    ui.painter().circle_filled(r.center(), 4.5, colour);
                    ui.label(RichText::new(text).color(colour));
                    if let Some(v) = &self.version {
                        ui.label(RichText::new(format!("  firmware {v}")).color(DIM));
                    }
                    if self.busy {
                        ui.add(egui::Spinner::new().size(12.0));
                    }
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(self.connected(), egui::Button::new("Read from mouse")).clicked() {
                    self.send(Request::ReadAll);
                }
                if ui.button("Search again").clicked() {
                    self.send(Request::Connect);
                }
            });
        });
    }

    fn nav(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 6.0;
        for (page, title, sub) in [
            (Page::Resolution, "Resolution", "DPI levels"),
            (Page::Buttons, "Buttons", "What each button does"),
            (Page::Performance, "Performance", "Polling, debounce, sensor"),
            (Page::About, "Help", "Access and notes"),
        ] {
            let on = self.page == page;
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 52.0), egui::Sense::click());
            let fill = if on { CARD } else if resp.hovered() { Color32::from_rgb(0x20, 0x20, 0x26) } else { PANEL };
            ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
            if on {
                ui.painter().rect_filled(egui::Rect::from_min_size(rect.min, Vec2::new(3.0, rect.height())), CornerRadius::same(2), ACCENT);
            }
            ui.painter().text(rect.min + Vec2::new(14.0, 10.0), Align2::LEFT_TOP, title, FontId::proportional(15.5), if on { TEXT } else { DIM });
            ui.painter().text(rect.min + Vec2::new(14.0, 30.0), Align2::LEFT_TOP, sub, FontId::proportional(11.5), DIM);
            if resp.clicked() {
                self.page = page;
            }
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            if self.confirm_reset {
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.confirm_reset = false;
                    }
                    if ui.button(RichText::new("Yes, reset").color(WARN)).clicked() {
                        self.confirm_reset = false;
                        self.send(Request::FactoryReset);
                    }
                });
                ui.label(RichText::new("Put every setting back to how it left the factory?").color(DIM).size(12.0));
            } else if ui.add_enabled(self.connected(), egui::Button::new("Factory reset…")).clicked() {
                self.confirm_reset = true;
            }
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| match self.log.last() {
            Some((t, m, ok)) => {
                ui.label(RichText::new(if *ok { "✔" } else { "⚠" }).color(if *ok { GOOD } else { WARN }));
                ui.label(RichText::new(m).color(TEXT));
                ui.label(RichText::new(format!("{}s ago", t.elapsed().as_secs())).color(DIM).size(11.0));
            }
            None => {
                ui.label(RichText::new("Changes are saved on the mouse as soon as you make them.").color(DIM));
            }
        });
    }

    fn banner(&mut self, ui: &mut egui::Ui) {
        let link = self.link.clone();
        match link {
            Link::NoPermission { path, .. } => {
                card(ui, WARN, |ui| {
                    ui.label(RichText::new("One-time step: let Ubuntu change the mouse's settings").size(16.0).strong().color(TEXT));
                    ui.label(RichText::new(format!(
                        "The mouse is plugged in ({path}), but Ubuntu only lets administrators send it settings. \
                         Allowing access adds a small rule so your account can, now and after restarts. \
                         You'll be asked for your password once."
                    )).color(DIM));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let label = if self.access_running { "Waiting for your password…" } else { "Allow access" };
                        if ui.add_enabled(!self.access_running, egui::Button::new(RichText::new(label).strong()).fill(ACCENT.gamma_multiply(0.35))).clicked() {
                            self.allow_access();
                        }
                        if ui.button("Try the demo instead").clicked() {
                            self.send(Request::UseDemo);
                        }
                    });
                    ui.collapsing("Or do it yourself in a terminal", |ui| {
                        let cmd = format!(
                            "sudo tee {RULE_FILE} >/dev/null <<'EOF'\n{RULE_TEXT}EOF\nsudo udevadm control --reload-rules && sudo udevadm trigger --action=change --subsystem-match=hidraw"
                        );
                        let mut c = cmd.clone();
                        ui.add(egui::TextEdit::multiline(&mut c).code_editor().desired_width(f32::INFINITY));
                        if ui.button("Copy").clicked() {
                            ui.ctx().copy_text(cmd);
                        }
                    });
                });
                ui.add_space(12.0);
            }
            Link::NotFound => {
                card(ui, WARN, |ui| {
                    ui.label(RichText::new("No AM Infinity mouse found").size(16.0).strong().color(TEXT));
                    ui.label(RichText::new("Plug the mouse in with its USB cable, then press Search again. The values below are samples until it connects.").color(DIM));
                    ui.horizontal(|ui| {
                        if ui.button("Search again").clicked() {
                            self.send(Request::Connect);
                        }
                        if ui.button("Try the demo").clicked() {
                            self.send(Request::UseDemo);
                        }
                    });
                });
                ui.add_space(12.0);
            }
            _ => {}
        }
    }

    // ---------------- Resolution ----------------

    fn page_resolution(&mut self, ui: &mut egui::Ui) {
        let live = self.ready();
        title(ui, "Resolution", "How far the pointer moves for each inch the mouse moves. The DPI button steps through these levels.");
        let mut synced = App::sync_state(ui.ctx());
        card(ui, LINE, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Levels in the cycle").color(TEXT));
                let mut n = self.dpi.levels as u32;
                let r = ui.add_enabled(live, egui::Slider::new(&mut n, 1..=DPI_LEVELS_MAX as u32).show_value(true));
                if r.changed() {
                    self.dpi.levels = n as usize;
                    self.dpi.current = self.dpi.current.min(self.dpi.levels - 1);
                }
                if r.drag_stopped() || (r.changed() && !r.dragged()) {
                    self.send(Request::SetDpiLevels(self.dpi.levels));
                }
                ui.add_space(24.0);
                if ui.checkbox(&mut synced, "Same speed across and up-down (X = Y)").changed() {
                    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("dpi-sync"), synced));
                    self.sync_cache = Some(synced);
                    if synced && live {
                        for i in 0..self.dpi.levels {
                            if self.dpi.y[i] != self.dpi.x[i] {
                                self.dpi.y[i] = self.dpi.x[i];
                                self.apply_dpi_level(i);
                            }
                        }
                    }
                }
            });
        });
        ui.add_space(10.0);
        for i in 0..self.dpi.levels {
            let active = self.dpi.current == i;
            let [r, g, b] = DPI_COLOURS[i];
            let colour = Color32::from_rgb(r, g, b);
            card(ui, if active { ACCENT } else { LINE }, |ui| {
                ui.horizontal(|ui| {
                    let (dot, _) = ui.allocate_exact_size(Vec2::splat(18.0), egui::Sense::hover());
                    ui.painter().circle_filled(dot.center(), 7.0, colour);
                    ui.label(RichText::new(format!("Level {}", i + 1)).size(15.0).strong().color(TEXT));
                    ui.add_space(8.0);
                    let lbl = if active { RichText::new("In use").color(ACCENT) } else { RichText::new("Use this level").color(DIM) };
                    if ui.add_enabled(live, egui::Button::selectable(active, lbl)).clicked() && !active {
                        self.dpi.current = i;
                        self.send(Request::SetDpiCurrent(i));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let text = if synced || self.dpi.x[i] == self.dpi.y[i] {
                            format!("{} DPI", self.dpi.x[i])
                        } else {
                            format!("{} × {} DPI", self.dpi.x[i], self.dpi.y[i])
                        };
                        ui.label(RichText::new(text).size(18.0).strong().color(TEXT));
                    });
                });
                let axes: &[(usize, &str)] = if synced { &[(0, "DPI")] } else { &[(0, "X"), (1, "Y")] };
                for &(axis, name) in axes {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(name).color(DIM));
                        let mut v = if axis == 0 { self.dpi.x[i] } else { self.dpi.y[i] } as f64;
                        ui.spacing_mut().slider_width = (ui.available_width() - 120.0).max(200.0);
                        let s = ui.add_enabled(
                            live,
                            egui::Slider::new(&mut v, DPI_MIN as f64..=DPI_MAX as f64).step_by(DPI_STEP as f64).show_value(false),
                        );
                        let d = ui.add_enabled(live, egui::DragValue::new(&mut v).range(DPI_MIN..=DPI_MAX).speed(DPI_STEP as f64));
                        let nv = snap_dpi(v);
                        if axis == 0 {
                            self.dpi.x[i] = nv;
                        } else {
                            self.dpi.y[i] = nv;
                        }
                        let commit = s.drag_stopped() || d.drag_stopped() || d.lost_focus()
                            || ((s.changed() && !s.dragged()) || (d.changed() && !d.dragged() && !d.has_focus()));
                        if commit {
                            self.apply_dpi_level(i);
                        }
                    });
                }
            });
            ui.add_space(6.0);
        }
        ui.add_space(8.0);
        card(ui, LINE, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Quick DPI switch").color(TEXT).strong());
                    ui.label(RichText::new("Hold both side buttons and turn the wheel to step DPI up or down.").color(DIM).size(12.0));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut on = self.sensor.dpi_switch;
                    if on_off(ui, live, &mut on) {
                        self.sensor.dpi_switch = on;
                        self.send(Request::SetDpiSwitch(on));
                    }
                });
            });
        });
    }

    // ---------------- Buttons ----------------

    fn page_buttons(&mut self, ui: &mut egui::Ui) {
        let live = self.ready();
        title(ui, "Buttons", "Pick a button, then choose what it should do. The mouse remembers it, on any computer.");
        ui.horizontal_top(|ui| {
            // Drawing of the mouse with numbered buttons.
            let (rect, _) = ui.allocate_exact_size(Vec2::new(260.0, 380.0), egui::Sense::hover());
            if let Some(hit) = draw_mouse(ui, rect, self.selected_button) {
                self.selected_button = hit;
            }
            ui.add_space(16.0);
            ui.vertical(|ui| {
                for i in 0..7 {
                    let sel = self.selected_button == i;
                    let current = self.buttons[i].map(|a| a.label()).unwrap_or_else(|| "(not read)".into());
                    let changed = self.buttons[i] != Some(DEFAULT_ACTIONS[i]);
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width().min(560.0), 40.0), egui::Sense::click());
                    let p = ui.painter();
                    p.rect_filled(rect, CornerRadius::same(8), if sel { CARD } else { PANEL });
                    p.rect_stroke(rect, CornerRadius::same(8), Stroke::new(1.0, if sel { ACCENT } else { LINE }), StrokeKind::Inside);
                    p.circle_filled(rect.left_center() + Vec2::new(20.0, 0.0), 11.0, if sel { ACCENT } else { LINE });
                    p.text(rect.left_center() + Vec2::new(20.0, 0.0), Align2::CENTER_CENTER, format!("{}", i + 1), FontId::proportional(13.0), if sel { BG } else { TEXT });
                    p.text(rect.left_center() + Vec2::new(42.0, 0.0), Align2::LEFT_CENTER, BUTTON_NAMES[i], FontId::proportional(14.5), TEXT);
                    p.text(rect.right_center() - Vec2::new(14.0, 0.0), Align2::RIGHT_CENTER, &current, FontId::proportional(14.5), if changed { ACCENT } else { DIM });
                    if resp.clicked() {
                        self.selected_button = i;
                    }
                    ui.add_space(4.0);
                }
                ui.add_space(10.0);
                self.button_editor(ui, live);
            });
        });
    }

    fn button_editor(&mut self, ui: &mut egui::Ui, live: bool) {
        let i = self.selected_button;
        card(ui, LINE, |ui| {
            ui.set_max_width(560.0);
            ui.label(RichText::new(format!("{} ({}) does…", BUTTON_NAMES[i], i + 1)).size(15.0).strong().color(TEXT));
            ui.horizontal(|ui| {
                for (p, name) in [(Pick::Mouse, "Mouse"), (Pick::Dpi, "DPI & profile"), (Pick::Media, "Media & apps"), (Pick::Keyboard, "Keyboard key")] {
                    ui.selectable_value(&mut self.pick, p, name);
                }
            });
            ui.separator();
            let current = self.buttons[i];
            let mut chosen: Option<Action> = None;
            ui.add_enabled_ui(live, |ui| match self.pick {
                Pick::Mouse => {
                    ui.horizontal_wrapped(|ui| {
                        for a in [Action::Left, Action::Right, Action::Middle, Action::Forward, Action::Back, Action::WheelUp, Action::WheelDown] {
                            if ui.selectable_label(current == Some(a), a.label()).clicked() {
                                chosen = Some(a);
                            }
                        }
                    });
                }
                Pick::Dpi => {
                    ui.horizontal_wrapped(|ui| {
                        for a in [Action::DpiPlus, Action::DpiMinus, Action::DpiCycle, Action::ProfileSwitch] {
                            if ui.selectable_label(current == Some(a), a.label()).clicked() {
                                chosen = Some(a);
                            }
                        }
                    });
                }
                Pick::Media => {
                    ui.horizontal_wrapped(|ui| {
                        for (code, _) in MEDIA {
                            let a = Action::Media(code);
                            if ui.selectable_label(current == Some(a), a.label()).clicked() {
                                chosen = Some(a);
                            }
                        }
                    });
                }
                Pick::Keyboard => {
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("key-pick")
                            .selected_text(key_name(self.key_usage))
                            .height(320.0)
                            .show_ui(ui, |ui| {
                                for (u, n) in KEYS {
                                    ui.selectable_value(&mut self.key_usage, *u, *n);
                                }
                            });
                        for (bit, n) in [(MOD_CTRL, "Ctrl"), (MOD_SHIFT, "Shift"), (MOD_ALT, "Alt"), (MOD_SUPER, "Super")] {
                            let mut on = self.key_mods & bit != 0;
                            if ui.checkbox(&mut on, n).changed() {
                                self.key_mods = if on { self.key_mods | bit } else { self.key_mods & !bit };
                            }
                        }
                    });
                    let a = Action::Key { usage: self.key_usage, mods: self.key_mods };
                    if ui.button(format!("Use {}", a.label())).clicked() {
                        chosen = Some(a);
                    }
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(live && current != Some(DEFAULT_ACTIONS[i]), egui::Button::new("Back to default")).clicked() {
                    chosen = Some(DEFAULT_ACTIONS[i]);
                }
                if current == Some(Action::Macro) {
                    ui.label(RichText::new("This button runs a macro made in AM Master; macros can't be edited here yet.").color(DIM).size(12.0));
                }
            });
            if i == 0 && matches!(chosen, Some(a) if a != Action::Left) && !self.buttons.iter().skip(1).any(|b| *b == Some(Action::Left)) {
                ui.label(RichText::new("Tip: keep left click on some button so you can still click.").color(WARN));
            }
            if let Some(a) = chosen {
                self.set_button(i, a);
            }
        });
    }

    // ---------------- Performance ----------------

    fn page_performance(&mut self, ui: &mut egui::Ui) {
        let live = self.ready();
        title(ui, "Performance", "How often the mouse reports, how it reads clicks, and how the sensor behaves.");

        setting_row(ui, "Polling rate", "Reports per second. Higher is smoother but uses more computer time.", |ui| {
            for hz in POLLING_RATES.iter().rev() {
                if ui.add_enabled(live, egui::Button::selectable(self.sensor.rate == *hz, format!("{hz}"))).clicked() && self.sensor.rate != *hz {
                    self.set_rate(*hz);
                }
            }
            ui.label(RichText::new("Hz").color(DIM));
        });

        let step = debounce_step(self.sensor.rate, self.sensor.debounce[0]);
        setting_row(ui, "Click debounce", "Faster reacts sooner; slower stops accidental double clicks.", |ui| {
            for (s, ms) in DEBOUNCE_LABELS_MS.iter().enumerate().rev() {
                if ui.add_enabled(live, egui::Button::selectable(step == s, format!("{ms} ms"))).clicked() && step != s {
                    self.sensor.debounce = debounce_triple(self.sensor.rate, s);
                    self.send(Request::SetDebounce(self.sensor.debounce));
                }
            }
        });

        setting_row(ui, "Lift-off distance", "How high you can lift the mouse before it stops tracking.", |ui| {
            for (v, n) in [(2u8, "High"), (1, "Middle"), (0, "Low")] {
                if ui.add_enabled(live, egui::Button::selectable(self.sensor.lod == v, n)).clicked() && self.sensor.lod != v {
                    self.sensor.lod = v;
                    self.send(Request::SetLod(v));
                }
            }
        });

        let toggles: [(&str, &str, u8); 4] = [
            ("Motion sync", "Lines sensor readings up with the polling rate for steadier tracking.", 0),
            ("Angle snapping", "Straightens slightly wobbly lines. Most gamers keep it off.", 1),
            ("Ripple control", "Smooths jitter at very high DPI.", 2),
            ("Game (FPS) mode", "AngryMiao's tuning for fast shooters.", 3),
        ];
        for (name, help, which) in toggles {
            let mut on = match which {
                0 => self.sensor.motion_sync,
                1 => self.sensor.angle_snap,
                2 => self.sensor.ripple,
                _ => self.sensor.fps_mode,
            };
            let mut changed = false;
            setting_row(ui, name, help, |ui| changed = on_off(ui, live, &mut on));
            if changed {
                match which {
                    0 => {
                        self.sensor.motion_sync = on;
                        self.send(Request::SetMotionSync(on))
                    }
                    1 => {
                        self.sensor.angle_snap = on;
                        self.send(Request::SetAngleSnap(on))
                    }
                    2 => {
                        self.sensor.ripple = on;
                        self.send(Request::SetRipple(on))
                    }
                    _ => {
                        self.sensor.fps_mode = on;
                        self.send(Request::SetFpsMode(on))
                    }
                }
            }
        }
    }

    fn page_about(&mut self, ui: &mut egui::Ui) {
        title(ui, "Help", "About this app and the mouse connection.");
        card(ui, LINE, |ui| {
            ui.label(RichText::new("How it works").strong().color(TEXT));
            ui.label(RichText::new("This app sends the same settings commands as AngryMiao's Windows app, over the mouse's USB settings channel. Settings are stored on the mouse itself, so they also apply on Windows and other computers.").color(DIM));
            ui.add_space(8.0);
            ui.label(RichText::new("Access").strong().color(TEXT));
            ui.label(RichText::new(format!("Ubuntu needs a one-time rule so your account may talk to the mouse. The \"Allow access\" button writes it to {RULE_FILE}. Delete that file to undo it.")).color(DIM));
            ui.add_space(8.0);
            ui.label(RichText::new("Unofficial").strong().color(TEXT));
            ui.label(RichText::new("This is an independent community project. It is not made, endorsed or supported by AngryMiao. AngryMiao and AM Infinity are trademarks of their owner.").color(DIM));
            ui.add_space(8.0);
            ui.label(RichText::new("Not in this version").strong().color(TEXT));
            ui.label(RichText::new("Lighting colours, macro editing, wireless sleep timers and firmware updates.").color(DIM));
            if let Link::Connected { path, .. } = &self.link {
                ui.add_space(8.0);
                ui.label(RichText::new(format!("Settings channel: {path}")).color(DIM).size(12.0));
            }
        });
    }

    // ---------------- screenshot ----------------

    fn screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.opts.screenshot.clone() else { return };
        if self.frame_no >= 120 && !self.shot_requested {
            self.shot_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let shot = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = shot {
            if let Err(e) = save_png(&path, &img) {
                eprintln!("screenshot failed: {e}");
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.frame_no > 2000 {
            eprintln!("screenshot never arrived");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

// ---------------- small drawing helpers ----------------

fn style(ctx: &egui::Context) {
    ctx.set_visuals(egui::Visuals::dark());
    ctx.all_styles_mut(|s| {
        s.visuals.panel_fill = BG;
        s.visuals.window_fill = PANEL;
        s.visuals.extreme_bg_color = Color32::from_rgb(0x0E, 0x0E, 0x11);
        s.visuals.selection.bg_fill = ACCENT.gamma_multiply(0.45);
        s.visuals.selection.stroke = Stroke::new(1.0, ACCENT);
        s.visuals.widgets.inactive.weak_bg_fill = CARD;
        s.visuals.widgets.inactive.bg_fill = Color32::from_rgb(0x40, 0x40, 0x4A);
        s.visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(0x3A, 0x3A, 0x43);
        s.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x2E, 0x2E, 0x37);
        s.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(0x38, 0x38, 0x44);
        s.visuals.override_text_color = Some(TEXT);
        s.visuals.slider_trailing_fill = true;
        s.spacing.item_spacing = Vec2::new(8.0, 7.0);
        s.spacing.button_padding = Vec2::new(10.0, 5.0);
        s.text_styles.insert(egui::TextStyle::Body, FontId::proportional(14.0));
        s.text_styles.insert(egui::TextStyle::Button, FontId::proportional(14.0));
    });
}

fn title(ui: &mut egui::Ui, t: &str, sub: &str) {
    ui.label(RichText::new(t).size(24.0).strong().color(TEXT));
    ui.label(RichText::new(sub).color(DIM));
    ui.add_space(12.0);
}

fn card<R>(ui: &mut egui::Ui, edge: Color32, f: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, edge))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            f(ui)
        })
        .inner
}

fn setting_row(ui: &mut egui::Ui, name: &str, help: &str, f: impl FnOnce(&mut egui::Ui)) {
    card(ui, LINE, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(300.0);
                ui.label(RichText::new(name).strong().color(TEXT));
                ui.label(RichText::new(help).color(DIM).size(12.0));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| f(ui));
        });
    });
    ui.add_space(6.0);
}

/// On / Off pair; returns true when changed.
fn on_off(ui: &mut egui::Ui, enabled: bool, on: &mut bool) -> bool {
    let mut changed = false;
    if ui.add_enabled(enabled, egui::Button::selectable(!*on, "Off")).clicked() && *on {
        *on = false;
        changed = true;
    }
    if ui.add_enabled(enabled, egui::Button::selectable(*on, "On")).clicked() && !*on {
        *on = true;
        changed = true;
    }
    changed
}

fn mouse_glyph(ui: &mut egui::Ui, h: f32) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(h * 0.7, h), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(r, CornerRadius::same((h * 0.35) as u8), CARD);
    p.rect_stroke(r, CornerRadius::same((h * 0.35) as u8), Stroke::new(1.5, ACCENT), StrokeKind::Inside);
    p.line_segment([r.center_top() + Vec2::new(0.0, 3.0), r.center_top() + Vec2::new(0.0, h * 0.35)], Stroke::new(1.5, ACCENT));
}

/// A top-down mouse with its seven buttons; returns a clicked button.
fn draw_mouse(ui: &mut egui::Ui, rect: egui::Rect, selected: usize) -> Option<usize> {
    let p = ui.painter_at(rect);
    let body = egui::Rect::from_center_size(rect.center() + Vec2::new(10.0, 10.0), Vec2::new(170.0, 320.0));
    p.rect_filled(body, CornerRadius { nw: 80, ne: 80, sw: 70, se: 70 }, Color32::from_rgb(0x2A, 0x2A, 0x31));
    p.rect_stroke(body, CornerRadius { nw: 80, ne: 80, sw: 70, se: 70 }, Stroke::new(1.5, LINE), StrokeKind::Inside);
    let split_y = body.top() + 130.0;
    p.line_segment([egui::pos2(body.left() + 6.0, split_y), egui::pos2(body.right() - 6.0, split_y)], Stroke::new(1.0, LINE));
    p.line_segment([egui::pos2(body.center().x, body.top() + 4.0), egui::pos2(body.center().x, split_y)], Stroke::new(1.0, LINE));
    // Areas of each button (index → rect).
    let left = egui::Rect::from_min_max(body.min + Vec2::new(8.0, 20.0), egui::pos2(body.center().x - 18.0, split_y - 6.0));
    let right = egui::Rect::from_min_max(egui::pos2(body.center().x + 18.0, body.top() + 20.0), egui::pos2(body.right() - 8.0, split_y - 6.0));
    let wheel = egui::Rect::from_center_size(egui::pos2(body.center().x, body.top() + 70.0), Vec2::new(22.0, 54.0));
    let wheel_up = egui::Rect::from_center_size(egui::pos2(body.center().x, body.top() + 30.0), Vec2::new(22.0, 22.0));
    let wheel_down = egui::Rect::from_center_size(egui::pos2(body.center().x, body.top() + 110.0), Vec2::new(22.0, 22.0));
    let fwd = egui::Rect::from_min_size(egui::pos2(body.left() - 14.0, split_y + 22.0), Vec2::new(16.0, 46.0));
    let back = egui::Rect::from_min_size(egui::pos2(body.left() - 14.0, split_y + 74.0), Vec2::new(16.0, 46.0));
    let areas = [left, right, wheel, fwd, back, wheel_up, wheel_down];
    let mut hit = None;
    for (i, a) in areas.iter().enumerate() {
        let resp = ui.interact(*a, ui.id().with(("mouse-btn", i)), egui::Sense::click());
        let sel = i == selected;
        let fill = if sel {
            ACCENT.gamma_multiply(0.55)
        } else if resp.hovered() {
            Color32::from_rgb(0x3A, 0x3A, 0x44)
        } else {
            Color32::from_rgb(0x30, 0x30, 0x38)
        };
        let radius = if i == 2 || i >= 5 { 10 } else { 6 };
        if i != 5 && i != 6 {
            p.rect_filled(*a, CornerRadius::same(radius), fill);
        } else {
            let c = a.center();
            let d = if i == 5 { -1.0 } else { 1.0 };
            let pts = vec![c + Vec2::new(-6.0, -4.0 * d), c + Vec2::new(6.0, -4.0 * d), c + Vec2::new(0.0, 5.0 * d)];
            p.add(egui::Shape::convex_polygon(pts, if sel { ACCENT } else { DIM }, Stroke::NONE));
        }
        let tag = match i {
            3 | 4 => a.left_center() - Vec2::new(14.0, 0.0),
            5 | 6 => a.right_center() + Vec2::new(14.0, 0.0),
            2 => a.center(),
            _ => a.center() + Vec2::new(0.0, 10.0),
        };
        p.circle_filled(tag, 10.0, if sel { ACCENT } else { BG });
        p.text(tag, Align2::CENTER_CENTER, format!("{}", i + 1), FontId::proportional(12.0), if sel { BG } else { TEXT });
        if resp.clicked() {
            hit = Some(i);
        }
    }
    hit
}

fn save_png(path: &str, img: &egui::ColorImage) -> Result<(), String> {
    if let Some(dir) = std::path::Path::new(path).parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
    }
    let [w, h] = img.size;
    let mut data = Vec::with_capacity(w * h * 4);
    for c in &img.pixels {
        data.extend_from_slice(&c.to_srgba_unmultiplied());
    }
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(&data).map_err(|e| e.to_string())?;
    Ok(())
}
