mod app;
mod config;
mod platform;
mod render;
mod sim;
mod tray;

use tray_icon::menu::MenuEvent;
use winit::event_loop::EventLoop;

use app::UserEvent;

fn main() {
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
