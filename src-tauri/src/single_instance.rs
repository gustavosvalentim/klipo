use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[cfg(target_os = "linux")]
use log::error;
#[cfg(target_os = "linux")]
use std::sync::Arc;

/// Coordinates picker activations without blocking the Tauri event loop.
#[derive(Default)]
pub struct PickerActivation {
    ready: AtomicBool,
    requested_generation: AtomicU64,
    delivered_generation: AtomicU64,
}

impl PickerActivation {
    pub fn activate_existing_instance<E>(
        &self,
        show_picker: impl FnOnce() -> Result<(), E>,
    ) -> Result<(), E> {
        let generation = self.requested_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let picker_is_ready = self.ready.load(Ordering::SeqCst);

        if picker_is_ready {
            self.deliver(generation, show_picker)
        } else {
            Ok(())
        }
    }

    pub fn flush<E>(&self, show_picker: impl FnOnce() -> Result<(), E>) -> Result<(), E> {
        self.ready.store(true, Ordering::SeqCst);

        let generation = self.requested_generation.load(Ordering::SeqCst);

        self.deliver(generation, show_picker)
    }

    fn deliver<E>(
        &self,
        generation: u64,
        show_picker: impl FnOnce() -> Result<(), E>,
    ) -> Result<(), E> {
        let delivered_generation = self.delivered_generation.load(Ordering::SeqCst);

        if delivered_generation < generation {
            let delivery = show_picker();

            if delivery.is_ok() {
                self.delivered_generation
                    .fetch_max(generation, Ordering::SeqCst);
            }

            delivery
        } else {
            Ok(())
        }
    }
}

#[cfg(target_os = "linux")]
pub fn register(
    builder: tauri::Builder<tauri::Wry>,
    picker_activation: Arc<PickerActivation>,
) -> tauri::Builder<tauri::Wry> {
    builder.plugin(tauri_plugin_single_instance::init(move |app, _, _| {
        #[cfg(all(debug_assertions, feature = "single-instance-test"))]
        let delivery =
            picker_activation.activate_existing_instance(|| test_support::show_picker_window(app));

        #[cfg(not(all(debug_assertions, feature = "single-instance-test")))]
        let delivery =
            picker_activation.activate_existing_instance(|| crate::window::show_picker_window(app));

        if let Err(error_value) = delivery {
            error!(error:debug = error_value; "Failed to activate picker window");
        }
    }))
}

pub fn run_primary_setup<T, E, D>(
    picker_activation: &PickerActivation,
    initialize_resources: impl FnOnce() -> Result<T, E>,
    show_picker: impl FnOnce() -> Result<(), D>,
    on_delivery_error: impl FnOnce(&D),
) -> Result<T, E> {
    let resources = initialize_resources()?;

    if let Err(error_value) = picker_activation.flush(show_picker) {
        on_delivery_error(&error_value);
    }

    Ok(resources)
}

#[cfg(all(
    target_os = "linux",
    debug_assertions,
    feature = "single-instance-test"
))]
pub mod test_support {
    use std::{
        fs::OpenOptions,
        io::Write,
        path::PathBuf,
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };

    use crate::state::AppState;
    use tauri::Manager;

    const SHUTDOWN_SIGNAL_ENV: &str = "KLIPO_SINGLE_INSTANCE_TEST_SHUTDOWN_SIGNAL";
    const TRACE_PATH_ENV: &str = "KLIPO_SINGLE_INSTANCE_TEST_TRACE_PATH";

    pub fn enabled() -> bool {
        trace_path().is_some()
    }

