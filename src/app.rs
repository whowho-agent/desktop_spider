use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::Vec2;
use tray_icon::menu::MenuEvent;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{Window, WindowId};

use crate::config::{self, Preset, PresetWatcher};
use crate::input::WhipInput;
use crate::platform;
use crate::render::{DrawList, Renderer};
use crate::settings::{Mode, Settings};
use crate::tray::{Tray, TrayAction};
use crate::world::World;

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
    world: World,
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

    settings: Settings,
    /// Ввод плети: `held` — хоткей зажат (и режим не «Паук»), кнопки — из событий окна при захвате.
    input: WhipInput,
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

        let mut settings = Settings::load();
        // При запуске всегда «Просто плеть»; режим можно сменить в меню на время работы.
        settings.mode = Mode::Whip;
        let mut name = std::env::var("SPIDER_PRESET")
            .ok()
            .or_else(|| Some(settings.preset.clone()).filter(|p| !p.is_empty()))
            .unwrap_or_else(|| "spider".into());
        let preset = config::load_preset(&name).unwrap_or_else(|e| {
            eprintln!("пресет {name}: {e}; беру встроенный");
            let p = Preset::default();
            name = p.name.clone();
            p
        });
        let cursor = platform::cursor_position(&window);
        let mut world = World::new(&preset, settings.mode, settings.whip_skin, settings.whip_size, cursor);
        world.set_bounds(logical_size(&window));
        let debug = std::env::var_os("SPIDER_DEBUG").is_some();

        // Меню-бар / трей: только на главном потоке после старта цикла, т.е. здесь.
        let tray = Tray::new(&config::list_presets(), &name, debug)
            .map_err(|e| eprintln!("иконка в меню-баре не создана: {e}"))
            .ok();
        if let Some(t) = &tray {
            t.set_settings(&settings);
        }

        window.request_redraw();
        let now = Instant::now();
        self.state = Some(State {
            window,
            renderer,
            world,
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
            settings,
            input: WhipInput::default(),
        });
        if let Some(s) = self.state.as_ref() {
            s.show_score();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(s) = self.state.as_mut() else { return };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                s.renderer.resize(size, s.window.scale_factor());
                s.world.set_bounds(logical_size(&s.window));
            }
            WindowEvent::ScaleFactorChanged { scale_factor, mut inner_size_writer } => {
                platform::on_scale_factor_changed(&s.window, &mut inner_size_writer);
                s.renderer.resize(s.window.inner_size(), scale_factor)
            }
            // Кнопки приходят, только пока оверлей захватывает мышь (хоткей плети зажат).
            WindowEvent::MouseInput { state, button, .. } if s.input.held => {
                let down = state == ElementState::Pressed;
                match button {
                    MouseButton::Left => s.input.lmb = down,
                    MouseButton::Right => s.input.rmb = down,
                    _ => {}
                }
                s.window.request_redraw();
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
            TrayAction::Mode(m) => s.set_mode(m),
            TrayAction::Skin(skin) => {
                s.settings.whip_skin = skin;
                s.world.skin = skin;
                s.save_settings();
            }
            TrayAction::Size(v) => {
                s.settings.whip_size = v;
                s.world.whip.set_scale(v, s.cursor);
                s.save_settings();
            }
            TrayAction::Hotkey(hk) => {
                s.settings.whip_hotkey = hk;
                s.save_settings();
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
        // Хоткей опрашивается и в idle: зажал — сразу полная частота кадров.
        if s.poll_hotkey() {
            s.last_move = now;
        }
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

        self.poll_hotkey();

        if !self.paused {
            // dt ограничен сверху, так что редкие idle-кадры не ломают аккумулятор.
            self.acc += (now - self.last).as_secs_f32().min(MAX_FRAME_DT);
            let mut score_changed = false;
            while self.acc >= STEP {
                score_changed |= self.world.step(STEP, cursor, self.input);
                self.acc -= STEP;
            }
            if score_changed {
                self.show_score();
            }
        }
        self.last = now;

        self.list.clear();
        // Плеть видна, пока зажат хоткей; эффекты (вспышки, следы) догорают и без него.
        self.world.draw(&mut self.list, &self.preset, cursor, self.input.held, self.debug);
        self.renderer.render(&self.list);
    }

    /// Активный режим: курсор двигался недавно, зажата плеть или догорают её эффекты.
    fn is_active(&self, now: Instant) -> bool {
        !self.paused
            && (now.duration_since(self.last_move) < IDLE_AFTER
                || self.input.held
                || self.world.whip_animating())
    }

    /// Опрашивает хоткей; на переходах включает/выключает захват мыши. Возвращает «только что зажали».
    fn poll_hotkey(&mut self) -> bool {
        let held = self.settings.mode != Mode::Spider && !self.paused && platform::hotkey_held(&self.settings.whip_hotkey);
        if held == self.input.held {
            return false;
        }
        self.input.held = held;
        if !held {
            self.input.lmb = false;
            self.input.rmb = false;
        } else if !self.world.whip.is_animating() {
            self.world.whip.reset(self.cursor);
        }
        platform::set_whip_capture(&self.window, held);
        held
    }

    /// Счёт у иконки меню-бара / в подсказке трея; только в режиме «Игра».
    fn show_score(&self) {
        let Some(t) = &self.tray else { return };
        if self.settings.mode == Mode::Game {
            t.set_score(Some(&format!("{} / {}", self.world.hits, self.world.strikes)));
        } else {
            t.set_score(None);
        }
    }

    fn set_mode(&mut self, mode: Mode) {
        if mode == self.settings.mode {
            return;
        }
        self.settings.mode = mode;
        self.world.set_mode(mode);
        if mode == Mode::Spider && self.input.held {
            self.input = WhipInput::default();
            platform::set_whip_capture(&self.window, false);
        }
        self.last_move = Instant::now();
        self.save_settings();
        self.show_score();
    }

    fn save_settings(&self) {
        self.settings.save();
        if let Some(t) = &self.tray {
            t.set_settings(&self.settings);
        }
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
                self.world.creature.set_preset(&p);
                self.preset = p;
                self.preset_name = name.to_string();
                if self.settings.preset != name {
                    self.settings.preset = name.to_string();
                    self.save_settings();
                }
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

/// Размер окна-оверлея в логических точках — поле, где живёт паук.
fn logical_size(window: &Window) -> Vec2 {
    let s = window.inner_size().to_logical::<f32>(window.scale_factor());
    Vec2::new(s.width, s.height)
}
