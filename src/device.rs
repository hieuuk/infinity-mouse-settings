//! Finding the mouse under /dev/hidraw*, and a pretend mouse for tests and demos.

use crate::protocol::*;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub trait Transport: Send {
    /// Send one frame and return the matching reply (input report 7, 62 bytes).
    fn transact(&mut self, frame: &[u8]) -> Result<Vec<u8>, String>;
}

#[derive(Clone, Debug, PartialEq)]
pub enum Found {
    /// path, product name, whether commands need the 0x80 "to the mouse" type byte
    Ready(PathBuf, String, bool),
    NoPermission(PathBuf, String),
    NotFound,
}

/// Look through /sys/class/hidraw for the mouse's settings interface (usage page 0xFF13).
pub fn find_mouse() -> Found {
    let Ok(dir) = std::fs::read_dir("/sys/class/hidraw") else { return Found::NotFound };
    let mut names: Vec<_> = dir.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    names.sort();
    for name in names {
        let base = PathBuf::from("/sys/class/hidraw").join(&name).join("device");
        let Ok(uevent) = std::fs::read_to_string(base.join("uevent")) else { continue };
        let id = uevent.lines().find_map(|l| l.strip_prefix("HID_ID=")).unwrap_or("").to_uppercase();
        let Some(&pid) = [PID_RECEIVER, PID_DIRECT].iter().find(|pid| id.ends_with(&format!("{:08X}:{:08X}", VID, pid))) else {
            continue;
        };
        let remote = pid == PID_RECEIVER;
        let desc = std::fs::read(base.join("report_descriptor")).unwrap_or_default();
        let page = [0x06, (USAGE_PAGE & 0xFF) as u8, (USAGE_PAGE >> 8) as u8];
        if !desc.windows(3).any(|w| w == page) {
            continue;
        }
        let product = uevent.lines().find_map(|l| l.strip_prefix("HID_NAME=")).unwrap_or("AM Infinity mouse").to_string();
        let path = PathBuf::from("/dev").join(&name);
        return match OpenOptions::new().read(true).write(true).open(&path) {
            Ok(_) => Found::Ready(path, product, remote),
            Err(_) => Found::NoPermission(path, product),
        };
    }
    Found::NotFound
}

pub struct Hidraw {
    file: File,
    remote: bool,
}

impl Hidraw {
    pub fn open(path: &PathBuf, remote: bool) -> Result<Hidraw, String> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)
            .map_err(|e| format!("can't open {}: {e}", path.display()))?;
        Ok(Hidraw { file, remote })
    }

    /// HIDIOCGINPUT(len): GET_REPORT for an input report (Linux 5.11 and later).
    fn get_input_report(&self, buf: &mut [u8]) -> isize {
        let len = buf.len() as libc::c_ulong;
        let req: libc::c_ulong = (3 << 30) | (len << 16) | ((b'H' as libc::c_ulong) << 8) | 0x0A;
        unsafe { libc::ioctl(self.file.as_raw_fd(), req as _, buf.as_mut_ptr()) as isize }
    }

    /// One GET_REPORT for input report 7, as AM Master's get_race_response does; None when the
    /// mouse has nothing to say (byte 1 is 0, ConnectionManager.read).
    fn fetch(&self) -> Option<[u8; REPORT_SIZE]> {
        let mut r = [0u8; REPORT_SIZE];
        r[0] = REPORT_IN;
        let n = self.get_input_report(&mut r);
        (n > 0 && r[1] != 0).then_some(r)
    }

    /// _clear_old_responses: throw away up to 10 waiting replies (and any queued interrupt data).
    fn drain(&mut self) {
        let mut buf = [0u8; 64];
        for _ in 0..32 {
            match self.file.read(&mut buf) {
                Ok(n) if n > 0 => continue,
                _ => break,
            }
        }
        for _ in 0..CLEAR_OLD_MAX {
            if self.fetch().is_none() {
                break;
            }
        }
    }
}

impl Transport for Hidraw {
    /// on_send: clear, write the padded report, then every 50 ms ask for report 7 until the
    /// reply with our command id arrives, for at most 5 s.
    fn transact(&mut self, frame: &[u8]) -> Result<Vec<u8>, String> {
        self.drain();
        let report = wrap(frame, self.remote);
        self.file.write_all(&report).map_err(|e| format!("sending to the mouse failed: {e}"))?;
        let deadline = Instant::now() + Duration::from_secs_f32(REPLY_TIMEOUT_S);
        let mut buf = [0u8; REPORT_SIZE];
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(REPLY_POLL_MS));
            if let Some(r) = self.fetch() {
                if reply_matches(frame, &r) {
                    return Ok(r.to_vec());
                }
            }
            // Linux extra: a reply pushed on the interrupt pipe is accepted too.
            if let Ok(n) = self.file.read(&mut buf) {
                if n > 0 && reply_matches(frame, &buf[..n]) {
                    return Ok(buf[..n].to_vec());
                }
            }
        }
        Err("the mouse didn't answer".into())
    }
}

/// A pretend mouse that answers like the real one, for the demo view (--demo).
pub struct SimMouse {
    pub dpi: Dpi,
    pub sensor: Sensor,
    pub keys: [Action; 7],
    pub frames_seen: usize,
}

