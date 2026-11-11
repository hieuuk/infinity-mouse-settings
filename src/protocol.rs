//! How the AM Infinity mouse is spoken to. Every number here is listed with its source in docs/PROTOCOL.md.

pub const VID: u16 = 0x0E8D;
/// Reached with AM35_D_CON: every command goes to the mouse with type byte 0x80 (remote_checker).
/// Tested on a real mouse: with type 0 the settings commands get no reply.
pub const PID_RECEIVER: u16 = 0x0703;
/// Reached with AM35_CON: commands use type byte 0.
pub const PID_DIRECT: u16 = 0x0880;
pub const TYPE_REMOTE: u8 = 0x80;
pub const USAGE_PAGE: u16 = 0xFF13;
pub const REPORT_OUT: u8 = 6;
pub const REPORT_IN: u8 = 7;
pub const REPORT_SIZE: usize = 62;
/// Wait before each reply fetch: get_race_response / ConnectionManager.read timeout_ms = 50.
pub const REPLY_POLL_MS: u64 = 50;
/// Give up on a reply after FunctionArea.DEFAULT_TIMEOUT = 5 s.
pub const REPLY_TIMEOUT_S: f32 = 5.0;
/// _clear_old_responses(max_clear = 10).
pub const CLEAR_OLD_MAX: usize = 10;
/// retry_on_fail(max_retries=4, delay=0.08, backoff=1.5) in AM35model.
pub const RETRY_TRIES: usize = 4;
pub const RETRY_DELAY_S: f32 = 0.08;
pub const RETRY_BACKOFF: f32 = 1.5;

pub const DPI_LEVELS_MAX: usize = 8;
pub const DPI_MIN: u16 = 50;
/// The web page caps its slider at 26000, but the mouse itself stores 30000 as level 8
/// (read live with C5 30), so the mouse's own limit is used.
pub const DPI_MAX: u16 = 30000;
pub const DPI_STEP: u16 = 50;
pub const DEFAULT_DPI: [u16; 8] = [800, 1600, 2400, 3200, 5000, 10000, 20000, 26000];
pub const DPI_COLOURS: [[u8; 3]; 8] = [
    [0xFF, 0x00, 0x00],
    [0x00, 0xFF, 0x00],
    [0x00, 0x00, 0xFF],
    [0x00, 0xFF, 0xFF],
    [0x80, 0x00, 0x80],
    [0xFF, 0xFF, 0x00],
    [0xFF, 0x00, 0xFF],
    [0xFF, 0xFF, 0xFF],
];
pub const DPI_SWITCH_ON: u8 = 0x18;

pub const POLLING_RATES: [u16; 7] = [125, 250, 500, 1000, 2000, 4000, 8000];
pub const DEBOUNCE_LABELS_MS: [u8; 7] = [10, 8, 6, 4, 3, 2, 0];
pub const BT_DEBOUNCE: [u8; 7] = [10, 8, 6, 4, 3, 2, 0];

/// Cable / 2.4G debounce value for a step, which depends on the polling rate.
pub fn debounce_table(rate: u16) -> [u8; 7] {
    match rate {
        2000 => [20, 16, 12, 8, 4, 2, 0],
        4000 => [39, 31, 23, 15, 7, 3, 0],
        8000 => [79, 63, 47, 31, 15, 7, 0],
        _ => [10, 8, 6, 4, 3, 2, 0],
    }
}

/// The step whose value is closest to what the mouse reports.
pub fn debounce_step(rate: u16, value: u8) -> usize {
    let t = debounce_table(rate);
    let mut best = 0;
    for (i, v) in t.iter().enumerate() {
        if (*v as i32 - value as i32).abs() < (t[best] as i32 - value as i32).abs() {
            best = i;
        }
    }
    best
}

pub fn debounce_triple(rate: u16, step: usize) -> [u8; 3] {
    let t = debounce_table(rate);
    let s = step.min(6);
    [t[s], t[s], BT_DEBOUNCE[s]]
}

