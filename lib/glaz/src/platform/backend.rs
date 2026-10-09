use std::collections::VecDeque;
use std::future::Future;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll, Wake, Waker};
use std::time::Instant;

impl From<winit::error::OsError> for Error {
    fn from(e: winit::error::OsError) -> Self {
        Self::Window(e.to_string())
    }
}

/// Waker „niebudzący” — zapytania `wgpu` rozwiązują się natychmiast,
/// więc nie potrzebujemy budzenia z innego wątku.
struct NoopWake;

impl Wake for NoopWake {
    fn wake(self: Arc<Self>) {}
    fn wake_by_ref(self: &Arc<Self>) {}
}

/// Oczekuje na przyszłość wewnątrz pętli zdarzeń.
///
/// `wgpu` udostępnia wyłącznie API asynchroniczne, a tworzenie urządzenia
/// GPU odbywa się w `resumed`, które nie jest `async`. Używamy własnego
/// minimalnego `block_on` — bez zależności od `futures`.
fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(NoopWake));
    let mut context = TaskContext::from_waker(&waker);
    let mut future = Box::pin(future);

    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            // Nie powinno się zdarzyć — gdyby jednak, oddajemy czas systemowi.
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowAttributes};

use crate::app::Application;
use crate::context::Context;
use crate::error::{Error, Result};
use crate::event::Event;
use crate::geometry::{Point, Rect, Size};
use crate::layout::{Cursor, Interaction, Layout, Limits};
use crate::message::{Message, Task};
use crate::renderer::{Renderer, RendererConfig};
use crate::tree::{Id, Tree};
use crate::window::chrome::{resize_direction_at, Chrome, ChromeAction, ResizeDirection};
use crate::window::{WindowCommand, WindowSettings};

/// Stan pojedynczego okna.
struct WindowState {
    window: Arc<Window>,
    device: wgpu::Device,
    surface: wgpu::Surface<'static>,
    renderer: Renderer,
    config: wgpu::SurfaceConfiguration,
    settings: WindowSettings,
    chrome: Chrome,
    tree: Tree,
    cursor: Cursor,
    modifiers: crate::event::Modifiers,
    focus: bool,
    occluded: bool,
    dragging: Option<DragKind>,
}

/// Aktywne przeciąganie okna lub jego krawędzi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragKind {
    Move,
    Resize(ResizeDirection),
}

/// Stan całej aplikacji.
struct Runtime {
    app: Box<dyn Application>,
    windows: Vec<WindowState>,
    start: Instant,
    queue: VecDeque<Message>,
    task: Option<Task>,
    dirty: bool,
}

/// Uruchamia aplikację w pętli zdarzeń `winit`.
pub fn run(app: impl Application + 'static, settings: WindowSettings) -> Result<()> {
    let event_loop = EventLoop::with_user_event()
        .build()
        .map_err(|e| Error::EventLoop(e.to_string()))?;

    let runtime = Runtime {
        app: Box::new(app),
        windows: Vec::new(),
        start: Instant::now(),
        queue: VecDeque::new(),
        task: None,
        dirty: true,
    };

    let mut backend = Backend {
        runtime,
        pending: Some(settings),
        close_requested: false,
    };
    event_loop
        .run_app(&mut backend)
        .map_err(|e| Error::EventLoop(e.to_string()))
}

/// Most między `winit` a runtime'em.
struct Backend {
    runtime: Runtime,
    pending: Option<WindowSettings>,
    /// Ustawiane, gdy przycisk okna poprosił o zamknięcie.
    close_requested: bool,
}

impl Backend {
    /// Tworzy pierwsze okno wraz z urządzeniem GPU.
    fn create_window(&mut self, event_loop: &ActiveEventLoop, settings: WindowSettings) {
        if let Err(error) = self.try_create_window(event_loop, settings) {
            log::error!("nie udało się utworzyć okna: {error}");
            event_loop.exit();
        }
    }

