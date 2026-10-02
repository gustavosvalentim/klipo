use crate::clipboard::ClipboardEventsEmitter;
use crate::content::PasteContent;
use crate::desktop::DesktopCapability;
use crate::input::simulate_paste_input;
use crate::state::AppState;
use crate::window::{capture_focused_window, get_main_window};
use crate::window::{has_focused_target, restore_focused_window};
use crate::window::{PICKER_HEIGHT, PICKER_WIDTH};
use log::{error, warn};
use serde::Serialize;
use tauri::{AppHandle, Manager, PhysicalPosition, Position, WebviewWindow};

pub fn open(app: &tauri::AppHandle) -> Result<(), String> {
    let window = get_main_window(app).ok_or("Picker window unavailable")?;
    let state = app.state::<AppState>();
    let picker_is_focused = window.is_focused();
    let generation = if matches!(picker_is_focused, Ok(true)) {
        state.picker_session()
    } else {
        state.next_picker_session()
    };
    if should_capture_focus_target(&picker_is_focused) {
        if app
            .get_webview_window("settings")
            .is_some_and(|settings| settings.is_focused().unwrap_or(false))
        {
            crate::window::clear_focus_target(&state);
        } else if let Err(error_value) = capture_focused_window(&state) {
            error!(error:debug = error_value; "Failed to get and store window state");
        }
    } else if let Err(error_value) = picker_is_focused {
        crate::window::clear_focus_target(&state);
        error!(error:debug = error_value; "Failed to determine whether the picker is focused");
    }

    let window_position = Position::Physical(get_picker_position(app, &window));

    if let Err(error_value) = window.set_position(window_position) {
        error!(error:debug = error_value; "Failed to position window");
        return Err(error_value.to_string());
    }

    if !state.picker_session_is_current(generation) {
        return Err("Picker session was dismissed".into());
    }
    window
        .show()
        .map_err(|error_value| error_value.to_string())?;
    if !state.picker_session_is_current(generation) {
        return Err("Picker session was dismissed".into());
    }
    window
        .set_focus()
        .map_err(|error_value| error_value.to_string())?;
    Ok(())
}

pub fn recover(app: &tauri::AppHandle, generation: u64) -> Result<(), String> {
    let state = app.state::<AppState>();
    if !state.picker_session_is_current(generation) {
        return Err("Picker session was dismissed".into());
    }
    let window = get_main_window(app).ok_or("Picker window unavailable")?;
    window.show().map_err(|error| error.to_string())?;
    if !state.picker_session_is_current(generation) {
        return Err("Picker session was dismissed".into());
    }
    window.set_focus().map_err(|error| error.to_string())?;
    state.expect_paste_focus_loss(false);
    Ok(())
}

