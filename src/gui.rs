//! The window: winit plus an OpenGL context (glutin) that egui draws into.
//! uvview owns the event loop instead of using eframe, so raw physical key
//! events can go to Windows before egui sees them.

use crate::app::App;
use egui_glow::glow::{self, HasContext};
use egui_glow::EguiGlow;
use glutin::config::ConfigTemplateBuilder;
use glutin::context::{ContextApi, ContextAttributesBuilder, NotCurrentGlContext, PossiblyCurrentContext};
use glutin::display::{GetGlDisplay, GlDisplay};
use glutin::surface::{GlSurface, Surface, SwapInterval, WindowSurface};
use glutin_winit::{DisplayBuilder, GlWindow};
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::raw_window_handle::HasWindowHandle;
use winit::window::{Window, WindowId};

type Error = Box<dyn std::error::Error>;

struct Gl {
    window: Window,
    surface: Surface<WindowSurface>,
    context: PossiblyCurrentContext,
    egui: EguiGlow,
    title: String,
}

struct Host {
    app: App,
    proxy: EventLoopProxy<Duration>,
    gl: Option<Gl>,
    /// When egui asked to be redrawn next (it sends delays through the proxy).
    next_repaint: Option<Instant>,
    error: Option<Error>,
}

pub fn run(app: App) -> Result<(), Error> {
    let event_loop = EventLoop::<Duration>::with_user_event().build()?;
    let mut host = Host { app, proxy: event_loop.create_proxy(), gl: None, next_repaint: None, error: None };
    event_loop.run_app(&mut host)?;
    host.app.shutdown();
    if let Some(mut gl) = host.gl.take() {
        gl.egui.destroy();
    }
    host.error.map_or(Ok(()), Err)
}

fn create_gl(el: &ActiveEventLoop, proxy: EventLoopProxy<Duration>) -> Result<Gl, Error> {
    let attrs = Window::default_attributes()
        .with_title("LinuxViewer")
        .with_inner_size(LogicalSize::new(1000.0, 760.0))
        .with_min_inner_size(LogicalSize::new(560.0, 480.0));
    let (window, config) = DisplayBuilder::new()
        .with_window_attributes(Some(attrs))
        .build(el, ConfigTemplateBuilder::new(), |mut configs| configs.next().expect("an OpenGL config"))?;
    let window = window.ok_or("could not create the window")?;
    let display = config.display();
    let handle = window.window_handle()?.as_raw();
    // Desktop OpenGL first, then OpenGL ES (some Wayland and ARM drivers offer only that).
    let context = unsafe {
        display.create_context(&config, &ContextAttributesBuilder::new().build(Some(handle))).or_else(|_| {
            display.create_context(&config, &ContextAttributesBuilder::new().with_context_api(ContextApi::Gles(None)).build(Some(handle)))
        })?
    };
    let surface_attrs = window.build_surface_attributes(Default::default())?;
    let surface = unsafe { display.create_window_surface(&config, &surface_attrs)? };
    let context = context.make_current(&surface)?;
    let _ = surface.set_swap_interval(&context, SwapInterval::Wait(NonZeroU32::MIN));
    let gl = unsafe { glow::Context::from_loader_function_cstr(|s| display.get_proc_address(s)) };
    let egui = EguiGlow::new(el, Arc::new(gl), None, Some(window.scale_factor() as f32), true);
    egui.egui_ctx.set_request_repaint_callback(move |info| {
        let _ = proxy.send_event(info.delay);
    });
    Ok(Gl { window, surface, context, egui, title: String::new() })
}

impl ApplicationHandler<Duration> for Host {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.gl.is_some() {
            return;
        }
        match create_gl(el, self.proxy.clone()) {
            Ok(gl) => {
                self.app.setup(&gl.egui.egui_ctx);
                gl.window.request_redraw();
                self.gl = Some(gl);
            }
            Err(e) => {
                self.error = Some(format!("could not open the window: {e}").into());
                el.exit();
            }
        }
    }

    fn user_event(&mut self, _: &ActiveEventLoop, delay: Duration) {
        let at = Instant::now() + delay;
        self.next_repaint = Some(self.next_repaint.map_or(at, |t| t.min(at)));
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        match self.next_repaint {
            Some(t) if t <= Instant::now() => {
                self.next_repaint = None;
                if let Some(gl) = &self.gl {
                    gl.window.request_redraw();
                }
                el.set_control_flow(ControlFlow::Wait);
            }
            Some(t) => el.set_control_flow(ControlFlow::WaitUntil(t)),
            None => el.set_control_flow(ControlFlow::Wait),
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(gl) = &mut self.gl else { return };
        match &event {
            WindowEvent::CloseRequested => {
                el.exit();
                return;
            }
            WindowEvent::Resized(size) => {
                if let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) {
                    gl.surface.resize(&gl.context, w, h);
                }
            }
            WindowEvent::RedrawRequested => {
                let app = &mut self.app;
                gl.egui.run(&gl.window, |ui| app.ui(ui));
                let title = app.title();
                if title != gl.title {
                    gl.window.set_title(&title);
                    gl.title = title;
                }
                unsafe {
                    let g = gl.egui.painter.gl();
                    g.clear_color(0.0, 0.0, 0.0, 1.0);
                    g.clear(glow::COLOR_BUFFER_BIT);
                }
                gl.egui.paint(&gl.window);
                let _ = gl.surface.swap_buffers(&gl.context);
                return;
            }
            _ => {}
        }
        let pixels_per_point = gl.egui.egui_ctx.pixels_per_point();
        if self.app.forward_input(&event, pixels_per_point) {
            return;
        }
        if gl.egui.on_window_event(&gl.window, &event).repaint {
            gl.window.request_redraw();
        }
    }
}