    fn try_create_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        settings: WindowSettings,
    ) -> Result<()> {
        let attributes = WindowAttributes::default()
            .with_title(&settings.title)
            .with_inner_size(LogicalSize::new(settings.size.width, settings.size.height))
            .with_min_inner_size(match settings.min_size {
                Some(size) => LogicalSize::new(size.width, size.height),
                None => LogicalSize::new(1.0, 1.0),
            })
            .with_max_inner_size(match settings.max_size {
                Some(size) => LogicalSize::new(size.width, size.height),
                None => LogicalSize::new(1.0e6_f32, 1.0e6_f32),
            })
            .with_resizable(settings.resizable)
            .with_decorations(settings.decorations)
            .with_transparent(settings.transparent)
            .with_visible(settings.visible);

        let window = Arc::new(event_loop.create_window(attributes)?);
        let physical = window.inner_size();
        let scale_factor = window.scale_factor() as f32;

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(Arc::clone(&window))?;

        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))?;

        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("aurola.device"),
            required_features: wgpu::Features::empty(),
            required_limits: adapter.limits(),
            ..Default::default()
        }))?;

        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or_else(|| capabilities.formats[0]);

        let config = Renderer::surface_config(
            format,
            physical.width,
            physical.height,
            settings.transparency,
        );
        surface.configure(&device, &config);

        let mut renderer = Renderer::new(
            device.clone(),
            queue,
            RendererConfig {
                surface_format: format,
                transparency: settings.transparency,
                scale_factor,
            },
        )?;
        renderer.set_shape(settings.shape);
        renderer.set_clear_color(settings.background);

        self.runtime.windows.push(WindowState {
            window,
            device,
            surface,
            renderer,
            config,
            settings,
            chrome: Chrome::new(),
            tree: Tree::new(Id::ROOT),
            cursor: Cursor::default(),
            modifiers: crate::event::Modifiers::NONE,
            focus: true,
            occluded: false,
            dragging: None,
        });
        Ok(())
    }

    /// Znajduje okno po identyfikatorze `winit`.
    #[allow(dead_code)]
    fn window_mut(&mut self, id: winit::window::WindowId) -> Option<&mut WindowState> {
        self.runtime
            .windows
            .iter_mut()
            .find(|state| state.window.id() == id)
    }

    /// Obsługa stref własnych dekoracji przy wciśnięciu lewego przycisku.
    ///
    /// Kolejność: obszary z widgetów → automatyczne strefy przy krawędziach.
    fn handle_chrome(&mut self, index: usize, event: &Event) {
        let Event::Mouse(crate::event::MouseEvent::ButtonPressed {
            button: crate::event::MouseButton::Left,
            position,
        }) = event
        else {
            return;
        };

        let state = &mut self.runtime.windows[index];
        let scale = state.renderer.scale_factor().max(0.1);
        let logical = Size::new(
            state.config.width as f32 / scale,
            state.config.height as f32 / scale,
        );
        let bounds = Rect::new(Point::ZERO, logical);

        let action = state.chrome.hit_test(*position).or_else(|| {
            resize_direction_at(
                *position,
                bounds,
                state.settings.resize_border,
                state.settings.resizable,
            )
            .map(ChromeAction::Resize)
        });

        match action {
            Some(ChromeAction::Drag) => {
                let _ = state.window.drag_window();
                state.dragging = Some(DragKind::Move);
            }
            Some(ChromeAction::Resize(direction)) => {
                let _ = state
                    .window
                    .drag_resize_window(to_winit_direction(direction));
                state.dragging = Some(DragKind::Resize(direction));
            }
            Some(ChromeAction::Command(command)) => {
                if self.apply_window_command(index, command) {
                    self.close_requested = true;
                }
            }
            _ => {}
        }
    }

    /// Wykonuje polecenie zarządzania oknem.
    ///
    /// Zwraca `true`, gdy należy zamknąć okno.
    fn apply_window_command(&mut self, index: usize, command: WindowCommand) -> bool {
        let state = &mut self.runtime.windows[index];
        let scale = state.renderer.scale_factor().max(0.1);
        match command {
            WindowCommand::Minimize => {
                let _ = state.window.set_minimized(true);
            }
            WindowCommand::ToggleMaximize => {
                let maximized = state.window.is_maximized();
                let _ = state.window.set_maximized(!maximized);
            }
            WindowCommand::Restore => {
                let _ = state.window.set_maximized(false);
            }
            WindowCommand::ShowSystemMenu => {
                let position = winit::dpi::PhysicalPosition::new(
                    (state.cursor.position.x * scale) as i32,
                    (state.cursor.position.y * scale) as i32,
                );
                let _ = state.window.show_window_menu(position);
            }
            // `winit` nie ma `request_close` — zamknięcie realizuje runtime.
            WindowCommand::Close => return true,
        }
        false
    }
}