fn should_capture_focus_target<FocusError>(picker_is_focused: &Result<bool, FocusError>) -> bool {
    matches!(picker_is_focused, Ok(false))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PhysicalRectangle {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

impl PhysicalRectangle {
    fn contains(self, point: PhysicalPosition<i32>) -> bool {
        let point_x = i64::from(point.x);
        let point_y = i64::from(point.y);
        let right = i64::from(self.x) + i64::from(self.width);
        let bottom = i64::from(self.y) + i64::from(self.height);

        point_x >= i64::from(self.x)
            && point_x < right
            && point_y >= i64::from(self.y)
            && point_y < bottom
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct MonitorGeometry {
    bounds: PhysicalRectangle,
    work_area: PhysicalRectangle,
    is_primary: bool,
}

fn resolve_picker_position(
    pointer: Option<PhysicalPosition<i32>>,
    monitors: &[MonitorGeometry],
    picker_size: (u32, u32),
) -> Option<PhysicalPosition<i32>> {
    let monitor_at_pointer = pointer.and_then(|point| {
        monitors
            .iter()
            .find(|monitor| monitor.bounds.contains(point))
    });
    let selected_monitor = monitor_at_pointer
        .or_else(|| monitors.iter().find(|monitor| monitor.is_primary))
        .or_else(|| monitors.first())?;

    let fallback_position =
        PhysicalPosition::new(selected_monitor.work_area.x, selected_monitor.work_area.y);
    let pointer_position = if monitor_at_pointer.is_some() {
        pointer.unwrap_or(fallback_position)
    } else {
        fallback_position
    };

    Some(PhysicalPosition::new(
        clamp_picker_axis(
            pointer_position.x,
            selected_monitor.work_area.x,
            selected_monitor.work_area.width,
            picker_size.0,
        ),
        clamp_picker_axis(
            pointer_position.y,
            selected_monitor.work_area.y,
            selected_monitor.work_area.height,
            picker_size.1,
        ),
    ))
}

fn clamp_picker_axis(pointer: i32, work_start: i32, work_length: u32, picker_length: u32) -> i32 {
    let work_start = i64::from(work_start);
    let work_end = work_start + i64::from(work_length);
    let picker_end = work_end - i64::from(picker_length);
    let clamped_position = if picker_end >= work_start {
        i64::from(pointer).clamp(work_start, picker_end)
    } else {
        // A picker larger than the usable area cannot fit fully, so anchor it at
        // the work-area origin to keep as much of it visible as possible.
        work_start
    };

    clamped_position.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

fn get_picker_position(app: &tauri::AppHandle, window: &WebviewWindow) -> PhysicalPosition<i32> {
    let picker_size = window
        .inner_size()
        .map(|size| (size.width, size.height))
        .unwrap_or_else(|error_value| {
            warn!(error:debug = error_value; "Failed to measure picker size; using configured size");
            (PICKER_WIDTH.round() as u32, PICKER_HEIGHT.round() as u32)
        });
    let monitors = match app.available_monitors() {
        Ok(monitors) => monitors,
        Err(error_value) => {
            warn!(error:debug = error_value; "Failed to list monitors; using origin");
            return PhysicalPosition::new(0, 0);
        }
    };
    let primary_monitor = match app.primary_monitor() {
        Ok(monitor) => monitor,
        Err(error_value) => {
            warn!(error:debug = error_value; "Failed to get primary monitor; using first monitor");
            None
        }
    };
    let monitor_geometries = monitors
        .iter()
        .map(|monitor| MonitorGeometry {
            bounds: PhysicalRectangle {
                x: monitor.position().x,
                y: monitor.position().y,
                width: monitor.size().width,
                height: monitor.size().height,
            },
            work_area: PhysicalRectangle {
                x: monitor.work_area().position.x,
                y: monitor.work_area().position.y,
                width: monitor.work_area().size.width,
                height: monitor.work_area().size.height,
            },
            is_primary: primary_monitor
                .as_ref()
                .is_some_and(|primary| monitors_match(monitor, primary)),
        })
        .collect::<Vec<_>>();
    // Tauri reports cursor, monitors, work areas, and window size in physical pixels.
    let pointer = match app.cursor_position() {
        Ok(position) => Some(PhysicalPosition::new(
            position.x.round() as i32,
            position.y.round() as i32,
        )),
        Err(error_value) => {
            warn!(error:debug = error_value; "Failed to get cursor position; using primary monitor");
            None
        }
    };

    resolve_picker_position(pointer, &monitor_geometries, picker_size).unwrap_or_else(|| {
        warn!("No monitor is available; using origin");
        PhysicalPosition::new(0, 0)
    })
}

fn monitors_match(left: &tauri::Monitor, right: &tauri::Monitor) -> bool {
    left.position() == right.position() && left.size() == right.size()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PasteOutcome {
    Pasted,
    CopiedForManualPaste,
}

pub(crate) trait PasteOperations {
    fn write_item(&self, item: &PasteContent) -> Result<(), String>;
    fn move_to_top(&self, hash: &str) -> Result<(), String>;
    fn restore_target(&self) -> Result<(), String>;
    fn hide_picker(&self) -> Result<(), String>;
    fn simulate_input(&self) -> Result<(), String>;
}

struct AppPasteOperations<'a> {
    app: &'a AppHandle,
    state: &'a AppState,
    generation: u64,
    reordered: std::cell::Cell<bool>,
}

impl AppPasteOperations<'_> {
    fn ensure_session(&self) -> Result<(), String> {
        if self.state.picker_session_is_current(self.generation) {
            Ok(())
        } else {
            Err("Picker session was dismissed".into())
        }
    }
}

impl PasteOperations for AppPasteOperations<'_> {
    fn write_item(&self, item: &PasteContent) -> Result<(), String> {
        self.ensure_session()?;
        let system_clipboard = self
            .state
            .system_clipboard
            .as_ref()
            .ok_or_else(|| "Clipboard write is unavailable".to_owned())?;

        system_clipboard
            .write_content(item)
            .map_err(|error| error.to_string())
    }

    fn move_to_top(&self, hash: &str) -> Result<(), String> {
        self.ensure_session()?;
        self.state
            .clipboard
            .move_to_top_by_hash(hash)
            .map_err(|error| error.to_string())?;
        self.reordered.set(true);
        Ok(())
    }

    fn restore_target(&self) -> Result<(), String> {
        self.ensure_session()?;
        self.state.expect_paste_focus_loss(true);
        restore_focused_window(self.state).map_err(|error| error.to_string())
    }

    fn hide_picker(&self) -> Result<(), String> {
        self.ensure_session()?;
        let window = get_main_window(self.app)
            .ok_or_else(|| String::from("Main picker window unavailable"))?;
        window.hide().map_err(|error| error.to_string())
    }

    fn simulate_input(&self) -> Result<(), String> {
        self.ensure_session()?;
        let mut guard = self
            .state
            .input
            .enigo
            .lock()
            .map_err(|_| "Input state is unavailable".to_string())?;
        let enigo = guard
            .as_mut()
            .ok_or_else(|| "Input simulation is unavailable".to_string())?;

        simulate_paste_input(enigo).map_err(|error| error.to_string())
    }
}

pub(crate) fn paste_with(
    operations: &impl PasteOperations,
    hash: &str,
    item: &PasteContent,
) -> Result<PasteOutcome, String> {
    if let Err(error_value) = operations.write_item(item) {
        error!(error:% = error_value; "Failed to write item to clipboard");
        return Err(error_value);
    }

    if let Err(error_value) = operations.move_to_top(hash) {
        error!(error:% = error_value; "Failed to move copied item to the top");
        return Ok(PasteOutcome::CopiedForManualPaste);
    }

    if let Err(error_value) = operations.restore_target() {
        error!(error:% = error_value; "Failed to restore paste target");
        return Ok(PasteOutcome::CopiedForManualPaste);
    }

    if let Err(error_value) = operations.hide_picker() {
        error!(error:% = error_value; "Failed to hide picker before automatic paste");
        return Ok(PasteOutcome::CopiedForManualPaste);
    }

    if let Err(error_value) = operations.simulate_input() {
        error!(error:% = error_value; "Failed to simulate paste input");
        return Ok(PasteOutcome::CopiedForManualPaste);
    }

    Ok(PasteOutcome::Pasted)
}

pub fn paste(app: &tauri::AppHandle, state: &AppState, hash: &str) -> Result<PasteOutcome, String> {
    let _paste_guard = state.begin_paste()?;
    let generation = state.picker_session();
    let item = match state.clipboard.load_for_paste(hash) {
        Ok(item) => item,
        Err(crate::storage::ClipboardError::ImageUnavailable {
            fallback_text: Some(text),
        }) => {
            log::warn!("Stored image unavailable; copying its text fallback");
            PasteContent::Text(text)
        }
        Err(error_value) => return Err(error_value.to_string()),
    };
    let operations = AppPasteOperations {
        app,
        state,
        generation,
        reordered: std::cell::Cell::new(false),
    };

    let can_paste_automatically = state.capability_is_available(DesktopCapability::AutomaticPaste)
        && has_focused_target(state);
    let outcome = if can_paste_automatically {
        paste_with(&operations, hash, &item)?
    } else {
        operations.write_item(&item)?;
        if let Err(error_value) = operations.move_to_top(hash) {
            error!(error:% = error_value; "Failed to move copied item to the top");
        }
        PasteOutcome::CopiedForManualPaste
    };

    if operations.reordered.get() {
        if let Err(error_value) = app.emit_clipboard_changed() {
            error!(error:debug = error_value; "Failed to emit clipboard changed event");
        }
    }
    if outcome == PasteOutcome::CopiedForManualPaste && can_paste_automatically {
        recover(app, generation).map_err(|error_value| {
            format!("Item copied, but picker recovery failed: {error_value}")
        })?;
    }
    Ok(outcome)
}

pub fn close(app: &tauri::AppHandle, state: &AppState) -> Result<(), String> {
    state.dismiss_picker_session();
    let window = match get_main_window(app) {
        Some(window) => window,
        None => {
            crate::window::clear_focus_target(state);
            return Err("Picker window unavailable".into());
        }
    };
    let hide_result = window.hide().map_err(|error_value| error_value.to_string());
    if let Err(error_value) = hide_result {
        crate::window::clear_focus_target(state);
        return Err(error_value);
    }
    let restoration = restore_focused_window(state).map_err(|error_value| error_value.to_string());
    crate::window::clear_focus_target(state);
    restoration
}

#[cfg(test)]
mod tests {
    use super::*;
    const PICKER_SIZE: (u32, u32) = (250, 350);

    #[test]
    fn repeated_shortcut_preserves_the_target_while_the_picker_is_focused() {
        assert!(should_capture_focus_target(&Ok::<bool, ()>(false)));
        assert!(!should_capture_focus_target(&Ok::<bool, ()>(true)));
        assert!(!should_capture_focus_target(&Err::<bool, ()>(())));
    }

    fn monitor(
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        work_area: PhysicalRectangle,
        is_primary: bool,
    ) -> MonitorGeometry {
        MonitorGeometry {
            bounds: PhysicalRectangle {
                x,
                y,
                width,
                height,
            },
            work_area,
            is_primary,
        }
    }

    fn work_area(x: i32, y: i32, width: u32, height: u32) -> PhysicalRectangle {
        PhysicalRectangle {
            x,
            y,
            width,
            height,
        }
    }

    fn position_for(
        pointer: Option<(i32, i32)>,
        monitors: &[MonitorGeometry],
    ) -> PhysicalPosition<i32> {
        let pointer = pointer.map(|(x, y)| PhysicalPosition::new(x, y));

        resolve_picker_position(pointer, monitors, PICKER_SIZE)
            .expect("a monitor should resolve a position")
    }

    fn is_within_work_area(
        position: PhysicalPosition<i32>,
        picker_size: (u32, u32),
        work_area: PhysicalRectangle,
    ) -> bool {
        let picker_right = i64::from(position.x) + i64::from(picker_size.0);
        let picker_bottom = i64::from(position.y) + i64::from(picker_size.1);
        let work_right = i64::from(work_area.x) + i64::from(work_area.width);
        let work_bottom = i64::from(work_area.y) + i64::from(work_area.height);

        i64::from(position.x) >= i64::from(work_area.x)
            && picker_right <= work_right
            && i64::from(position.y) >= i64::from(work_area.y)
            && picker_bottom <= work_bottom
    }

    fn intersects_work_area(
        position: PhysicalPosition<i32>,
        picker_size: (u32, u32),
        work_area: PhysicalRectangle,
    ) -> bool {
        let picker_right = i64::from(position.x) + i64::from(picker_size.0);
        let picker_bottom = i64::from(position.y) + i64::from(picker_size.1);
        let work_right = i64::from(work_area.x) + i64::from(work_area.width);
        let work_bottom = i64::from(work_area.y) + i64::from(work_area.height);

        i64::from(position.x) < work_right
            && picker_right > i64::from(work_area.x)
            && i64::from(position.y) < work_bottom
            && picker_bottom > i64::from(work_area.y)
    }

    #[test]
    fn positions_picker_on_primary_monitor_and_clamps_at_its_edges() {
        let primary = monitor(0, 0, 1920, 1080, work_area(0, 0, 1920, 1040), true);

        assert_eq!(
            position_for(Some((500, 400)), &[primary]),
            PhysicalPosition::new(500, 400)
        );
        assert_eq!(
            position_for(Some((1919, 1079)), &[primary]),
            PhysicalPosition::new(1670, 690)
        );
    }

    #[test]
    fn selects_secondary_monitors_with_positive_and_negative_origins() {
        let left = monitor(-1280, 0, 1280, 1024, work_area(-1280, 0, 1280, 1024), false);
        let primary = monitor(0, 0, 1920, 1080, work_area(0, 0, 1920, 1080), true);
        let right = monitor(1920, 0, 1600, 900, work_area(1920, 0, 1600, 900), false);

        assert_eq!(
            position_for(Some((-1200, 200)), &[left, primary, right]),
            PhysicalPosition::new(-1200, 200)
        );
        assert_eq!(
            position_for(Some((3500, 850)), &[left, primary, right]),
            PhysicalPosition::new(3270, 550)
        );
    }

    #[test]
    fn preserves_negative_vertical_origins() {
        let above_primary = monitor(0, -900, 1600, 900, work_area(0, -900, 1600, 900), false);
        let primary = monitor(0, 0, 1920, 1080, work_area(0, 0, 1920, 1080), true);

        assert_eq!(
            position_for(Some((400, -10)), &[above_primary, primary]),
            PhysicalPosition::new(400, -350)
        );
    }

    #[test]
    fn clamps_to_work_area_instead_of_full_monitor_bounds() {
        let primary = monitor(0, 0, 1920, 1080, work_area(0, 32, 1920, 1008), true);

        assert_eq!(
            position_for(Some((20, 10)), &[primary]),
            PhysicalPosition::new(20, 32)
        );
        assert_eq!(
            position_for(Some((1900, 1070)), &[primary]),
            PhysicalPosition::new(1670, 690)
        );
    }

    #[test]
    fn uses_measured_picker_size_on_mixed_scale_monitors() {
        let primary = monitor(0, 0, 1920, 1080, work_area(0, 0, 1920, 1080), true);
        // X11 can report the primary output's scale for every monitor even when
        // the secondary output transforms its pixels differently.
        let scaled_secondary = monitor(1920, 0, 2560, 1440, work_area(1920, 0, 2560, 1440), false);

        // Tauri has already converted the 250×350 logical picker size to
        // 375×525 physical pixels on this scaled display.
        let measured_physical_size = (375, 525);
        assert_eq!(
            resolve_picker_position(
                Some(PhysicalPosition::new(4400, 1300)),
                &[primary, scaled_secondary],
                measured_physical_size,
            )
            .expect("a monitor should resolve a position"),
            PhysicalPosition::new(4105, 915)
        );
    }

    #[test]
    fn falls_back_to_primary_or_first_monitor_when_pointer_is_unavailable() {
        let left = monitor(-1280, 0, 1280, 1024, work_area(-1280, 0, 1280, 1024), false);
        let negative_origin_primary =
            monitor(-1920, 0, 640, 480, work_area(-1920, 20, 640, 460), true);

        assert_eq!(
            position_for(None, &[left, negative_origin_primary]),
            PhysicalPosition::new(-1920, 20)
        );
        assert_eq!(position_for(None, &[left]), PhysicalPosition::new(-1280, 0));
    }

    #[test]
    fn falls_back_when_pointer_is_outside_every_monitor() {
        let primary = monitor(-1920, 0, 1920, 1080, work_area(-1920, 0, 1920, 1080), true);

        assert_eq!(
            position_for(Some((500, 500)), &[primary]),
            PhysicalPosition::new(-1920, 0)
        );
    }

    #[test]
    fn topology_matrix_keeps_a_fitting_picker_inside_each_work_area() {
        let monitors = [
            monitor(
                -1600,
                -900,
                1600,
                900,
                work_area(-1600, -900, 1600, 900),
                false,
            ),
            monitor(0, 0, 1920, 1080, work_area(0, 24, 1920, 1056), true),
            monitor(1920, 0, 2560, 1440, work_area(1920, 0, 2560, 1400), false),
        ];
        let pointer_positions = [
            (-1600, -900),
            (-1, -1),
            (0, 0),
            (1919, 1079),
            (1920, 0),
            (4479, 1439),
        ];

        for pointer in pointer_positions {
            let position = position_for(Some(pointer), &monitors);
            let selected_monitor = monitors
                .iter()
                .find(|monitor| {
                    monitor
                        .bounds
                        .contains(PhysicalPosition::new(pointer.0, pointer.1))
                })
                .expect("test pointer should be on a monitor");

            assert!(is_within_work_area(
                position,
                PICKER_SIZE,
                selected_monitor.work_area,
            ));
        }
    }

    #[test]
    fn oversized_picker_stays_visible_instead_of_resolving_outside_the_work_area() {
        let primary = monitor(-100, -100, 200, 200, work_area(-100, -100, 200, 200), true);
        let position = position_for(Some((-50, -50)), &[primary]);

        assert_eq!(position, PhysicalPosition::new(-100, -100));
        assert!(intersects_work_area(
            position,
            PICKER_SIZE,
            primary.work_area
        ));
    }
}