    pub fn initialize_primary_resources(
        app: &tauri::App,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let trace_path = trace_path().expect("test resources require a trace path");
        record("setup");

        let database_path = trace_path.with_extension("sqlite3");
        AppState::new(database_path)?;
        record("resource");
        crate::window::create_picker_window(&app.handle())
            .map_err(|error_value| std::io::Error::other(error_value.to_string()))?;
        record("picker-ready");

        let shutdown_signal = std::env::var_os(SHUTDOWN_SIGNAL_ENV)
            .map(PathBuf::from)
            .expect("test resources require a shutdown signal");
        let app_handle = app.handle().clone();

        thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(30);

            while !shutdown_signal.exists() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(25));
            }

            app_handle.exit(0);
        });

        Ok(())
    }

    pub fn show_picker_window(app: &tauri::AppHandle) -> Result<(), tauri::Error> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let app_handle = app.clone();

        app.run_on_main_thread(move || {
            let delivered = crate::window::show_picker_window(&app_handle).is_ok();
            let _ = sender.send(delivered);
        })?;

        let delivered = receiver
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| tauri::Error::WindowNotFound)?;

        if !delivered {
            return Err(tauri::Error::WindowNotFound);
        }

        let window = app
            .get_webview_window("main")
            .ok_or(tauri::Error::WindowNotFound)?;
        let deadline = Instant::now() + Duration::from_secs(2);

        while Instant::now() < deadline {
            if window.is_visible()? && window.is_focused()? {
                record("activation");
                return Ok(());
            }

            thread::sleep(Duration::from_millis(25));
        }

        Err(tauri::Error::WindowNotFound)
    }

    pub fn record(event: &str) {
        if let Some(trace_path) = trace_path() {
            let result = OpenOptions::new()
                .create(true)
                .append(true)
                .open(trace_path)
                .and_then(|mut trace| writeln!(trace, "{event}"));

            if let Err(error_value) = result {
                error!(error:debug = error_value; "Failed to write single-instance test trace");
            }
        }
    }

    fn trace_path() -> Option<PathBuf> {
        std::env::var_os(TRACE_PATH_ENV).map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        sync::{
            atomic::{AtomicUsize, Ordering},
            mpsc, Arc, Barrier,
        },
        thread,
        time::Duration,
    };

    use super::{run_primary_setup, PickerActivation};

    #[test]
    fn pre_ready_activation_waits_for_primary_setup_to_flush_it() {
        let picker_activation = PickerActivation::default();
        let stages = RefCell::new(Vec::new());

        assert!(picker_activation
            .activate_existing_instance(|| {
                stages.borrow_mut().push("unexpected-picker");
                Ok::<_, ()>(())
            })
            .is_ok());
        assert!(stages.borrow().is_empty());

        let result = run_primary_setup(
            &picker_activation,
            || {
                stages.borrow_mut().push("resources");
                Ok::<_, ()>(())
            },
            || {
                stages.borrow_mut().push("picker");
                Ok::<_, ()>(())
            },
            |_| {},
        );

        assert!(result.is_ok());
        assert_eq!(stages.into_inner(), ["resources", "picker"]);
    }

    #[test]
    fn readiness_and_activation_overlap_without_losing_the_activation() {
        let picker_activation = Arc::new(PickerActivation::default());
        let start = Arc::new(Barrier::new(3));
        let show_attempts = Arc::new(AtomicUsize::new(0));
        let delivery_activation = Arc::clone(&picker_activation);
        let delivery_start = Arc::clone(&start);
        let delivery_attempts = Arc::clone(&show_attempts);

        let delivery_thread = thread::spawn(move || {
            delivery_start.wait();

            delivery_activation.activate_existing_instance(|| {
                delivery_attempts.fetch_add(1, Ordering::SeqCst);
                Ok::<_, ()>(())
            })
        });
        let flush_activation = Arc::clone(&picker_activation);
        let flush_start = Arc::clone(&start);
        let flush_attempts = Arc::clone(&show_attempts);

        let flush_thread = thread::spawn(move || {
            flush_start.wait();

            flush_activation.flush(|| {
                flush_attempts.fetch_add(1, Ordering::SeqCst);
                Ok::<_, ()>(())
            })
        });

        start.wait();
        let delivery_result = delivery_thread.join().expect("delivery thread completes");
        let flush_result = flush_thread.join().expect("flush thread completes");

        assert!(delivery_result.is_ok());
        assert!(flush_result.is_ok());
        assert!(show_attempts.load(Ordering::SeqCst) >= 1);
        assert_eq!(
            picker_activation
                .requested_generation
                .load(Ordering::SeqCst),
            picker_activation
                .delivered_generation
                .load(Ordering::SeqCst)
        );
    }

    #[test]
    fn failed_later_activation_remains_pending_after_an_older_delivery_succeeds() {
        let picker_activation = Arc::new(PickerActivation::default());
        let (flush_started_sender, flush_started_receiver) = mpsc::sync_channel(1);
        let (release_flush_sender, release_flush_receiver) = mpsc::sync_channel(1);

        assert!(picker_activation
            .activate_existing_instance(|| -> Result<(), ()> {
                panic!("pre-ready activation must not show the picker")
            })
            .is_ok());

        let flush_activation = Arc::clone(&picker_activation);
        let flush_thread = thread::spawn(move || {
            flush_activation.flush(|| {
                let _ = flush_started_sender.send(());
                let _ = release_flush_receiver.recv();
                Ok::<_, ()>(())
            })
        });

        let flush_started = flush_started_receiver
            .recv_timeout(Duration::from_secs(1))
            .is_ok();
        let (concurrent_delivery_sender, concurrent_delivery_receiver) = mpsc::sync_channel(1);
        let concurrent_activation = Arc::clone(&picker_activation);
        let concurrent_delivery_thread = thread::spawn(move || {
            let delivery = concurrent_activation.activate_existing_instance(|| Err::<(), _>(()));
            let _ = concurrent_delivery_sender.send(());
            delivery
        });
        let concurrent_delivery_completed_before_flush = concurrent_delivery_receiver
            .recv_timeout(Duration::from_secs(1))
            .is_ok();

        let _ = release_flush_sender.send(());
        let flush_result = flush_thread.join().expect("flush thread completes");
        let concurrent_delivery = concurrent_delivery_thread
            .join()
            .expect("concurrent delivery thread completes");

        assert!(flush_started, "flush delivers the pre-ready activation");
        assert!(
            concurrent_delivery_completed_before_flush,
            "the concurrent delivery does not wait for the flush"
        );
        assert!(concurrent_delivery.is_err());
        assert!(flush_result.is_ok());
        assert_eq!(
            picker_activation
                .requested_generation
                .load(Ordering::SeqCst),
            2
        );
        assert_eq!(
            picker_activation
                .delivered_generation
                .load(Ordering::SeqCst),
            1
        );
    }

    #[test]
    fn flush_does_not_wait_for_an_in_progress_picker_delivery() {
        let picker_activation = Arc::new(PickerActivation::default());
        assert!(picker_activation.flush(|| Ok::<_, ()>(())).is_ok());

        let (delivery_started_sender, delivery_started_receiver) = mpsc::sync_channel(1);
        let (release_delivery_sender, release_delivery_receiver) = mpsc::sync_channel(1);
        let delivery_activation = Arc::clone(&picker_activation);

        let delivery_thread = thread::spawn(move || {
            delivery_activation.activate_existing_instance(|| {
                let _ = delivery_started_sender.send(());
                let _ = release_delivery_receiver.recv();
                Ok::<_, ()>(())
            })
        });

        let delivery_started = delivery_started_receiver
            .recv_timeout(Duration::from_secs(1))
            .is_ok();
        let (flush_completed_sender, flush_completed_receiver) = mpsc::sync_channel(1);
        let flush_activation = Arc::clone(&picker_activation);

        let flush_thread = thread::spawn(move || {
            let flush_result = flush_activation.flush(|| Ok::<_, ()>(()));
            let _ = flush_completed_sender.send(flush_result);
        });

        let flush_completed_before_delivery = flush_completed_receiver
            .recv_timeout(Duration::from_secs(1))
            .is_ok();

        let _ = release_delivery_sender.send(());
        let delivery_result = delivery_thread.join().expect("delivery thread completes");
        flush_thread.join().expect("flush thread completes");

        assert!(delivery_started, "delivery begins before flushing");
        assert!(delivery_result.is_ok());
        assert!(
            flush_completed_before_delivery,
            "flush completes without waiting for the in-progress delivery"
        );
    }
}
