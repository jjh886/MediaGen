use std::io::{Read, Write};
use std::net::TcpStream;

use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::launch::LaunchSession;
use crate::open_browser::open_browser;

enum UserEvent {
    Menu(MenuEvent),
}

struct TrayMenuIds {
    open: MenuId,
    quit: MenuId,
}

pub fn run(session: LaunchSession) -> Result<(), String> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |event| {
        let _ = proxy.send_event(UserEvent::Menu(event));
    }));

    let menu_ids = TrayMenuIds {
        open: MenuId::new("open-console"),
        quit: MenuId::new("quit-mediagen"),
    };
    open_browser(&session.url)?;
    let mut tray_icon: Option<TrayIcon> = None;
    let mut startup_error: Option<String> = None;
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::NewEvents(StartCause::Init) => match build_tray_icon(&menu_ids) {
                Ok(icon) => tray_icon = Some(icon),
                Err(message) => startup_error = Some(message),
            },
            Event::MainEventsCleared => {
                if let Some(message) = startup_error.take() {
                    eprintln!("{message}");
                    *control_flow = ControlFlow::Exit;
                }
                let _ = tray_icon.as_ref();
            }
            Event::UserEvent(UserEvent::Menu(event)) => {
                handle_menu_event(event, &menu_ids, &session, control_flow);
            }
            _ => {}
        }
    });
}

fn build_tray_icon(ids: &TrayMenuIds) -> Result<TrayIcon, String> {
    let tray_menu = build_menu(ids)?;
    TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip("MediaGen")
        .with_icon(load_icon()?)
        .with_menu_on_left_click(true)
        .build()
        .map_err(|err| format!("创建托盘图标失败: {err}"))
}

fn build_menu(ids: &TrayMenuIds) -> Result<Menu, String> {
    let menu = Menu::new();
    let open = MenuItem::with_id(ids.open.clone(), "打开控制台", true, None);
    let quit = MenuItem::with_id(ids.quit.clone(), "退出 MediaGen", true, None);
    menu.append_items(&[&open, &PredefinedMenuItem::separator(), &quit])
        .map_err(|err| format!("创建托盘菜单失败: {err}"))?;
    Ok(menu)
}

fn handle_menu_event(
    event: MenuEvent,
    ids: &TrayMenuIds,
    session: &LaunchSession,
    control_flow: &mut ControlFlow,
) {
    if event.id == ids.open {
        let _ = open_browser(&session.url);
        return;
    }
    if event.id == ids.quit {
        let _ = request_server_exit(session.port);
        *control_flow = ControlFlow::Exit;
    }
}

fn request_server_exit(port: u16) -> Result<(), String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .map_err(|err| format!("连接本机服务失败: {err}"))?;
    let request = b"POST /api/system/exit HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    stream
        .write_all(request)
        .map_err(|err| format!("发送退出请求失败: {err}"))?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|err| format!("读取退出响应失败: {err}"))?;
    if response.starts_with("HTTP/1.1 200") {
        Ok(())
    } else {
        Err("本机服务拒绝退出请求".to_string())
    }
}

fn load_icon() -> Result<Icon, String> {
    let image = image::load_from_memory(include_bytes!("../assets/mediagen.png"))
        .map_err(|err| format!("读取托盘图标失败: {err}"))?
        .into_rgba8();
    let (width, height) = image.dimensions();
    Icon::from_rgba(image.into_raw(), width, height)
        .map_err(|err| format!("创建托盘图标失败: {err}"))
}