impl Backend {
    /// Buduje, układa i rysuje jedną klatkę okna.
    fn draw_frame(&mut self, index: usize) {
        if self.runtime.windows[index].occluded {
            return;
        }

        // Chrome z poprzedniej klatki obsługujemy przed układaniem, bo
        // kliknięcie w drag/resize nie powinno trafić do widgetów.
        while let Some(message) = self.runtime.queue.pop_front() {
            if let Message::Event(event) = &message {
                self.handle_chrome(index, event);
            }
            self.dispatch(message);
        }

        let element = self.runtime.app.view();
        let theme = self.runtime.app.theme();
        let start = self.runtime.start;

        let state = &mut self.runtime.windows[index];
        let scale = state.renderer.scale_factor().max(0.1);
        let logical = Size::new(
            state.config.width as f32 / scale,
            state.config.height as f32 / scale,
        );

        // Synchronizacja drzewa — stan przenoszony po identyfikatorach.
        state.tree.sync(&element);
        state.chrome.clear();
        state.renderer.begin_frame(logical);
        state.renderer.set_clear_color(state.settings.background);

        let bounds = Rect::new(Point::ZERO, logical);
        let mut output: Vec<Message> = Vec::new();
        let mut commands: Vec<WindowCommand> = Vec::new();

        {
            let WindowState {
                ref mut renderer,
                ref mut chrome,
                ref mut tree,
                ref cursor,
                ref focus,
                ..
            } = *state;

            let mut ctx = Context::new(
                renderer,
                &theme,
                *cursor,
                &mut output,
                chrome,
                start,
                *focus,
                logical,
            );

            let root = element.as_widget();
            let limits = Limits::none(logical);
            root.layout(tree, &limits, &mut ctx);

            let root_layout = Layout::new(bounds)
                .with_clip(bounds)
                .with_interaction(Interaction::Idle);
            root.draw(tree, &mut ctx, &root_layout);
        }

        // Kursor systemowy: chrome ma pierwszeństwo przed widgetami.
        let icon = state.chrome.cursor_at(state.cursor.position);
        if let Some(icon) = icon {
            state.window.set_cursor(winit::window::Cursor::Icon(
                crate::platform::to_winit_cursor(icon),
            ));
        }

        let render_result = state.renderer.render(&state.surface);
        if let Err(error) = render_result {
            log::error!("błąd renderowania: {error}");
        }

        // Wiadomości z widgetów trafiają do aplikacji; `WindowCommand` obsługujemy my.
        for message in output {
            match message {
                Message::Window(command) => commands.push(command),
                other => self.runtime.queue.push_back(other),
            }
        }

        for command in commands {
            if self.apply_window_command(index, command) {
                self.close_requested = true;
            }
        }

        if self.close_requested {
            self.close_requested = false;
            self.runtime.windows.remove(index);
        }
    }

    /// Przekazuje wiadomość do aplikacji wraz z kontekstem.
    fn dispatch(&mut self, message: Message) {
        if message.should_exit() {
            return;
        }
        let theme = self.runtime.app.theme();
        let start = self.runtime.start;
        let Some(state) = self.runtime.windows.first_mut() else {
            return;
        };

        let scale = state.renderer.scale_factor().max(0.1);
        let logical = Size::new(
            state.config.width as f32 / scale,
            state.config.height as f32 / scale,
        );

        let mut sink: Vec<Message> = Vec::new();
        let mut scratch = Chrome::new();
        let task = {
            let mut ctx = Context::new(
                &mut state.renderer,
                &theme,
                state.cursor,
                &mut sink,
                &mut scratch,
                start,
                state.focus,
                logical,
            );
            self.runtime.app.update(&mut ctx, message)
        };

        // Ostatnie zadanie ma pierwszeństwo; wcześniejsze są nadpisywane.
        if task.is_none() {
            self.runtime.task = self.runtime.task.take();
        } else {
            self.runtime.task = Some(task);
        }

        for extra in sink {
            if !matches!(extra, Message::Window(_)) {
                self.runtime.queue.push_back(extra);
            }
        }
    }
}