// Command ids (sent as cmd_lo cmd_hi).
pub const CMD_SET_RATE: u16 = 0x3010;
pub const CMD_GET_RATE: u16 = 0x3011;
pub const CMD_SET_KEY: u16 = 0x30C2;
pub const CMD_GET_KEY: u16 = 0x30C3;
pub const CMD_SET_DPI_XY: u16 = 0x30C4;
pub const CMD_GET_DPI: u16 = 0x30C5;
pub const CMD_SET_DPI_LEVELS: u16 = 0x30C6;
pub const CMD_SET_DPI_CURRENT: u16 = 0x30C7;
pub const CMD_SET_LOD: u16 = 0x30CC;
pub const CMD_GET_LOD: u16 = 0x30CD;
pub const CMD_RESET: u16 = 0x30CE;
pub const CMD_SET_MOTION: u16 = 0x30D2;
pub const CMD_GET_MOTION: u16 = 0x30D3;
pub const CMD_SET_ANGLE: u16 = 0x30D4;
pub const CMD_GET_ANGLE: u16 = 0x30D5;
pub const CMD_SET_RIPPLE: u16 = 0x30D6;
pub const CMD_GET_RIPPLE: u16 = 0x30D7;
pub const CMD_SET_FPS: u16 = 0x30DE;
pub const CMD_GET_FPS: u16 = 0x30DF;
pub const CMD_SET_DEBOUNCE: u16 = 0x30E8;
pub const CMD_GET_DEBOUNCE: u16 = 0x30E9;
pub const CMD_SET_DPI_SWITCH: u16 = 0x30EA;
pub const CMD_GET_DPI_SWITCH: u16 = 0x30EB;
pub const CMD_VERSION: u16 = 0x1C07;

/// `05 5A len_lo len_hi cmd_lo cmd_hi data…`
pub fn frame(cmd: u16, data: &[u8]) -> Vec<u8> {
    let len = (2 + data.len()) as u16;
    let mut f = vec![0x05, 0x5A, len as u8, (len >> 8) as u8, cmd as u8, (cmd >> 8) as u8];
    f.extend_from_slice(data);
    f
}

/// The 62-byte output report carrying a frame: `06, len, type, frame…`.
pub fn wrap(frame: &[u8], remote: bool) -> Vec<u8> {
    let mut r = vec![REPORT_OUT, frame.len() as u8, if remote { TYPE_REMOTE } else { 0 }];
    r.extend_from_slice(frame);
    r.resize(REPORT_SIZE, 0);
    r
}

/// Does this reply (input report 7) answer this frame?
pub fn reply_matches(frame: &[u8], reply: &[u8]) -> bool {
    reply.len() >= 9 && reply[0] == REPORT_IN && reply[1] != 0 && reply[7..9] == frame[4..6]
}

/// A key-assignment frame exactly as AM Master writes it: "05 5A 08 00 C2 30 …" + 7 bytes.
fn key_frame(data: &[u8]) -> Vec<u8> {
    let mut f = frame(CMD_SET_KEY, data);
    f[2] = 0x08;
    f
}

pub fn u16le(b: &[u8], at: usize) -> u16 {
    b.get(at).copied().unwrap_or(0) as u16 | (b.get(at + 1).copied().unwrap_or(0) as u16) << 8
}

#[derive(Clone, Debug, PartialEq)]
pub struct Dpi {
    pub levels: usize,
    pub current: usize,
    pub x: [u16; 8],
    pub y: [u16; 8],
}

impl Default for Dpi {
    fn default() -> Self {
        Dpi { levels: 6, current: 1, x: DEFAULT_DPI, y: DEFAULT_DPI }
    }
}

impl Dpi {
    pub fn parse(r: &[u8]) -> Option<Dpi> {
        if r.len() < 44 {
            return None;
        }
        let mut d = Dpi { levels: (r[10] as usize).clamp(1, 8), current: r[11] as usize, x: [0; 8], y: [0; 8] };
        for i in 0..8 {
            d.x[i] = u16le(r, 12 + i * 2);
            d.y[i] = u16le(r, 28 + i * 2);
        }
        d.current = d.current.min(d.levels - 1);
        Some(d)
    }
}

pub fn snap_dpi(v: f64) -> u16 {
    let v = (v / DPI_STEP as f64).round() * DPI_STEP as f64;
    v.clamp(DPI_MIN as f64, DPI_MAX as f64) as u16
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sensor {
    pub rate: u16,
    pub debounce: [u8; 3],
    pub lod: u8,
    pub motion_sync: bool,
    pub angle_snap: bool,
    pub ripple: bool,
    pub fps_mode: bool,
    pub dpi_switch: bool,
}

impl Default for Sensor {
    fn default() -> Self {
        Sensor {
            rate: 8000,
            debounce: [79, 79, 10],
            lod: 1,
            motion_sync: true,
            angle_snap: true,
            ripple: true,
            fps_mode: false,
            dpi_switch: true,
        }
    }
}

/// What a button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Left,
    Right,
    Middle,
    Forward,
    Back,
    WheelUp,
    WheelDown,
    DpiPlus,
    DpiMinus,
    DpiCycle,
    ProfileSwitch,
    Media(u16),
    Key { usage: u8, mods: u8 },
    Macro,
    Other(u8),
}

