//! Infinity Mouse Settings: an unofficial Linux settings app for the AngryMiao AM Infinity mouse.
//!
//! Options:
//!   --demo                 use a pretend mouse (nothing is sent to real hardware)
//!   --screenshot <file>    draw about 120 frames, save the window as PNG, exit
//!   --page <name>          open on resolution, buttons, performance or help
//!   --read                 print every setting stored on the mouse, change nothing

mod device;
mod protocol;
mod ui;
mod worker;

use eframe::egui;

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub demo: bool,
    pub screenshot: Option<String>,
    pub page: Option<String>,
}

fn parse_args() -> Options {
    let mut o = Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--demo" => o.demo = true,
            "--screenshot" => o.screenshot = args.next(),
            "--page" => o.page = args.next(),
            "--read" => {
                read_and_print();
                std::process::exit(0);
            }
            "--help" | "-h" => {
                println!("infinity-mouse-settings [--demo] [--page name] [--screenshot file.png] [--read]");
                std::process::exit(0);
            }
            _ => {}
        }
    }
    o
}

/// `--read`: read every setting from the real mouse and print it (changes nothing).
fn read_and_print() {
    match device::find_mouse() {
        device::Found::Ready(path, product, remote) => {
            println!("{product} at {} (to the mouse with type 0x80: {remote})", path.display());
            let mut h = match device::Hidraw::open(&path, remote) {
                Ok(h) => h,
                Err(e) => return println!("{e}"),
            };
            let show = |e: worker::Event| println!("{e:?}");
            if let Err(e) = worker::read_all(&mut h, &show) {
                println!("failed: {e}");
            }
        }
        other => println!("{other:?}"),
    }
}

fn main() -> eframe::Result<()> {
    let opts = parse_args();
    let quiet = opts.screenshot.is_some();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Infinity Mouse Settings")
        .with_inner_size([1120.0, 740.0])
        .with_min_inner_size([900.0, 600.0])
        .with_app_id("infinity-mouse-settings");
    if quiet {
        // A screenshot run should not steal focus.
        viewport = viewport.with_active(false);
    }
    let native = eframe::NativeOptions { viewport, run_and_return: true, ..Default::default() };
    eframe::run_native(
        "Infinity Mouse Settings",
        native,
        Box::new(move |cc| Ok(Box::new(ui::App::new(cc, opts)))),
    )
}
