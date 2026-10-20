//! uvview shows and controls a Windows PC over UltraViewer.

mod audio;
mod input;
mod messages;
mod profile;
mod screen;
mod session;
mod wire;

use input::Mouse;
use session::{Config, Event};
use softbuffer::{Context, Surface};
use std::num::NonZeroU32;
use std::process::exit;
use std::rc::Rc;
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::PhysicalKey;
use winit::window::{Window, WindowId};

const USAGE: &str = "usage: uvview --id <ID> [--sound] [--snapshot screen.png]\n\n  --sound      play the Windows computer's audio\n  --snapshot   save the first screen to a PNG file and exit (no window)\n\nThe password is read from $UV_PASSWORD or asked for.";

fn fail(msg: impl std::fmt::Display) -> ! {
    eprintln!("uvview: {msg}");
    exit(1)
}

fn main() {
    let (mut id, mut snapshot, mut sound) = (String::new(), None, false);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.trim_start_matches('-') {
            "id" => id = args.next().unwrap_or_default(),
            "snapshot" => snapshot = args.next(),
            "sound" => sound = true,
            _ => {
                eprintln!("{USAGE}");
                exit(2)
            }
        }
    }
    if id.is_empty() {
        eprintln!("{USAGE}");
        exit(2)
    }
    let password = match std::env::var("UV_PASSWORD") {
        Ok(p) if !p.is_empty() => p,
        _ => rpassword::prompt_password("UltraViewer password: ").unwrap_or_else(|e| fail(format!("set UV_PASSWORD or run in a terminal ({e})"))),
    };
    let registration = profile::load_registration().unwrap_or_else(|e| fail(e));
    // shortcut: reports a fixed 1920x1080 viewer screen in Login; read the real monitor if Windows scales by it.
    let cfg = Config {
        registration, screen_width: 1920, screen_height: 1080, display_name: "LinuxViewer".into(),
        on_status: Box::new(|s| eprintln!("{s}")),
        on_frame: Box::new(|_, _, _| {}),
        on_peer_changed: Box::new(|_| {}),
        sound: false,
        on_audio: Box::new(|_| {}),
    };
    match snapshot {
        Some(path) => run_snapshot(cfg, &id, &password, &path),
        None => run_window(cfg, id, password, sound),
    }
}

