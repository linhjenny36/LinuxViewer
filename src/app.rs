//! The window's contents, styled after MacViewer: a Connect card with recent
//! computers, then the remote screen under a toolbar.

use crate::audio::Player;
use crate::input::Mouse;
use crate::profile;
use crate::session::{self, Config, Event};
use egui::{
    vec2, Align, Button, CentralPanel, Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily,
    FontId, Frame, Layout, Margin, Panel, Rect, RichText, ScrollArea, Sense, Stroke, TextEdit, TextureHandle,
    TextureOptions, Ui,
};
use std::collections::HashMap;
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::PhysicalKey;

struct Palette {
    background: Color32,
    card: Color32,
    border: Color32,
    header: Color32,
    text: Color32,
    muted: Color32,
    accent: Color32,
    accent_soft: Color32,
    danger: Color32,
    screen: Color32,
}

fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            background: Color32::from_rgb(30, 30, 32), card: Color32::from_rgb(44, 44, 46),
            border: Color32::from_rgb(64, 64, 68), header: Color32::from_rgb(38, 38, 40),
            text: Color32::from_rgb(242, 242, 247), muted: Color32::from_rgb(152, 152, 159),
            accent: Color32::from_rgb(64, 140, 255), accent_soft: Color32::from_rgb(28, 52, 92),
            danger: Color32::from_rgb(255, 99, 88), screen: Color32::from_rgb(18, 18, 20),
        }
    } else {
        Palette {
            background: Color32::from_rgb(236, 236, 236), card: Color32::WHITE,
            border: Color32::from_rgb(220, 220, 222), header: Color32::from_rgb(244, 244, 245),
            text: Color32::from_rgb(29, 29, 31), muted: Color32::from_rgb(110, 110, 115),
            accent: Color32::from_rgb(47, 124, 246), accent_soft: Color32::from_rgb(226, 238, 255),
            danger: Color32::from_rgb(215, 52, 42), screen: Color32::from_rgb(28, 28, 30),
        }
    }
}

fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

/// What the session thread reports to the window.
#[derive(Default)]
struct Shared {
    pixels: Vec<u8>, // top-down RGBA
    width: usize,
    height: usize,
    generation: u64,
    status: String,
    peer: Option<String>,
    /// Set when the session thread finishes: its error, or empty after Disconnect.
    ended: Option<String>,
}

/// Lets a session callback change `Shared` and wake the window.
#[derive(Clone)]
struct Notify {
    shared: Arc<Mutex<Shared>>,
    ctx: Context,
}

impl Notify {
    fn update(&self, f: impl FnOnce(&mut Shared)) {
        f(&mut self.shared.lock().unwrap());
        self.ctx.request_repaint();
    }
}

/// Where the remote screen was drawn, in points, and how big it is in pixels.
#[derive(Clone, Copy)]
struct View {
    rect: Rect,
    visible: Rect,
    width: usize,
    height: usize,
}

/// A running session and its window state.
struct Remote {
    id: String,
    password: String,
    remember: bool,
    tx: SyncSender<Event>,
    notify: Notify,
    worker: JoinHandle<()>,
    texture: Option<TextureHandle>,
    shown: u64,
    size: (usize, usize),
    accepted: bool,
    peer: String,
    status: String,
    fit: bool,
    sound: bool,
    closing: bool,
    view: Option<View>,
    pointer_inside: bool,
    buttons_held: u8,
    /// Unsent fraction of a wheel notch (touchpads scroll in pixels).
    wheel_rest: f64,
}

pub struct App {
    id: String,
    password: String,
    remember: bool,
    message: Option<String>,
    recents: Vec<String>,
    /// Passwords found in the keyring for recent IDs.
    saved: Arc<Mutex<HashMap<String, String>>>,
    remote: Option<Remote>,
    player: Arc<Mutex<Option<Player>>>,
}

impl App {
    pub fn new(id: String) -> Self {
        App {
            id, password: String::new(), remember: true, message: None, recents: profile::load_recents(),
            saved: Arc::default(), remote: None, player: Arc::default(),
        }
    }

