//! uvview (LinuxViewer) shows and controls a Windows PC over UltraViewer.

mod app;
mod audio;
mod gui;
mod input;
mod messages;
mod profile;
mod screen;
mod session;
mod wire;

use session::{Config, Event};
use std::process::exit;
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Mutex};

const USAGE: &str = "usage: uvview [--id <ID>]\n       uvview --id <ID> --snapshot screen.png\n\n  --id         fill in the partner ID\n  --snapshot   connect without a window, save the first screen to a PNG file and exit\n               (the password is read from $UV_PASSWORD or asked for)";

fn fail(msg: impl std::fmt::Display) -> ! {
    eprintln!("uvview: {msg}");
    exit(1)
}

fn main() {
    let (mut id, mut snapshot) = (String::new(), None);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.trim_start_matches('-') {
            "id" => id = args.next().unwrap_or_default(),
            "snapshot" => snapshot = args.next(),
            _ => {
                eprintln!("{USAGE}");
                exit(2)
            }
        }
    }
    match snapshot {
        Some(path) if !id.is_empty() => run_snapshot(&id, &path),
        Some(_) => {
            eprintln!("{USAGE}");
            exit(2)
        }
        None => gui::run(app::App::new(id)).unwrap_or_else(|e| fail(e)),
    }
}

fn run_snapshot(id: &str, path: &str) {
    let password = match std::env::var("UV_PASSWORD") {
        Ok(p) if !p.is_empty() => p,
        _ => rpassword::prompt_password("UltraViewer password: ").unwrap_or_else(|e| fail(format!("set UV_PASSWORD or run in a terminal ({e})"))),
    };
    let (tx, rx) = sync_channel(64);
    let saved: Arc<Mutex<Option<Result<(), String>>>> = Arc::default();
    let (quit, out, file) = (tx.clone(), saved.clone(), path.to_string());
    let cfg = Config {
        registration: profile::load_registration().unwrap_or_else(|e| fail(e)),
        screen_width: 1920, screen_height: 1080, display_name: "LinuxViewer".into(),
        on_status: Box::new(|s| eprintln!("{s}")),
        on_frame: Box::new(move |px, w, h| {
            let mut out = out.lock().unwrap();
            if out.is_none() {
                *out = Some(save_png(&file, px, w, h).map_err(|e| e.to_string()));
            }
            let _ = quit.try_send(Event::Quit); // try_: this callback runs on the receiving thread
        }),
        on_peer_changed: Box::new(|_| {}),
        sound: false,
        on_audio: Box::new(|_| {}),
    };
    let result = session::run(cfg, id, &password, tx, rx, &session::dial);
    let outcome = saved.lock().unwrap().take();
    match outcome {
        Some(Ok(())) => eprintln!("Saved {path}"),
        Some(Err(e)) => fail(e),
        None => fail(result.err().map_or("no screen received".into(), |e| e.to_string())),
    }
}

fn save_png(path: &str, px: &[u8], w: usize, h: usize) -> Result<(), Box<dyn std::error::Error>> {
    let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(path)?), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(px)?;
    Ok(())
}
