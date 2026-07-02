use std::sync::{Arc, Mutex};

use enigo::{Enigo, Mouse};
use tauri::{LogicalPosition, Manager, Position};

fn show_on_cursor_handler(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let enigo = app.state::<Arc<Mutex<Enigo>>>();
        let enigo = match enigo.lock() {
            Ok(enigo) => enigo,
            Err(_) => {
                println!("Failed to get cursor position");
                return;
            }
        };

        let (mouse_x, mouse_y) = match enigo.location() {
            Ok(location) => location,
            Err(_) => {
                println!("Failed to get cursor position");
                return;
            }
        };

        // Physical position causes the position to be off on HiDPI screens
        // TODO: clamp the position to the screen size
        let new_pos = LogicalPosition {
            x: f64::from(mouse_x),
            y: f64::from(mouse_y),
        };

        match window.set_position(Position::Logical(new_pos)) {
            Ok(_) => {}
            Err(e) => {
                println!("Failed to position window: {e}");
                return;
            }
        }

        let window = window.clone();
        // this is a hack to make the window appear on the correct
        // position without flickering.
        // Because tauri window methods are async, show() may run before
        // set_position() finishes, causing the window to briefly appear
        // on the old position before moving to the new one.
        // Since we don't want to block the main thread, we spawn another
        // one to show the window; otherwise the flickering will be worse,
        // since we block the main thread for a short time.
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(25));

            match window.show() {
                Ok(_) => {}
                Err(e) => println!("Failed to show window: {e}"),
            }

            match window.set_focus() {
                Ok(_) => {}
                Err(e) => println!("Failed to focus window: {e}"),
            }
        });
    }
}

pub fn register_shortcuts(app: &tauri::AppHandle) -> Result<(), tauri::Error> {
    #[cfg(desktop)]
    {
        use tauri_plugin_global_shortcut::{
            Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState,
        };

        let show_window_shortcut =
            Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyV);

        let global_shortcut_handler = tauri_plugin_global_shortcut::Builder::new()
            .with_handler(move |app, shortcut, event| {
                if shortcut == &show_window_shortcut {
                    match event.state() {
                        ShortcutState::Pressed => show_on_cursor_handler(app),
                        ShortcutState::Released => {}
                    }
                }
            })
            .build();

        app.plugin(global_shortcut_handler)?;

        match app.global_shortcut().register(show_window_shortcut) {
            Ok(_) => println!("Registered shortcut"),
            Err(e) => println!("Failed to register shortcut: {e}"),
        };
    }

    Ok(())
}