    /// Fonts, style, and loading saved passwords; needs the egui context.
    pub fn setup(&mut self, ctx: &Context) {
        load_fonts(ctx);
        // Follow the desktop's light/dark setting; when it doesn't say, use MacViewer's light look.
        ctx.options_mut(|o| o.fallback_theme = egui::Theme::Light);
        ctx.all_styles_mut(|s| {
            s.spacing.item_spacing = vec2(8.0, 8.0);
            s.spacing.button_padding = vec2(12.0, 6.0);
            s.spacing.interact_size.y = 30.0;
            for (style, size) in [(egui::TextStyle::Body, 14.0), (egui::TextStyle::Button, 14.0), (egui::TextStyle::Small, 12.0)] {
                s.text_styles.insert(style, FontId::proportional(size));
            }
            let radius = CornerRadius::same(7);
            for w in [&mut s.visuals.widgets.inactive, &mut s.visuals.widgets.hovered, &mut s.visuals.widgets.active,
                      &mut s.visuals.widgets.open, &mut s.visuals.widgets.noninteractive] {
                w.corner_radius = radius;
            }
        });
        // Outlined fields and buttons, as in MacViewer.
        for (theme, outline, field) in [
            (egui::Theme::Light, Color32::from_rgb(208, 208, 213), Color32::from_rgb(250, 250, 251)),
            (egui::Theme::Dark, Color32::from_rgb(72, 72, 76), Color32::from_rgb(28, 28, 30)),
        ] {
            ctx.style_mut_of(theme, |s| {
                s.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, outline);
                s.visuals.extreme_bg_color = field;
            });
        }
        let (ids, saved, ctx) = (self.recents.clone(), self.saved.clone(), ctx.clone());
        thread::spawn(move || {
            for id in ids {
                if let Some(p) = profile::saved_password(&id) {
                    saved.lock().unwrap().insert(id, p);
                }
            }
            ctx.request_repaint();
        });
    }

    pub fn title(&self) -> String {
        match &self.remote {
            Some(r) if r.accepted => format!("{} ({}) — LinuxViewer", r.peer, group_id(&r.id)),
            _ => "LinuxViewer".into(),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        self.poll();
        let pal = palette(ui.visuals().dark_mode);
        match self.remote.as_mut() {
            Some(r) if r.texture.is_some() => session_view(ui, r, &pal, &self.player),
            Some(r) => connecting_view(ui, r, &pal),
            None => self.home_view(ui, &pal),
        }
    }

    /// Ends a running session when the window closes, giving it time to tell Windows.
    pub fn shutdown(&mut self) {
        if let Some(r) = self.remote.take() {
            let _ = r.tx.send(Event::Quit);
            let start = Instant::now();
            while !r.worker.is_finished() && start.elapsed() < Duration::from_secs(2) {
                thread::sleep(Duration::from_millis(20));
            }
        }
    }

    /// Sends input over the remote screen to Windows. Returns true when egui must not
    /// see the event: keys belong to Windows, so Tab or Enter can't press a toolbar button.
    pub fn forward_input(&mut self, event: &WindowEvent, pixels_per_point: f32) -> bool {
        let Some(r) = self.remote.as_mut() else { return false };
        let Some(view) = r.view else { return false };
        if r.closing {
            return false;
        }
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let p = egui::pos2(position.x as f32, position.y as f32) / pixels_per_point;
                r.pointer_inside = view.visible.contains(p);
                if r.pointer_inside || r.buttons_held > 0 {
                    let x = ((p.x - view.rect.min.x) * view.width as f32 / view.rect.width()).floor() as i32;
                    let y = ((p.y - view.rect.min.y) * view.height as f32 / view.rect.height()).floor() as i32;
                    let _ = r.tx.try_send(Event::Mouse(Mouse::Move { x, y })); // a dropped move is harmless
                }
                false
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let button = match button {
                    MouseButton::Left => 0,
                    MouseButton::Right => 1,
                    MouseButton::Middle => 2,
                    _ => return false,
                };
                let pressed = *state == ElementState::Pressed;
                // Presses on the toolbar stay local; releases always go, so nothing sticks down.
                if pressed && !r.pointer_inside {
                    return false;
                }
                r.buttons_held = if pressed { r.buttons_held + 1 } else { r.buttons_held.saturating_sub(1) };
                let _ = r.tx.send(Event::Mouse(Mouse::Button { button, pressed }));
                false
            }
            WindowEvent::MouseWheel { delta, .. } if r.pointer_inside => {
                // shortcut: 40 touchpad pixels per notch; tune if touchpad scrolling feels off.
                r.wheel_rest += match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y as f64,
                    MouseScrollDelta::PixelDelta(p) => p.y / 40.0,
                };
                let notches = r.wheel_rest.trunc();
                r.wheel_rest -= notches;
                if notches != 0.0 {
                    let _ = r.tx.send(Event::Mouse(Mouse::Wheel(notches as i32)));
                }
                false
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    let _ = r.tx.send(Event::Key(code, event.state == ElementState::Pressed));
                }
                true
            }
            WindowEvent::Focused(false) => {
                r.buttons_held = 0;
                let _ = r.tx.send(Event::ReleaseAll);
                false
            }
            _ => false,
        }
    }

    /// Takes the session thread's news: a new frame, acceptance, or the end.
    fn poll(&mut self) {
        let Some(r) = self.remote.as_mut() else { return };
        let mut sh = r.notify.shared.lock().unwrap();
        r.status = sh.status.clone();
        if sh.generation != r.shown && sh.width > 0 {
            let image = egui::ColorImage::from_rgba_unmultiplied([sh.width, sh.height], &sh.pixels);
            let options = if r.fit { TextureOptions::LINEAR } else { TextureOptions::NEAREST };
            match &mut r.texture {
                Some(t) => t.set(image, options),
                None => r.texture = Some(r.notify.ctx.load_texture("remote-screen", image, options)),
            }
            (r.shown, r.size) = (sh.generation, (sh.width, sh.height));
        }
        if let (Some(peer), false) = (&sh.peer, r.accepted) {
            r.accepted = true;
            r.peer = peer.clone();
            self.recents = profile::add_recent(&r.id);
            let (id, password, remember, saved, notify) = (r.id.clone(), r.password.clone(), r.remember, self.saved.clone(), r.notify.clone());
            thread::spawn(move || {
                if !remember {
                    profile::forget_password(&id);
                    saved.lock().unwrap().remove(&id);
                    return;
                }
                match profile::save_password(&id, &password) {
                    Ok(()) => drop(saved.lock().unwrap().insert(id, password)),
                    Err(e) => notify.update(|s| s.status = e.to_string()),
                }
            });
        }
        let ended = sh.ended.take();
        drop(sh);
        if let Some(err) = ended {
            self.message = (!err.is_empty()).then_some(err);
            self.remote = None;
        }
    }

    fn connect(&mut self, ctx: &Context) {
        let id: String = self.id.chars().filter(|c| !c.is_whitespace()).collect();
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
            self.message = Some("Enter the partner's UltraViewer ID (digits only).".into());
            return;
        }
        if self.password.is_empty() {
            self.message = Some("Enter the partner's password.".into());
            return;
        }
        let registration = match profile::load_registration() {
            Ok(r) => r,
            Err(e) => {
                self.message = Some(e.to_string());
                return;
            }
        };
        self.message = None;
        let notify = Notify { shared: Arc::new(Mutex::new(Shared { status: "Connecting…".into(), ..Default::default() })), ctx: ctx.clone() };
        let (status, frame, peer, player) = (notify.clone(), notify.clone(), notify.clone(), self.player.clone());
        // shortcut: reports a fixed 1920x1080 viewer screen in Login; read the real monitor if Windows scales by it.
        let cfg = Config {
            registration, screen_width: 1920, screen_height: 1080, display_name: "LinuxViewer".into(),
            on_status: Box::new(move |s| status.update(|sh| sh.status = s.into())),
            on_frame: Box::new(move |px, w, h| frame.update(|sh| {
                sh.pixels.clear();
                sh.pixels.extend_from_slice(px);
                (sh.width, sh.height) = (w, h);
                sh.generation += 1;
            })),
            on_peer_changed: Box::new(move |name| peer.update(|sh| sh.peer = Some(name.into()))),
            sound: false,
            on_audio: Box::new(move |pcm| {
                if let Some(p) = player.lock().unwrap().as_ref() {
                    p.play(pcm);
                }
            }),
        };
        let (tx, rx) = sync_channel(64);
        let (session_tx, done, password) = (tx.clone(), notify.clone(), self.password.clone());
        let session_id = id.clone();
        let worker = thread::spawn(move || {
            let result = session::run(cfg, &session_id, &password, session_tx, rx, &session::dial);
            done.update(|sh| sh.ended = Some(result.err().map(|e| e.to_string()).unwrap_or_default()));
        });
        self.remote = Some(Remote {
            id, password: self.password.clone(), remember: self.remember, tx, notify, worker, texture: None, shown: 0,
            size: (0, 0), accepted: false, peer: String::new(), status: String::new(), fit: true, sound: false,
            closing: false, view: None, pointer_inside: false, buttons_held: 0, wheel_rest: 0.0,
        });
    }

    fn home_view(&mut self, ui: &mut Ui, pal: &Palette) {
        Panel::top("header")
            .frame(Frame::new().fill(pal.header).inner_margin(Margin::symmetric(20, 12)).stroke(Stroke::new(1.0, pal.border)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    app_icon(ui, 40.0, pal);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.label(RichText::new("LinuxViewer").font(bold(17.0)).color(pal.text));
                        ui.label(RichText::new("Works with UltraViewer").size(12.5).color(pal.muted));
                    });
                });
            });
        CentralPanel::default().frame(Frame::new().fill(pal.background)).show(ui, |ui| {
            ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                let width = (ui.available_width() - 32.0).min(760.0);
                ui.add_space(28.0);
                ui.horizontal(|ui| {
                    ui.add_space((ui.available_width() - width) / 2.0);
                    ui.vertical(|ui| {
                    ui.set_max_width(width);
                    ui.label(RichText::new("Connect").font(bold(28.0)).color(pal.text));
                    ui.label(RichText::new("Control a Windows computer running UltraViewer.").size(15.0).color(pal.muted));
                    ui.add_space(14.0);
                    self.connect_card(ui, pal, width);
                    ui.add_space(22.0);
                    self.recents_section(ui, pal, width);
                    ui.add_space(28.0);
                    });
                });
            });
        });
    }

    fn connect_card(&mut self, ui: &mut Ui, pal: &Palette, width: f32) {
        let mut connect = false;
        card(pal).show(ui, |ui| {
            ui.set_width(width);
            Frame::new().inner_margin(Margin::same(20)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    monitor_icon(ui, 40.0, pal);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.label(RichText::new("CONNECT").font(bold(11.5)).color(pal.accent).extra_letter_spacing(1.2));
                        ui.label(RichText::new("Connect to another computer").font(bold(16.0)).color(pal.text));
                    });
                });
            });
            divider(ui, pal);
            Frame::new().inner_margin(Margin::same(20)).show(ui, |ui| {
                let field_width = ui.available_width();
                ui.label(RichText::new("Partner ID").font(bold(14.0)).color(pal.text));
                let id = ui.add(TextEdit::singleline(&mut self.id).hint_text("Partner's UltraViewer ID")
                    .font(FontId::monospace(17.0)).desired_width(field_width).margin(vec2(10.0, 8.0)));
                ui.add_space(6.0);
                ui.label(RichText::new("Partner password").font(bold(14.0)).color(pal.text));
                let pw = ui.add(TextEdit::singleline(&mut self.password).password(true).hint_text("Partner's password")
                    .desired_width(field_width).margin(vec2(10.0, 8.0)));
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                connect |= enter && (id.lost_focus() || pw.lost_focus());
                ui.add_space(4.0);
                ui.checkbox(&mut self.remember, "Remember password on this computer");
                ui.label(RichText::new("Successful connection passwords are saved in the system keyring.").size(12.0).color(pal.muted));
                if let Some(m) = &self.message {
                    ui.add_space(4.0);
                    ui.label(RichText::new(m).color(pal.danger));
                }
            });
            divider(ui, pal);
            Frame::new().inner_margin(Margin::same(20)).show(ui, |ui| {
                let ready = !self.id.trim().is_empty() && !self.password.is_empty();
                let text = RichText::new("Connect to another computer").size(15.0)
                    .color(if ready { Color32::WHITE } else { pal.muted });
                let button = Button::new(text).fill(if ready { pal.accent } else { pal.background })
                    .min_size(vec2(ui.available_width(), 42.0)).corner_radius(9);
                connect |= ui.add_enabled(ready, button).clicked();
                ui.label(RichText::new("Mouse and keyboard work while the remote screen has focus.").size(12.0).color(pal.muted));
            });
        });
        if connect {
            self.connect(ui.ctx());
        }
    }

    fn recents_section(&mut self, ui: &mut Ui, pal: &Palette, width: f32) {
        if self.recents.is_empty() {
            return;
        }
        ui.horizontal(|ui| {
            ui.set_width(width);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new("Recent computers").font(bold(17.0)).color(pal.text));
                ui.label(RichText::new("Choose a computer to reuse its connection details.").size(13.0).color(pal.muted));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(Button::new(RichText::new("Clear history").size(13.0).color(pal.muted)).frame(false)).clicked() {
                    thread::spawn(profile::clear_recents);
                    self.recents.clear();
                    self.saved.lock().unwrap().clear();
                }
            });
        });
        ui.add_space(6.0);
        let saved = self.saved.lock().unwrap().clone();
        let mut chosen = None;
        card(pal).show(ui, |ui| {
            ui.set_width(width);
            for (i, id) in self.recents.iter().enumerate() {
                if i > 0 {
                    divider(ui, pal);
                }
                let row = Frame::new().inner_margin(Margin::symmetric(18, 12)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        monitor_icon(ui, 40.0, pal);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.label(RichText::new(group_id(id)).font(FontId::monospace(19.0)).color(pal.text));
                            let note = if saved.contains_key(id) { "Remote computer · password saved" } else { "Remote computer" };
                            ui.label(RichText::new(note).size(12.5).color(pal.muted));
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new("›").size(20.0).color(pal.muted));
                            ui.label(RichText::new("Select").font(bold(14.0)).color(pal.accent));
                        });
                    });
                });
                let row = ui.interact(row.response.rect, ui.scope_id().with(("recent", i)), Sense::click());
                if row.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if row.clicked() {
                    chosen = Some(id.clone());
                }
            }
        });
        if let Some(id) = chosen {
            self.password = saved.get(&id).cloned().unwrap_or_default();
            self.remember = saved.contains_key(&id) || self.remember;
            self.id = id;
            self.message = None;
        }
    }
}