fn run_snapshot(mut cfg: Config, id: &str, password: &str, path: &str) {
    let (tx, rx) = sync_channel(64);
    let saved: Arc<Mutex<Option<Result<(), String>>>> = Arc::default();
    let (quit, out, file) = (tx.clone(), saved.clone(), path.to_string());
    cfg.on_frame = Box::new(move |px, w, h| {
        let mut out = out.lock().unwrap();
        if out.is_none() {
            *out = Some(save_png(&file, px, w, h).map_err(|e| e.to_string()));
        }
        let _ = quit.try_send(Event::Quit); // try_: this callback runs on the receiving thread
    });
    let result = session::run(cfg, id, password, tx, rx, &session::dial);
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

/// State the session thread hands to the window.
#[derive(Default)]
struct Shared {
    pix: Vec<u8>, // top-down RGBA
    w: usize,
    h: usize,
    label: String,
    status: String,
}

/// Lets the session thread change `Shared` and wake the window to redraw.
#[derive(Clone)]
struct Ui {
    shared: Arc<Mutex<Shared>>,
    proxy: EventLoopProxy<()>,
}

impl Ui {
    fn update(&self, f: impl FnOnce(&mut Shared)) {
        f(&mut self.shared.lock().unwrap());
        let _ = self.proxy.send_event(());
    }
}

struct App {
    shared: Arc<Mutex<Shared>>,
    window: Option<(Rc<Window>, Surface<Rc<Window>, Rc<Window>>)>,
    /// Carries input and the final Quit to the session thread.
    tx: SyncSender<Event>,
    /// Unsent fraction of a wheel notch (touchpads scroll in pixels).
    wheel_rest: f64,
}

fn run_window(mut cfg: Config, id: String, password: String, sound: bool) {
    let event_loop = EventLoop::<()>::with_user_event().build().unwrap_or_else(|e| fail(e));
    let shared = Arc::new(Mutex::new(Shared { label: format!("UltraViewer {id}"), status: "Connecting…".into(), ..Default::default() }));
    let ui = Ui { shared: shared.clone(), proxy: event_loop.create_proxy() };
    let status = ui.clone();
    cfg.on_status = Box::new(move |s| {
        eprintln!("{s}");
        status.update(|sh| sh.status = s.into());
    });
    let (label, id2) = (ui.clone(), id.clone());
    cfg.on_peer_changed = Box::new(move |name| label.update(|sh| sh.label = format!("{name} ({id2})")));
    let frame = ui.clone();
    cfg.on_frame = Box::new(move |px, w, h| frame.update(|sh| {
        sh.pix.clear();
        sh.pix.extend_from_slice(px);
        (sh.w, sh.h) = (w, h);
    }));

    if sound {
        match audio::Player::start() {
            Ok(player) => {
                cfg.sound = true;
                cfg.on_audio = Box::new(move |pcm| player.play(pcm));
            }
            Err(e) => eprintln!("uvview: sound is off: {e}"),
        }
    }

    let (tx, rx) = sync_channel(64);
    let session_tx = tx.clone();
    let worker = thread::spawn(move || {
        if let Err(e) = session::run(cfg, &id, &password, session_tx, rx, &session::dial) {
            eprintln!("Disconnected: {e}");
            ui.update(|sh| sh.status = format!("Disconnected: {e}"));
        }
    });

    let mut app = App { shared, window: None, tx, wheel_rest: 0.0 };
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("uvview: {e}");
    }
    let _ = app.tx.send(Event::Quit);
    let start = Instant::now(); // let the session tell Windows we left
    while !worker.is_finished() && start.elapsed() < Duration::from_secs(2) {
        thread::sleep(Duration::from_millis(20));
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes().with_title("uvview").with_inner_size(LogicalSize::new(1280, 720));
        let window = Rc::new(el.create_window(attrs).unwrap_or_else(|e| fail(e)));
        let context = Context::new(window.clone()).unwrap_or_else(|e| fail(e));
        let surface = Surface::new(&context, window.clone()).unwrap_or_else(|e| fail(e));
        self.window = Some((window, surface));
    }

    fn user_event(&mut self, _: &ActiveEventLoop, _: ()) {
        if let Some((w, _)) = &self.window {
            w.request_redraw();
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some((window, surface)) = &mut self.window else { return };
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::CursorMoved { position, .. } => {
                let sh = self.shared.lock().unwrap();
                let size = window.inner_size();
                let (ox, oy, w, h) = fit(size.width as usize, size.height as usize, sh.w, sh.h);
                if w > 0 {
                    // Window pixels to remote pixels; the session clamps points outside the screen.
                    let x = ((position.x - ox as f64) * sh.w as f64 / w as f64).floor() as i32;
                    let y = ((position.y - oy as f64) * sh.h as f64 / h as f64).floor() as i32;
                    let _ = self.tx.try_send(Event::Mouse(Mouse::Move { x, y })); // a dropped move is harmless
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let button = match button {
                    MouseButton::Left => 0,
                    MouseButton::Right => 1,
                    MouseButton::Middle => 2,
                    _ => return,
                };
                let _ = self.tx.send(Event::Mouse(Mouse::Button { button, pressed: state == ElementState::Pressed }));
            }
            WindowEvent::MouseWheel { delta, .. } => {
                // shortcut: 40 touchpad pixels per notch; tune if touchpad scrolling feels off.
                self.wheel_rest += match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as f64,
                    MouseScrollDelta::PixelDelta(p) => p.y / 40.0,
                };
                let notches = self.wheel_rest.trunc();
                self.wheel_rest -= notches;
                if notches != 0.0 {
                    let _ = self.tx.send(Event::Mouse(Mouse::Wheel(notches as i32)));
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    let _ = self.tx.send(Event::Key(code, event.state == ElementState::Pressed));
                }
            }
            WindowEvent::Focused(false) => {
                let _ = self.tx.send(Event::ReleaseAll);
            }
            WindowEvent::RedrawRequested => {
                let sh = self.shared.lock().unwrap();
                window.set_title(&format!("{} — {}", sh.label, sh.status));
                let size = window.inner_size();
                let (Some(dw), Some(dh)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else { return };
                if surface.resize(dw, dh).is_err() {
                    return;
                }
                let Ok(mut buf) = surface.buffer_mut() else { return };
                draw_fit(&mut buf, size.width as usize, size.height as usize, &sh.pix, sh.w, sh.h);
                let _ = buf.present();
            }
            _ => {}
        }
    }
}

/// Where the remote screen sits in the window: x, y, width, height (zero size before a frame).
fn fit(dw: usize, dh: usize, sw: usize, sh: usize) -> (usize, usize, usize, usize) {
    if sw == 0 || sh == 0 || dw == 0 || dh == 0 {
        return (0, 0, 0, 0);
    }
    let scale = (dw as f64 / sw as f64).min(dh as f64 / sh as f64);
    let (w, h) = (((sw as f64 * scale) as usize).clamp(1, dw), ((sh as f64 * scale) as usize).clamp(1, dh));
    ((dw - w) / 2, (dh - h) / 2, w, h)
}

/// Scales the remote screen to fit, keeping its aspect ratio, into 0RGB pixels.
/// shortcut: nearest-neighbour scaling on the CPU; switch to a GPU blit if large screens stutter.
fn draw_fit(dst: &mut [u32], dw: usize, dh: usize, src: &[u8], sw: usize, sh: usize) {
    dst.fill(0);
    if sw == 0 || sh == 0 || src.len() < sw * sh * 4 {
        return;
    }
    let (ox, oy, w, h) = fit(dw, dh, sw, sh);
    let xs: Vec<usize> = (0..w).map(|x| (x * sw / w) * 4).collect();
    for y in 0..h {
        let row = &src[(y * sh / h) * sw * 4..];
        let out = &mut dst[(oy + y) * dw + ox..][..w];
        for (o, &sx) in out.iter_mut().zip(&xs) {
            *o = (row[sx] as u32) << 16 | (row[sx + 1] as u32) << 8 | row[sx + 2] as u32;
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn draw_fit_letterboxes_and_converts() {
        // 2x1 red/blue into a 4x4 window: scaled to 4x2, centred vertically.
        let mut dst = vec![7u32; 16];
        super::draw_fit(&mut dst, 4, 4, &[255, 0, 0, 255, 0, 0, 255, 255], 2, 1);
        let (r, b) = (0xff0000, 0x0000ff);
        assert_eq!(dst, [0, 0, 0, 0, r, r, b, b, r, r, b, b, 0, 0, 0, 0]);
    }
}
