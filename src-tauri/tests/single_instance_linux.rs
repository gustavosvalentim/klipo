#![cfg(all(
    target_os = "linux",
    debug_assertions,
    feature = "single-instance-test"
))]

use std::{
    fs,
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const SHUTDOWN_SIGNAL_ENV: &str = "KLIPO_SINGLE_INSTANCE_TEST_SHUTDOWN_SIGNAL";
const TRACE_PATH_ENV: &str = "KLIPO_SINGLE_INSTANCE_TEST_TRACE_PATH";

#[test]
fn repeated_launch_uses_the_primary_instance_and_exit_releases_the_endpoint() {
    let temporary_directory = tempfile::tempdir().expect("temporary directory");
    let trace_path = temporary_directory.path().join("single-instance.trace");
    let first_shutdown_signal = temporary_directory.path().join("first.shutdown");

    let mut first_instance = ManagedChild::launch(&trace_path, &first_shutdown_signal);
    wait_for_events(&trace_path, 1, 1, 0, 1);

    let second_shutdown_signal = temporary_directory.path().join("second.shutdown");
    fs::write(&second_shutdown_signal, b"").expect("prepare secondary shutdown signal");
    let mut second_instance = ManagedChild::launch(&trace_path, &second_shutdown_signal);
    second_instance.wait_success();
    wait_for_events(&trace_path, 1, 1, 1, 1);

    fs::write(&first_shutdown_signal, b"").expect("request primary shutdown");
    first_instance.wait_success();

    let third_shutdown_signal = temporary_directory.path().join("third.shutdown");
    let mut third_instance = ManagedChild::launch(&trace_path, &third_shutdown_signal);
    wait_for_events(&trace_path, 2, 2, 1, 2);
    fs::write(&third_shutdown_signal, b"").expect("request later primary shutdown");
    third_instance.wait_success();
}

struct ManagedChild {
    child: Option<Child>,
    shutdown_signal: std::path::PathBuf,
}

impl ManagedChild {
    fn launch(trace_path: &Path, shutdown_signal: &Path) -> Self {
        let child = Command::new(env!("CARGO_BIN_EXE_klipo"))
            .env(TRACE_PATH_ENV, trace_path)
            .env(SHUTDOWN_SIGNAL_ENV, shutdown_signal)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("launch Klipo");

        Self {
            child: Some(child),
            shutdown_signal: shutdown_signal.to_path_buf(),
        }
    }

    fn wait_success(&mut self) {
        let output = self
            .child
            .take()
            .expect("child remains managed until it exits")
            .wait_with_output()
            .expect("managed child exits");

        assert!(
            output.status.success(),
            "managed child exits successfully; status={}; stdout={}; stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
}

impl Drop for ManagedChild {
    fn drop(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };

        let _ = fs::write(&self.shutdown_signal, b"");
        let deadline = Instant::now() + Duration::from_secs(2);

        while Instant::now() < deadline {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => thread::sleep(Duration::from_millis(25)),
                Err(_) => break,
            }
        }

        let _ = child.kill();
        let _ = child.wait();
    }
}

fn wait_for_events(
    trace_path: &Path,
    setups: usize,
    resources: usize,
    activations: usize,
    picker_ready: usize,
) {
    let timeout = Instant::now() + Duration::from_secs(10);

    loop {
        let trace = fs::read_to_string(trace_path).unwrap_or_default();
        let setup_count = trace.lines().filter(|event| *event == "setup").count();
        let resource_count = trace.lines().filter(|event| *event == "resource").count();
        let activation_count = trace.lines().filter(|event| *event == "activation").count();
        let picker_ready_count = trace
            .lines()
            .filter(|event| *event == "picker-ready")
            .count();

        if setup_count == setups
            && resource_count == resources
            && activation_count == activations
            && picker_ready_count == picker_ready
        {
            return;
        }

        assert!(
            Instant::now() < timeout,
            "expected setup={setups}, resource={resources}, activation={activations}, picker-ready={picker_ready}; trace was {trace:?}"
        );
        thread::sleep(Duration::from_millis(25));
    }
}