fn connecting_view(ui: &mut Ui, r: &mut Remote, pal: &Palette) {
    CentralPanel::default().frame(Frame::new().fill(pal.background)).show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() / 2.0 - 110.0).max(0.0));
            monitor_icon(ui, 56.0, pal);
            ui.label(RichText::new(group_id(&r.id)).font(FontId::monospace(22.0)).color(pal.text));
            ui.spinner();
            ui.label(RichText::new(&r.status).size(14.0).color(pal.muted));
            ui.add_space(6.0);
            if ui.add_enabled(!r.closing, Button::new("Cancel").min_size(vec2(110.0, 32.0))).clicked() {
                r.closing = true;
                let _ = r.tx.send(Event::Quit);
            }
        });
    });
}

fn session_view(ui: &mut Ui, r: &mut Remote, pal: &Palette, player: &Mutex<Option<Player>>) {
    Panel::top("toolbar")
        .frame(Frame::new().fill(pal.header).inner_margin(Margin::symmetric(14, 8)).stroke(Stroke::new(1.0, pal.border)))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                monitor_icon(ui, 30.0, pal);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(RichText::new(&r.peer).font(bold(14.0)).color(pal.text));
                    ui.label(RichText::new(&r.status).size(12.0).color(pal.muted));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let disconnect = Button::new(RichText::new("Disconnect").color(pal.danger)).min_size(vec2(0.0, 30.0));
                    if ui.add_enabled(!r.closing, disconnect).clicked() {
                        r.closing = true;
                        let _ = r.tx.send(Event::Quit);
                    }
                    ui.separator();
                    let fit = r.fit;
                    ui.selectable_value(&mut r.fit, false, "Actual size");
                    ui.selectable_value(&mut r.fit, true, "Fit");
                    if r.fit != fit {
                        r.shown = 0; // re-upload with the matching filter
                    }
                    ui.separator();
                    let label = if r.sound { "🔊 Sound on" } else { "🔈 Sound off" };
                    if ui.toggle_value(&mut r.sound, label).changed() {
                        set_sound(r, player);
                    }
                });
            });
        });
    CentralPanel::default().frame(Frame::new().fill(pal.screen)).show(ui, |ui| {
        let Some(texture) = &r.texture else { return };
        let (w, h) = r.size;
        let area = ui.available_rect_before_wrap();
        if r.fit {
            let scale = (area.width() / w as f32).min(area.height() / h as f32);
            let rect = Rect::from_center_size(area.center(), vec2(w as f32 * scale, h as f32 * scale));
            ui.painter().image(texture.id(), rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            r.view = Some(View { rect, visible: rect.intersect(area), width: w, height: h });
        } else {
            // Wheel events belong to Windows, so the view only scrolls by its scroll bars.
            ScrollArea::both().scroll_source(egui::scroll_area::ScrollSource::SCROLL_BAR).auto_shrink(false).show(ui, |ui| {
                let size = vec2(w as f32, h as f32) / ui.ctx().pixels_per_point();
                let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                ui.painter().image(texture.id(), rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                r.view = Some(View { rect, visible: rect.intersect(ui.clip_rect()), width: w, height: h });
            });
        }
    });
}

/// Starts the audio player on first use, then asks Windows to start or stop sending sound.
fn set_sound(r: &mut Remote, player: &Mutex<Option<Player>>) {
    let mut p = player.lock().unwrap();
    if r.sound && p.is_none() {
        match Player::start() {
            Ok(started) => *p = Some(started),
            Err(e) => {
                r.sound = false;
                r.notify.update(|s| s.status = format!("Sound is unavailable: {e}"));
                return;
            }
        }
    }
    let _ = r.tx.send(Event::Sound(r.sound));
}

fn card(pal: &Palette) -> Frame {
    Frame::new().fill(pal.card).stroke(Stroke::new(1.0, pal.border)).corner_radius(14)
}

fn divider(ui: &mut Ui, pal: &Palette) {
    let rect = ui.available_rect_before_wrap();
    let y = rect.top();
    ui.painter().hline(rect.x_range(), y, Stroke::new(1.0, pal.border));
    ui.add_space(1.0);
}

/// A rounded tile with a small monitor, like MacViewer's card icons.
fn monitor_icon(ui: &mut Ui, size: f32, pal: &Palette) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same((size * 0.24) as u8), pal.accent_soft);
    draw_monitor(p, rect, pal.accent, pal.accent_soft);
}