pub const BUTTON_NAMES: [&str; 7] =
    ["Left button", "Right button", "Wheel click", "Forward (front side)", "Back (rear side)", "Wheel up", "Wheel down"];

pub const DEFAULT_ACTIONS: [Action; 7] =
    [Action::Left, Action::Right, Action::Middle, Action::Forward, Action::Back, Action::WheelUp, Action::WheelDown];

pub const MEDIA: [(u16, &str); 16] = [
    (0x00CD, "Play / pause"),
    (0x00B7, "Stop"),
    (0x00B5, "Previous track"),
    (0x00B6, "Next track"),
    (0x00E2, "Mute"),
    (0x00E9, "Volume up"),
    (0x00EA, "Volume down"),
    (0x0183, "Media player"),
    (0x0192, "Calculator"),
    (0x018A, "Email"),
    (0x0194, "My computer"),
    (0x0227, "Refresh"),
    (0x0221, "Search"),
    (0x0223, "Home page"),
    (0x006F, "Brightness up"),
    (0x0070, "Brightness down"),
];

pub const MOD_CTRL: u8 = 1;
pub const MOD_SHIFT: u8 = 2;
pub const MOD_ALT: u8 = 4;
pub const MOD_SUPER: u8 = 8;

/// Keyboard keys a button can press (HID usage, name).
pub const KEYS: &[(u8, &str)] = &[
    (0x04, "A"), (0x05, "B"), (0x06, "C"), (0x07, "D"), (0x08, "E"), (0x09, "F"), (0x0A, "G"),
    (0x0B, "H"), (0x0C, "I"), (0x0D, "J"), (0x0E, "K"), (0x0F, "L"), (0x10, "M"), (0x11, "N"),
    (0x12, "O"), (0x13, "P"), (0x14, "Q"), (0x15, "R"), (0x16, "S"), (0x17, "T"), (0x18, "U"),
    (0x19, "V"), (0x1A, "W"), (0x1B, "X"), (0x1C, "Y"), (0x1D, "Z"),
    (0x1E, "1"), (0x1F, "2"), (0x20, "3"), (0x21, "4"), (0x22, "5"), (0x23, "6"), (0x24, "7"),
    (0x25, "8"), (0x26, "9"), (0x27, "0"),
    (0x28, "Enter"), (0x29, "Esc"), (0x2A, "Backspace"), (0x2B, "Tab"), (0x2C, "Space"),
    (0x2D, "-"), (0x2E, "="), (0x2F, "["), (0x30, "]"), (0x31, "\\"), (0x33, ";"), (0x34, "'"),
    (0x35, "`"), (0x36, ","), (0x37, "."), (0x38, "/"), (0x39, "Caps Lock"),
    (0x3A, "F1"), (0x3B, "F2"), (0x3C, "F3"), (0x3D, "F4"), (0x3E, "F5"), (0x3F, "F6"),
    (0x40, "F7"), (0x41, "F8"), (0x42, "F9"), (0x43, "F10"), (0x44, "F11"), (0x45, "F12"),
    (0x46, "Print Screen"), (0x47, "Scroll Lock"), (0x48, "Pause"), (0x49, "Insert"),
    (0x4A, "Home"), (0x4B, "Page Up"), (0x4C, "Delete"), (0x4D, "End"), (0x4E, "Page Down"),
    (0x4F, "Right arrow"), (0x50, "Left arrow"), (0x51, "Down arrow"), (0x52, "Up arrow"),
    (0x65, "Menu"),
];

pub fn key_name(usage: u8) -> String {
    KEYS.iter().find(|k| k.0 == usage).map(|k| k.1.to_string()).unwrap_or(format!("key 0x{usage:02X}"))
}

pub fn mods_text(mods: u8) -> String {
    let mut s = String::new();
    for (bit, name) in [(MOD_CTRL, "Ctrl+"), (MOD_SHIFT, "Shift+"), (MOD_ALT, "Alt+"), (MOD_SUPER, "Super+")] {
        if mods & bit != 0 {
            s.push_str(name);
        }
    }
    s
}

