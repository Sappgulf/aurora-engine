//! Application trait and cross-platform runner (native + WASM).

use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::color::Color;
use crate::renderer::Renderer;
use crate::time::Time;

/// Implement this for your game / demo.
pub trait Game: 'static {
    fn name(&self) -> &str {
        "Aurora Engine"
    }

    /// Called once after the GPU renderer is ready.
    fn on_start(&mut self, _renderer: &mut Renderer) {}

    /// Called every frame before render.
    fn on_update(&mut self, time: &Time, renderer: &mut Renderer);

    /// Optional: handle raw window events (keys, mouse, etc.).
    /// Return `true` if the event was consumed.
    fn on_event(&mut self, _event: &WindowEvent) -> bool {
        false
    }
}

enum UserEvent {
    RendererReady(Renderer),
}

/// Launch a game on the current platform (desktop window or browser canvas).
pub fn run<G: Game>(game: G) {
    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            init_logging_wasm();
        } else {
            init_logging_native();
        }
    }

    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .expect("failed to create event loop");
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);

    let proxy = event_loop.create_proxy();

    let app = EngineApp {
        game: Some(game),
        window: None,
        renderer: None,
        time: Time::new(),
        proxy,
        init_started: false,
    };

    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            use winit::platform::web::EventLoopExtWebSys;
            event_loop.spawn_app(app);
        } else {
            let mut app = app;
            event_loop
                .run_app(&mut app)
                .expect("event loop terminated with error");
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn init_logging_native() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init();
}

#[cfg(target_arch = "wasm32")]
fn init_logging_wasm() {
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));
    let _ = console_log::init_with_level(log::Level::Info);
}

struct EngineApp<G: Game> {
    game: Option<G>,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    time: Time,
    proxy: EventLoopProxy<UserEvent>,
    init_started: bool,
}

impl<G: Game> EngineApp<G> {
    fn start_renderer(&mut self, window: Arc<Window>) {
        if self.init_started {
            return;
        }
        self.init_started = true;
        log::info!("Creating Aurora renderer…");

        #[cfg(not(target_arch = "wasm32"))]
        {
            let renderer = pollster::block_on(Renderer::new(window));
            let _ = self.proxy.send_event(UserEvent::RendererReady(renderer));
        }

        #[cfg(target_arch = "wasm32")]
        {
            let proxy = self.proxy.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let renderer = Renderer::new(window).await;
                let _ = proxy.send_event(UserEvent::RendererReady(renderer));
            });
        }
    }
}

impl<G: Game> ApplicationHandler<UserEvent> for EngineApp<G> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let title = self
            .game
            .as_ref()
            .map(|g| g.name().to_string())
            .unwrap_or_else(|| "Aurora Engine".into());

        let window_attrs = {
            let attrs = Window::default_attributes()
                .with_title(title)
                .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));

            #[cfg(target_arch = "wasm32")]
            {
                use wasm_bindgen::JsCast;
                use winit::platform::web::WindowAttributesExtWebSys;

                if let Some(canvas) = web_sys::window()
                    .and_then(|w| w.document())
                    .and_then(|d| d.get_element_by_id("aurora-canvas"))
                    .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
                {
                    attrs.with_canvas(Some(canvas))
                } else {
                    attrs
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                attrs
            }
        };

        let window = Arc::new(
            event_loop
                .create_window(window_attrs)
                .expect("failed to create window"),
        );

        #[cfg(target_arch = "wasm32")]
        {
            use winit::platform::web::WindowExtWebSys;
            if let Some(canvas) = window.canvas() {
                canvas.set_width(1280);
                canvas.set_height(720);
                let style = canvas.style();
                let _ = style.set_property("width", "100%");
                let _ = style.set_property("max-width", "1280px");
                let _ = style.set_property("aspect-ratio", "16 / 9");
                let _ = style.set_property("display", "block");
                let _ = style.set_property("margin", "0 auto");
                let _ = style.set_property("background", "#0a0d1a");
            }
        }

        self.start_renderer(window.clone());
        self.window = Some(window);
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::RendererReady(mut renderer) => {
                if let Some(game) = self.game.as_mut() {
                    game.on_start(&mut renderer);
                }
                self.renderer = Some(renderer);
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
                log::info!("Aurora renderer ready — entering main loop");
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        if let Some(game) = self.game.as_mut() {
            if game.on_event(&event) {
                return;
            }
        }

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state.is_pressed()
                    && matches!(event.logical_key, Key::Named(NamedKey::Escape)) =>
            {
                event_loop.exit();
            }
            WindowEvent::Resized(physical_size) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(physical_size);
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                let (Some(renderer), Some(game), Some(window)) = (
                    self.renderer.as_mut(),
                    self.game.as_mut(),
                    self.window.as_ref(),
                ) else {
                    return;
                };

                self.time.tick();
                game.on_update(&self.time, renderer);

                match renderer.render(self.time.elapsed) {
                    Ok(()) => {}
                    Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                        renderer.resize(window.inner_size());
                    }
                    Err(wgpu::SurfaceError::OutOfMemory) => {
                        log::error!("GPU out of memory");
                        event_loop.exit();
                    }
                    Err(wgpu::SurfaceError::Timeout) => {
                        log::warn!("Surface timeout");
                    }
                    Err(wgpu::SurfaceError::Other) => {
                        log::warn!("Surface error (other)");
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if self.renderer.is_some() {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }
}

/// Built-in smoke-test game used by examples.
pub struct TriangleDemo {
    pub title: String,
}

impl Default for TriangleDemo {
    fn default() -> Self {
        Self {
            title: "Aurora Engine — Triangle Demo".into(),
        }
    }
}

impl Game for TriangleDemo {
    fn name(&self) -> &str {
        &self.title
    }

    fn on_update(&mut self, time: &Time, renderer: &mut Renderer) {
        let hue = (time.elapsed * 0.05) % 1.0;
        let clear = Color::from_hue(hue).night_blend(0.82);
        renderer.set_clear_color(clear);
    }
}