/// The app icon: a white monitor on the accent colour.
fn app_icon(ui: &mut Ui, size: f32, pal: &Palette) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same((size * 0.24) as u8), pal.accent);
    draw_monitor(p, rect, Color32::WHITE, pal.accent);
}

fn draw_monitor(p: &egui::Painter, tile: Rect, ink: Color32, paper: Color32) {
    let s = tile.width();
    let screen = Rect::from_center_size(tile.center() - vec2(0.0, s * 0.06), vec2(s * 0.56, s * 0.38));
    p.rect_filled(screen, CornerRadius::same((s * 0.05) as u8), ink);
    p.rect_filled(screen.shrink(s * 0.045), CornerRadius::same(1), paper);
    let foot = egui::pos2(tile.center().x, screen.bottom() + s * 0.12);
    p.line_segment([egui::pos2(foot.x, screen.bottom()), foot], Stroke::new(s * 0.05, ink));
    p.line_segment([foot - vec2(s * 0.13, 0.0), foot + vec2(s * 0.13, 0.0)], Stroke::new(s * 0.05, ink));
}

/// "123456789" → "123 456 789", as UltraViewer shows IDs.
fn group_id(id: &str) -> String {
    id.as_bytes().chunks(3).map(|c| std::str::from_utf8(c).unwrap_or("")).collect::<Vec<_>>().join(" ")
}