impl Action {
    pub fn label(&self) -> String {
        match self {
            Action::Left => "Left click".into(),
            Action::Right => "Right click".into(),
            Action::Middle => "Middle click".into(),
            Action::Forward => "Forward".into(),
            Action::Back => "Back".into(),
            Action::WheelUp => "Scroll up".into(),
            Action::WheelDown => "Scroll down".into(),
            Action::DpiPlus => "DPI up".into(),
            Action::DpiMinus => "DPI down".into(),
            Action::DpiCycle => "DPI cycle".into(),
            Action::ProfileSwitch => "Profile switch".into(),
            Action::Media(c) => MEDIA.iter().find(|m| m.0 == *c).map(|m| m.1.to_string()).unwrap_or(format!("Media 0x{c:04X}")),
            Action::Key { usage, mods } => format!("{}{}", mods_text(*mods), key_name(*usage)),
            Action::Macro => "Macro (set in AM Master)".into(),
            Action::Other(t) => format!("Unknown function {t}"),
        }
    }

    /// The frame that assigns this action to button `n` (1..=7).
    pub fn set_frame(&self, n: u8) -> Vec<u8> {
        let simple = |t: u8| frame(CMD_SET_KEY, &[n, t]);
        match *self {
            Action::Left => simple(1),
            Action::Right => simple(2),
            Action::Middle => simple(3),
            Action::Forward => simple(4),
            Action::Back => simple(5),
            Action::WheelUp => simple(12),
            Action::WheelDown => simple(13),
            Action::ProfileSwitch => simple(14),
            Action::DpiPlus => frame(CMD_SET_KEY, &[n, 11, 0, 0, 0]),
            Action::DpiMinus => frame(CMD_SET_KEY, &[n, 11, 1, 0, 0]),
            Action::DpiCycle => frame(CMD_SET_KEY, &[n, 11, 3, 0, 0]),
            // AM Master sends these with length 08 and a trailing 00 (change_key); copied exactly.
            Action::Media(c) => key_frame(&[n, 6, c as u8, (c >> 8) as u8, 0, 0, 0]),
            Action::Key { usage, mods } => key_frame(&[n, 6, 0, 0, usage, mods & 0x0F, 0]),
            Action::Macro => simple(8),
            Action::Other(t) => simple(t),
        }
    }

    /// Read a `C3 30` reply.
    pub fn parse(r: &[u8]) -> Option<Action> {
        if r.len() < 12 {
            return None;
        }
        let t = r[11];
        Some(match t {
            1 => Action::Left,
            2 => Action::Right,
            3 => Action::Middle,
            4 => Action::Forward,
            5 => Action::Back,
            12 => Action::WheelUp,
            13 => Action::WheelDown,
            14 => Action::ProfileSwitch,
            8 => Action::Macro,
            11 => match r.get(12).copied().unwrap_or(0) {
                1 => Action::DpiMinus,
                3 => Action::DpiCycle,
                _ => Action::DpiPlus,
            },
            6 => {
                let consumer = u16le(r, 12);
                let usage = r.get(14).copied().unwrap_or(0);
                let mods = r.get(15).copied().unwrap_or(0);
                if consumer != 0 {
                    Action::Media(consumer)
                } else {
                    Action::Key { usage, mods }
                }
            }
            0 | 0xFF => return None,
            other => Action::Other(other),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frames_match_am_master() {
        // AM35model set_dpixy: "05 5A 07 00 C4 30 {id} {x} {y}"
        assert_eq!(frame(CMD_SET_DPI_XY, &[2, 0x60, 0x09, 0x60, 0x09]), vec![5, 0x5A, 7, 0, 0xC4, 0x30, 2, 0x60, 9, 0x60, 9]);
        // AM35model restore: "05 5A 02 00 CE 30"
        assert_eq!(frame(CMD_RESET, &[]), vec![5, 0x5A, 2, 0, 0xCE, 0x30]);
        // set_report_rate 8000 → "05 5A 04 00 10 30 40 1F"
        assert_eq!(frame(CMD_SET_RATE, &[0x40, 0x1F]), vec![5, 0x5A, 4, 0, 0x10, 0x30, 0x40, 0x1F]);
        // change_key consumer Volume up on button 4 → "05 5A 08 00 C2 30 04 06 e9 00 00 00 00"
        assert_eq!(Action::Media(0xE9).set_frame(4), vec![5, 0x5A, 8, 0, 0xC2, 0x30, 4, 6, 0xE9, 0, 0, 0, 0]);
        // change_key std key A with Ctrl → "... 06 00 00 04 01 00"
        assert_eq!(Action::Key { usage: 4, mods: 1 }.set_frame(5), vec![5, 0x5A, 8, 0, 0xC2, 0x30, 5, 6, 0, 0, 4, 1, 0]);
        let w = wrap(&frame(CMD_GET_DPI, &[]), false);
        assert_eq!(&w[..9], &[6, 6, 0, 5, 0x5A, 2, 0, 0xC5, 0x30]);
        assert_eq!(w.len(), 62);
    }
}