/// Mapuje kierunek resize na typ `winit`.
fn to_winit_direction(direction: ResizeDirection) -> winit::window::ResizeDirection {
    use winit::window::ResizeDirection as W;
    match direction {
        ResizeDirection::North => W::North,
        ResizeDirection::South => W::South,
        ResizeDirection::West => W::West,
        ResizeDirection::East => W::East,
        ResizeDirection::NorthEast => W::NorthEast,
        ResizeDirection::NorthWest => W::NorthWest,
        ResizeDirection::SouthEast => W::SouthEast,
        ResizeDirection::SouthWest => W::SouthWest,
    }
}

impl ApplicationHandler<()> for Backend {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(settings) = self.pending.take() {
            self.create_window(event_loop, settings);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let Some(index) = self
            .runtime
            .windows
            .iter()
            .position(|state| state.window.id() == window_id)
        else {
            return;
        };

        let mut incoming: Vec<Event> = Vec::new();
        let mut close = false;
        let mut redraw = false;

        {
            let state = &mut self.runtime.windows[index];
            let scale = state.window.scale_factor() as f32;
            let logical = |p: winit::dpi::PhysicalPosition<f64>| {
                Point::new((p.x as f32 / scale).floor(), (p.y as f32 / scale).floor())
            };

            match event {
                WindowEvent::CloseRequested => close = true,
                WindowEvent::Resized(size) => {
                    state.config.width = size.width.max(1);
                    state.config.height = size.height.max(1);
                    state.surface.configure(&state.device, &state.config);
                    self.runtime.dirty = true;
                }
                WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                    state.renderer.set_scale_factor(scale_factor as f32);
                    self.runtime.dirty = true;
                }
                WindowEvent::Focused(focused) => {
                    state.focus = focused;
                    self.runtime.dirty = true;
                }
                WindowEvent::Occluded(occluded) => state.occluded = occluded,
                WindowEvent::CursorLeft { .. } => {
                    state.cursor = Cursor::new(state.cursor.position, false);
                    self.runtime.dirty = true;
                }
                WindowEvent::CursorMoved { position, .. } => {
                    let point = logical(position);
                    state.cursor = Cursor::new(point, true);
                    incoming.push(Event::Mouse(crate::event::MouseEvent::CursorMoved {
                        position: point,
                    }));
                    self.runtime.dirty = true;
                }
                WindowEvent::ModifiersChanged(modifiers) => {
                    state.modifiers = to_modifiers(modifiers.state());
                }
                WindowEvent::MouseInput {
                    state: button,
                    button: which,
                    ..
                } => {
                    let position = state.cursor.position;
                    let mapped = to_mouse_button(which);
                    if button.is_pressed() {
                        incoming.push(Event::Mouse(crate::event::MouseEvent::ButtonPressed {
                            button: mapped,
                            position,
                        }));
                    } else {
                        incoming.push(Event::Mouse(crate::event::MouseEvent::ButtonReleased {
                            button: mapped,
                            position,
                        }));
                    }
                    self.runtime.dirty = true;
                }
                WindowEvent::MouseWheel { delta, .. } => {
                    let mapped = match delta {
                        winit::event::MouseScrollDelta::LineDelta(x, y) => {
                            crate::event::ScrollDelta::Lines { x, y }
                        }
                        winit::event::MouseScrollDelta::PixelDelta(p) => {
                            crate::event::ScrollDelta::Pixels {
                                x: p.x as f32,
                                y: p.y as f32,
                            }
                        }
                    };
                    incoming.push(Event::Mouse(crate::event::MouseEvent::Wheel {
                        delta: mapped,
                    }));
                    self.runtime.dirty = true;
                }
                WindowEvent::KeyboardInput { event, .. } => {
                    let named = to_named_key(event.logical_key.clone());
                    incoming.push(Event::Keyboard(crate::event::KeyEvent {
                        key: to_key(event.logical_key),
                        named,
                        pressed: event.state.is_pressed(),
                        modifiers: state.modifiers,
                        repeat: event.repeat,
                    }));
                    self.runtime.dirty = true;
                }
                WindowEvent::ThemeChanged(_) => {
                    self.runtime.dirty = true;
                }
                WindowEvent::RedrawRequested => {
                    redraw = true;
                }
                _ => {}
            }
        }