/// Uses the desktop's Noto Sans or DejaVu Sans (they cover Vietnamese and have a
/// bold face); falls back to egui's built-in font.
/// shortcut: fixed font paths for Debian/Ubuntu/Fedora/Arch; ask fontconfig if a distro misses them.
fn load_fonts(ctx: &Context) {
    const FACES: [(&str, &str); 6] = [
        ("/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf", "/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf"),
        ("/usr/share/fonts/noto/NotoSans-Regular.ttf", "/usr/share/fonts/noto/NotoSans-Bold.ttf"),
        ("/usr/share/fonts/google-noto/NotoSans-Regular.ttf", "/usr/share/fonts/google-noto/NotoSans-Bold.ttf"),
        ("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"),
        ("/usr/share/fonts/TTF/DejaVuSans.ttf", "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf"),
        ("/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf", "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans-Bold.ttf"),
    ];
    let mut defs = FontDefinitions::default();
    let fallback = defs.families[&FontFamily::Proportional].clone();
    let mut bold_family = fallback.clone();
    if let Some((regular, bold)) = FACES.iter().find_map(|(r, b)| Some((std::fs::read(r).ok()?, std::fs::read(b).ok()?))) {
        defs.font_data.insert("ui".into(), Arc::new(FontData::from_owned(regular)));
        defs.font_data.insert("ui-bold".into(), Arc::new(FontData::from_owned(bold)));
        defs.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "ui".into());
        bold_family.insert(0, "ui-bold".into());
    }
    defs.families.insert(FontFamily::Name("bold".into()), bold_family);
    ctx.set_fonts(defs);
}

#[cfg(test)]
mod tests {
    #[test]
    fn ids_group_in_threes() {
        assert_eq!(super::group_id("123456789"), "123 456 789");
        assert_eq!(super::group_id("1234"), "123 4");
    }
}
