//! A background thread that owns the connection, so the window never freezes while the
//! mouse thinks.

use crate::device::{self, Found, Hidraw, SimMouse, Transport};
use crate::protocol::*;
use std::sync::mpsc::{channel, Receiver, Sender};

#[derive(Clone, Debug)]
pub enum Request {
    Connect,
    UseDemo,
    ReadAll,
    SetDpiLevel { level: usize, x: u16, y: u16 },
    SetDpiCurrent(usize),
    SetDpiLevels(usize),
    SetDpiSwitch(bool),
    SetRate(u16),
    SetDebounce([u8; 3]),
    SetLod(u8),
    SetMotionSync(bool),
    SetAngleSnap(bool),
    SetRipple(bool),
    SetFpsMode(bool),
    SetButton { button: u8, action: Action },
    FactoryReset,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Link {
    Searching,
    NotFound,
    NoPermission { path: String, product: String },
    Connected { path: String, product: String },
    Demo,
}

#[derive(Clone, Debug)]
pub enum Event {
    Link(Link),
    Dpi(Dpi),
    Sensor(Sensor),
    Buttons([Option<Action>; 7]),
    Version(String),
    Saved(String),
    Failed(String),
    Busy(bool),
}

pub struct Worker {
    pub tx: Sender<Request>,
    pub rx: Receiver<Event>,
}

pub fn start(demo: bool, notify: impl Fn() + Send + 'static) -> Worker {
    let (tx, req_rx) = channel::<Request>();
    let (ev_tx, rx) = channel::<Event>();
    std::thread::spawn(move || {
        let mut link: Option<Box<dyn Transport>> = None;
        let send = |e: Event| {
            let _ = ev_tx.send(e);
            notify();
        };
        let first = if demo { Request::UseDemo } else { Request::Connect };
        let mut queue = vec![first];
        loop {
            let req = match queue.pop() {
                Some(r) => r,
                None => match req_rx.recv() {
                    Ok(r) => r,
                    Err(_) => return,
                },
            };
            send(Event::Busy(true));
            match req {
                Request::Connect => {
                    send(Event::Link(Link::Searching));
                    link = None;
                    match device::find_mouse() {
                        Found::Ready(path, product, remote) => match Hidraw::open(&path, remote) {
                            Ok(h) => {
                                link = Some(Box::new(h));
                                send(Event::Link(Link::Connected { path: path.display().to_string(), product }));
                                queue.push(Request::ReadAll);
                            }
                            Err(e) => send(Event::Failed(e)),
                        },
                        Found::NoPermission(path, product) => {
                            send(Event::Link(Link::NoPermission { path: path.display().to_string(), product }))
                        }
                        Found::NotFound => send(Event::Link(Link::NotFound)),
                    }
                }
                Request::UseDemo => {
                    link = Some(Box::new(SimMouse::default()));
                    send(Event::Link(Link::Demo));
                    queue.push(Request::ReadAll);
                }
                other => match link.as_mut() {
                    None => send(Event::Failed("The mouse isn't connected yet.".into())),
                    Some(t) => run(t.as_mut(), other, &send),
                },
            }
            send(Event::Busy(false));
        }
    });
    Worker { tx, rx }
}

fn ask(t: &mut dyn Transport, cmd: u16, data: &[u8]) -> Result<Vec<u8>, String> {
    t.transact(&frame(cmd, data))
}

/// AM35model retry_on_fail(4, 0.08, 1.5): up to 4 tries, waiting 80 ms, then x1.5 each time.
fn ask_retry(t: &mut dyn Transport, cmd: u16, data: &[u8]) -> Result<Vec<u8>, String> {
    let mut delay = RETRY_DELAY_S;
    let mut last = String::new();
    for attempt in 0..RETRY_TRIES {
        match ask(t, cmd, data) {
            Ok(r) => return Ok(r),
            Err(e) => last = e,
        }
        if attempt + 1 < RETRY_TRIES {
            std::thread::sleep(std::time::Duration::from_secs_f32(delay));
            delay *= RETRY_BACKOFF;
        }
    }
    Err(last)
}

pub fn read_all(t: &mut dyn Transport, send: &dyn Fn(Event)) -> Result<(), String> {
    if let Ok(r) = ask(t, CMD_VERSION, &[0]) {
        // get_version: version text starts at byte 9.
        let text: String = r[9..].iter().map(|&b| if b.is_ascii_graphic() { b as char } else { ' ' }).collect();
        if let Some(v) = text.split_whitespace().find(|w| w.chars().filter(|c| *c == '.').count() >= 2) {
            send(Event::Version(v.to_string()));
        }
    }
    let r = ask_retry(t, CMD_GET_DPI, &[])?;
    if let Some(d) = Dpi::parse(&r) {
        send(Event::Dpi(d));
    }
    // Every setting must really come from the mouse: a value that can't be read is reported,
    // never filled in with a made-up default.
    let mut missing: Vec<&str> = vec![];
    let mut s = Sensor::default();
    match ask_retry(t, CMD_GET_RATE, &[]) {
        Ok(r) if POLLING_RATES.contains(&u16le(&r, 10)) => s.rate = u16le(&r, 10),
        _ => missing.push("polling rate"),
    }
    match ask_retry(t, CMD_GET_DEBOUNCE, &[]) {
        Ok(r) => s.debounce = [r[10], r[11], r[12]],
        Err(_) => missing.push("debounce"),
    }
    let reads: [(u16, &str); 6] = [
        (CMD_GET_LOD, "lift-off"),
        (CMD_GET_MOTION, "motion sync"),
        (CMD_GET_ANGLE, "angle snapping"),
        (CMD_GET_RIPPLE, "ripple control"),
        (CMD_GET_DPI_SWITCH, "quick DPI switch"),
        (CMD_GET_FPS, "game mode"),
    ];
    for (cmd, name) in reads {
        match ask_retry(t, cmd, &[]) {
            Ok(r) => match cmd {
                CMD_GET_LOD => s.lod = r[10].min(2),
                CMD_GET_MOTION => s.motion_sync = r[10] != 0,
                CMD_GET_ANGLE => s.angle_snap = r[10] != 0,
                CMD_GET_RIPPLE => s.ripple = r[10] != 0,
                CMD_GET_DPI_SWITCH => s.dpi_switch = r[10] == DPI_SWITCH_ON,
                _ => s.fps_mode = r[9] != 0,
            },
            Err(_) => missing.push(name),
        }
    }
    let mut keys = [None; 7];
    for n in 1..=7u8 {
        match ask_retry(t, CMD_GET_KEY, &[n]) {
            Ok(r) => keys[n as usize - 1] = Action::parse(&r),
            Err(_) => missing.push(BUTTON_NAMES[n as usize - 1]),
        }
    }
    if !missing.is_empty() {
        // Don't show or build on guessed values.
        return Err(format!("The mouse didn't answer for: {}. Press Read from mouse to try again.", missing.join(", ")));
    }
    send(Event::Sensor(s));
    send(Event::Buttons(keys));
    Ok(())
}

fn run(t: &mut dyn Transport, req: Request, send: &dyn Fn(Event)) {
    let result: Result<String, String> = match req {
        Request::ReadAll => read_all(t, send).map(|_| "Read the settings from the mouse.".to_string()),
        Request::SetDpiLevel { level, x, y } => {
            let [xl, xh] = x.to_le_bytes();
            let [yl, yh] = y.to_le_bytes();
            ask(t, CMD_SET_DPI_XY, &[level as u8, xl, xh, yl, yh]).map(|_| {
                if x == y {
                    format!("Level {} set to {x} DPI.", level + 1)
                } else {
                    format!("Level {} set to {x} × {y} DPI.", level + 1)
                }
            })
        }
        Request::SetDpiCurrent(i) => ask(t, CMD_SET_DPI_CURRENT, &[i as u8]).map(|_| format!("Now using level {}.", i + 1)),
        Request::SetDpiLevels(n) => ask(t, CMD_SET_DPI_LEVELS, &[n as u8]).map(|_| format!("The DPI button now cycles through {n} levels.")),
        Request::SetDpiSwitch(on) => ask(t, CMD_SET_DPI_SWITCH, &[if on { DPI_SWITCH_ON } else { 0 }])
            .map(|_| format!("Quick DPI switch {}.", if on { "on" } else { "off" })),
        Request::SetRate(hz) => ask(t, CMD_SET_RATE, &hz.to_le_bytes()).map(|_| format!("Polling rate set to {hz} Hz.")),
        Request::SetDebounce(v) => ask(t, CMD_SET_DEBOUNCE, &v).map(|_| "Debounce saved.".to_string()),
        Request::SetLod(v) => ask(t, CMD_SET_LOD, &[v]).map(|_| "Lift-off distance saved.".to_string()),
        Request::SetMotionSync(on) => ask(t, CMD_SET_MOTION, &[on as u8]).map(|_| "Motion sync saved.".to_string()),
        Request::SetAngleSnap(on) => ask(t, CMD_SET_ANGLE, &[on as u8]).map(|_| "Angle snapping saved.".to_string()),
        Request::SetRipple(on) => ask(t, CMD_SET_RIPPLE, &[on as u8]).map(|_| "Ripple control saved.".to_string()),
        Request::SetFpsMode(on) => ask(t, CMD_SET_FPS, &[on as u8]).map(|_| "Game mode saved.".to_string()),
        Request::SetButton { button, action } => t.transact(&action.set_frame(button)).and_then(|_| {
            // Read it back so the window shows what the mouse really stored.
            let r = ask(t, CMD_GET_KEY, &[button])?;
            let got = Action::parse(&r);
            let mut keys = [None; 7];
            keys[button as usize - 1] = got;
            send(Event::Buttons(keys));
            Ok(format!("{} now does: {}.", BUTTON_NAMES[button as usize - 1], action.label()))
        }),
        Request::FactoryReset => ask(t, CMD_RESET, &[]).and_then(|_| {
            std::thread::sleep(std::time::Duration::from_millis(300));
            read_all(t, send).map(|_| "The mouse is back to its factory settings.".to_string())
        }),
        Request::Connect | Request::UseDemo => Ok(String::new()),
    };
    match result {
        Ok(m) if !m.is_empty() => send(Event::Saved(m)),
        Ok(_) => {}
        Err(e) => send(Event::Failed(e)),
    }
}
