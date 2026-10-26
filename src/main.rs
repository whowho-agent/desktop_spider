// Windows: release-сборка — GUI-подсистема, иначе при запуске (двойной клик, автозапуск)
// открывается консольное окно с кнопкой на панели задач. Логи (eprintln) при этом
// по-прежнему пишутся, если stderr перенаправлен (`2> log`, `cargo run`).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod config;
mod input;
mod platform;
mod record;
mod render;
mod settings;
mod sim;
mod tray;
mod whip;
mod world;

use tray_icon::menu::MenuEvent;
use winit::event_loop::EventLoop;

use app::UserEvent;

fn main() {
    // Запись демо-роликов без окна (для README): `--record <spider|game|whip> <out.rgba>`.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--record") {
        if let Err(e) = record::run(&args[1..]) {
            eprintln!("{e}");
            std::process::exit(2);
        }
        return;
    }
    let mut builder = EventLoop::<UserEvent>::with_user_event();
    platform::configure_event_loop(&mut builder);
    let event_loop = builder.build().expect("event loop");

    // События меню (AppKit-колбэки / Win32-сообщения трея) → пересылаем в winit-цикл.
    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |ev| {
        let _ = proxy.send_event(UserEvent::Menu(ev));
    }));

    let mut app = app::App::default();
    event_loop.run_app(&mut app).expect("run app");
}