impl Default for SimMouse {
    fn default() -> Self {
        SimMouse { dpi: Dpi::default(), sensor: Sensor::default(), keys: DEFAULT_ACTIONS, frames_seen: 0 }
    }
}

impl SimMouse {
    fn reply(frame: &[u8], data: &[u8]) -> Vec<u8> {
        let len = (2 + 1 + data.len()) as u16;
        let mut r = vec![REPORT_IN, (4 + len) as u8, 0, 0x05, 0x5B, len as u8, (len >> 8) as u8, frame[4], frame[5], 0];
        r.extend_from_slice(data);
        r.resize(REPORT_SIZE, 0);
        r
    }
}

impl Transport for SimMouse {
    fn transact(&mut self, f: &[u8]) -> Result<Vec<u8>, String> {
        self.frames_seen += 1;
        if f.len() < 6 || f[0] != 0x05 || f[1] != 0x5A {
            return Err("bad frame".into());
        }
        let cmd = u16le(f, 4);
        let d = &f[6..];
        let at = |i: usize| d.get(i).copied().unwrap_or(0);
        let ok = Self::reply(f, &[]);
        let s = &mut self.sensor;
        Ok(match cmd {
            CMD_GET_DPI => {
                let mut v = vec![self.dpi.levels as u8, self.dpi.current as u8];
                for x in self.dpi.x {
                    v.extend_from_slice(&x.to_le_bytes());
                }
                for y in self.dpi.y {
                    v.extend_from_slice(&y.to_le_bytes());
                }
                Self::reply(f, &v)
            }
            CMD_SET_DPI_XY => {
                let i = (at(0) as usize).min(7);
                self.dpi.x[i] = u16le(d, 1);
                self.dpi.y[i] = u16le(d, 3);
                ok
            }
            CMD_SET_DPI_CURRENT => {
                self.dpi.current = at(0) as usize;
                ok
            }
            CMD_SET_DPI_LEVELS => {
                self.dpi.levels = (at(0) as usize).clamp(1, 8);
                ok
            }
            CMD_SET_RATE => {
                s.rate = u16le(d, 0);
                ok
            }
            CMD_GET_RATE => Self::reply(f, &s.rate.to_le_bytes()),
            CMD_SET_DEBOUNCE => {
                s.debounce = [at(0), at(1), at(2)];
                ok
            }
            CMD_GET_DEBOUNCE => Self::reply(f, &s.debounce),
            CMD_SET_LOD => {
                s.lod = at(0);
                ok
            }
            CMD_GET_LOD => Self::reply(f, &[s.lod]),
            CMD_SET_MOTION => {
                s.motion_sync = at(0) != 0;
                ok
            }
            CMD_GET_MOTION => Self::reply(f, &[s.motion_sync as u8]),
            CMD_SET_ANGLE => {
                s.angle_snap = at(0) != 0;
                ok
            }
            CMD_GET_ANGLE => Self::reply(f, &[s.angle_snap as u8]),
            CMD_SET_RIPPLE => {
                s.ripple = at(0) != 0;
                ok
            }
            CMD_GET_RIPPLE => Self::reply(f, &[s.ripple as u8]),
            CMD_SET_DPI_SWITCH => {
                s.dpi_switch = at(0) == DPI_SWITCH_ON;
                ok
            }
            CMD_GET_DPI_SWITCH => Self::reply(f, &[if s.dpi_switch { DPI_SWITCH_ON } else { 0 }]),
            CMD_SET_FPS => {
                s.fps_mode = at(0) != 0;
                ok
            }
            CMD_GET_FPS => {
                let mut r = Self::reply(f, &[]);
                r[9] = s.fps_mode as u8;
                r
            }
            CMD_SET_KEY => {
                let n = at(0) as usize;
                if (1..=7).contains(&n) {
                    let mut fake = vec![0u8; 12];
                    fake.extend_from_slice(&d[2.min(d.len())..]);
                    fake[11] = at(1);
                    if let Some(a) = Action::parse(&fake) {
                        self.keys[n - 1] = a;
                    }
                }
                ok
            }
            CMD_GET_KEY => {
                let n = (at(0) as usize).clamp(1, 7);
                let set = self.keys[n - 1].set_frame(n as u8);
                // set frame data after "n type" is laid out like the read reply from byte 12.
                let mut v = vec![n as u8];
                v.extend_from_slice(&set[7..]);
                Self::reply(f, &v)
            }
            CMD_RESET => {
                *self = SimMouse { frames_seen: self.frames_seen, ..SimMouse::default() };
                ok
            }
            CMD_VERSION => {
                let mut r = Self::reply(f, b"v1.0.0-demo");
                r[8] = 12;
                r
            }
            _ => ok,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sim_round_trips_buttons_and_dpi() {
        let mut m = SimMouse::default();
        let a = Action::Key { usage: 0x06, mods: MOD_CTRL };
        m.transact(&a.set_frame(4)).unwrap();
        let r = m.transact(&frame(CMD_GET_KEY, &[4])).unwrap();
        assert_eq!(Action::parse(&r), Some(a));
        m.transact(&frame(CMD_SET_DPI_XY, &[0, 0x58, 0x02, 0x84, 0x03])).unwrap();
        let r = m.transact(&frame(CMD_GET_DPI, &[])).unwrap();
        let d = Dpi::parse(&r).unwrap();
        assert_eq!((d.x[0], d.y[0]), (600, 900));
    }
}
