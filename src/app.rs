use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::Vec2;
use tray_icon::menu::MenuEvent;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{Window, WindowId};

use crate::config::{self, Preset, PresetWatcher};
use crate::platform;
use crate::render::{DrawList, Renderer};
use crate::sim::Creature;
use crate::tray::{Tray, TrayAction};

/// Шаг симуляции: 120 Гц, независимо от частоты кадров.
const STEP: f32 = 1.0 / 120.0;
/// Не догоняем больше этого после подвисаний (сон, перетаскивание и т.п.).
const MAX_FRAME_DT: f32 = 0.1;
/// Курсор стоит дольше этого → паук почти наверняка тоже стоит, рендер в idle.
const IDLE_AFTER: Duration = Duration::from_millis(1500);
/// Частота кадров в idle (заодно частота опроса курсора).
const IDLE_TICK: Duration = Duration::from_micros(1_000_000 / 15);
/// На паузе кадров нет, но раз в полсекунды проверяем изменения пресетов.
const PAUSED_TICK: Duration = Duration::from_millis(500);

/// События из других потоков/библиотек в winit-цикл.
#[derive(Debug)]
pub enum UserEvent {
    Menu(MenuEvent),
}

#[derive(Default)]
pub struct App {
    state: Option<State>,
}

struct State {
    window: Arc<Window>,
    renderer: Renderer,
    creature: Creature,
    preset: Preset,
    /// Имя файла текущего пресета (ключ для перезагрузки).
    preset_name: String,
    list: DrawList,
    last: Instant,
    acc: f32,
    debug: bool,

    tray: Option<Tray>,
    watcher: PresetWatcher,
    /// Пауза: симуляция заморожена, на экране остаётся последний кадр (окно не перерисовывается).
    paused: bool,
    cursor: Vec2,
    last_move: Instant,
    next_idle_tick: Instant,
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        let attrs = platform::overlay_attributes();
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        platform::configure_overlay(&window);

        let mut renderer = Renderer::new(window.clone());
        renderer.resize(window.inner_size(), window.scale_factor());

        let mut name = std::env::var("SPIDER_PRESET").unwrap_or_else(|_| "spider".into());
        let preset = config::load_preset(&name).unwrap_or_else(|e| {
            eprintln!("пресет {name}: {e}; беру встроенный");
            let p = Preset::default();
            name = p.name.clone();
            p
        });
        let cursor = platform::cursor_position(&window);
        let creature = Creature::new(&preset, cursor);
        let debug = std::env::var_os("SPIDER_DEBUG").is_some();

        // Меню-бар / трей: только на главном потоке после старта цикла, т.е. здесь.
        let tray = Tray::new(&config::list_presets(), &name, debug)
            .map_err(|e| eprintln!("иконка в меню-баре не создана: {e}"))
            .ok();

        window.request_redraw();
        let now = Instant::now();
        self.state = Some(State {
            window,
            renderer,
            creature,
            preset,
            preset_name: name,
            list: DrawList::default(),
            last: now,
            acc: 0.0,
            debug,
            tray,
            watcher: PresetWatcher::new(),
            paused: false,
            cursor,
            last_move: now,
            next_idle_tick: now,
        });
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(s) = self.state.as_mut() else { return };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => s.renderer.resize(size, s.window.scale_factor()),
            WindowEvent::ScaleFactorChanged { scale_factor, mut inner_size_writer } => {
                platform::on_scale_factor_changed(&s.window, &mut inner_size_writer);
                s.renderer.resize(s.window.inner_size(), scale_factor)
            }
            WindowEvent::RedrawRequested => {
                s.frame();
                // Непрерывный рендер (vsync) только в активном режиме;
                // idle-кадры планирует about_to_wait.
                if s.is_active(Instant::now()) {
                    s.window.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        let Some(s) = self.state.as_mut() else { return };
        let UserEvent::Menu(ev) = event;
        let Some(action) = s.tray.as_ref().and_then(|t| t.action(&ev)) else { return };
        match action {
            TrayAction::Quit => event_loop.exit(),
            TrayAction::Preset(name) => s.switch_preset(&name),
            TrayAction::Reload => {
                let name = s.preset_name.clone();
                if let Some(t) = s.tray.as_mut() {
                    t.set_presets(&config::list_presets(), &name);
                }
                s.switch_preset(&name);
            }
            TrayAction::Pause(on) => {
                s.paused = on;
                if let Some(t) = &s.tray {
                    t.set_paused(on);
                }
                // После паузы не догоняем пропущенное время.
                s.last = Instant::now();
                s.acc = 0.0;
                s.last_move = s.last;
            }
            TrayAction::Debug(on) => {
                s.debug = on;
                if let Some(t) = &s.tray {
                    t.set_debug(on);
                }
            }
        }
        // Любое изменение из меню показываем сразу, даже на паузе.
        s.window.request_redraw();
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(s) = self.state.as_mut() else { return };

        if s.watcher.poll() {
            let name = s.preset_name.clone();
            s.switch_preset(&name);
            s.window.request_redraw();
        }

        let now = Instant::now();
        if s.paused {
            event_loop.set_control_flow(ControlFlow::WaitUntil(now + PAUSED_TICK));
            return;
        }

        let was_active = s.is_active(now);
        s.track_cursor(now);
        if s.is_active(now) {
            // Курсор сдвинулся после idle → сразу назад к vsync-кадрам.
            if !was_active {
                s.window.request_redraw();
            }
            event_loop.set_control_flow(ControlFlow::Wait);
        } else {
            if now >= s.next_idle_tick {
                s.window.request_redraw();
                s.next_idle_tick = now + IDLE_TICK;
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(s.next_idle_tick));
        }
    }
}

impl State {
    fn frame(&mut self) {
        let now = Instant::now();
        self.track_cursor(now);
        let cursor = self.cursor;

        if !self.paused {
            // dt ограничен сверху, так что редкие idle-кадры не ломают аккумулятор.
            self.acc += (now - self.last).as_secs_f32().min(MAX_FRAME_DT);
            while self.acc >= STEP {
                self.creature.update(STEP, cursor);
                self.acc -= STEP;
            }
        }
        self.last = now;

        self.list.clear();
        self.list.ring(cursor, self.preset.body.stop_radius, 1.0, self.preset.color_ring);
        self.creature.draw(&mut self.list, self.debug);
        self.renderer.render(&self.list);
    }

    /// Активный режим: курсор двигался недавно.
    fn is_active(&self, now: Instant) -> bool {
        !self.paused && now.duration_since(self.last_move) < IDLE_AFTER
    }

    fn track_cursor(&mut self, now: Instant) {
        let c = platform::cursor_position(&self.window);
        if c.distance_squared(self.cursor) > 0.25 {
            self.cursor = c;
            self.last_move = now;
        }
    }

    /// Загружает пресет по имени; при ошибке остаёмся на текущем.
    fn switch_preset(&mut self, name: &str) {
        match config::load_preset(name) {
            Ok(p) => {
                eprintln!("пресет загружен: {name}");
                self.creature.set_preset(&p);
                self.preset = p;
                self.preset_name = name.to_string();
                // Новые ноги должны успеть встать на место, даже если курсор стоит.
                self.last_move = Instant::now();
            }
            Err(e) => eprintln!("пресет {name}: {e}"),
        }
        if let Some(t) = &self.tray {
            t.set_current(&self.preset_name);
        }
    }
}