        if redraw {
            self.runtime.dirty = false;
            self.draw_frame(index);
            return;
        }

        if close {
            self.runtime.windows.remove(index);
            if self.runtime.windows.is_empty() {
                event_loop.exit();
            }
            return;
        }

        for event in incoming {
            self.runtime.queue.push_back(Message::Event(event));
        }

        if self.runtime.dirty {
            self.runtime.dirty = false;
            if let Some(state) = self.runtime.windows.get_mut(index) {
                state.window.request_redraw();
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let needs_redraw = self.runtime.windows.iter().any(|state| !state.occluded);
        if needs_redraw {
            for state in &mut self.runtime.windows {
                if !state.occluded {
                    state.window.request_redraw();
                }
            }
        }
        if self.runtime.windows.is_empty() {
            event_loop.exit();
        }
    }
}

/// Mapuje modyfikatory `winit` na typy biblioteki.
fn to_modifiers(state: ModifiersState) -> crate::event::Modifiers {
    crate::event::Modifiers {
        ctrl: state.contains(ModifiersState::CONTROL),
        shift: state.contains(ModifiersState::SHIFT),
        alt: state.contains(ModifiersState::ALT),
        meta: state.contains(ModifiersState::SUPER),
    }
}

/// Mapuje przycisk myszy `winit`.
fn to_mouse_button(button: winit::event::MouseButton) -> crate::event::MouseButton {
    use winit::event::MouseButton as W;
    match button {
        W::Left => crate::event::MouseButton::Left,
        W::Right => crate::event::MouseButton::Right,
        W::Middle => crate::event::MouseButton::Middle,
        _ => crate::event::MouseButton::Other,
    }
}

/// Mapuje klawisz `winit` na tekst.
fn to_key(key: winit::keyboard::Key) -> crate::event::Key {
    match key {
        winit::keyboard::Key::Character(c) => crate::event::Key::new(c.to_string()),
        other => crate::event::Key::new(format!("{other:?}")),
    }
}

/// Mapuje klawisz `winit` na klawisz specjalny.
fn to_named_key(key: winit::keyboard::Key) -> crate::event::NamedKey {
    use winit::keyboard::{Key as K, NamedKey as NK};
    match &key {
        K::Named(NK::Enter) => crate::event::NamedKey::Enter,
        K::Named(NK::Escape) => crate::event::NamedKey::Escape,
        K::Named(NK::Space) => crate::event::NamedKey::Space,
        K::Named(NK::Tab) => crate::event::NamedKey::Tab,
        K::Named(NK::Backspace) => crate::event::NamedKey::Backspace,
        K::Named(NK::Delete) => crate::event::NamedKey::Delete,
        K::Named(NK::ArrowUp) => crate::event::NamedKey::ArrowUp,
        K::Named(NK::ArrowDown) => crate::event::NamedKey::ArrowDown,
        K::Named(NK::ArrowLeft) => crate::event::NamedKey::ArrowLeft,
        K::Named(NK::ArrowRight) => crate::event::NamedKey::ArrowRight,
        K::Named(NK::Home) => crate::event::NamedKey::Home,
        K::Named(NK::End) => crate::event::NamedKey::End,
        K::Named(NK::PageUp) => crate::event::NamedKey::PageUp,
        K::Named(NK::PageDown) => crate::event::NamedKey::PageDown,
        _ => crate::event::NamedKey::Unidentified,
    }
}
