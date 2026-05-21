// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

use std::{
    any::Any,
    env,
    ffi::OsStr,
    io,
    panic::{self, AssertUnwindSafe},
    path::{Path, PathBuf},
    process::{Command, ExitCode, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use nautilus_event_store::{
    EventStoreError, IndexDrift, Verifier, VerifyError, VerifyFinding, VerifyReport,
};

const EXIT_CLEAN: u8 = 0;
const EXIT_CORRUPT: u8 = 1;
const EXIT_ERROR: u8 = 2;
const WORKER_POLL_INTERVAL: Duration = Duration::from_millis(100);
const DEFAULT_WORKER_TIMEOUT: Duration = Duration::from_secs(30);
const TIMEOUT_ENV: &str = "NAUTILUS_EVENT_STORE_VERIFY_TIMEOUT_SECS";
#[cfg(debug_assertions)]
const WORKER_SLEEP_ENV: &str = "NAUTILUS_EVENT_STORE_VERIFY_SLEEP_WORKER_MS";

enum WorkerOutput {
    Exited(Output),
    TimedOut { output: Output, timeout: Duration },
}

fn main() -> ExitCode {
    let mut args = env::args_os();
    let program = args.next().unwrap_or_else(|| OsStr::new("verify").into());
    let Some(first) = args.next() else {
        print_usage(&program);
        return ExitCode::from(EXIT_ERROR);
    };

    if first.as_os_str() == OsStr::new("--worker") {
        let Some(path) = args.next() else {
            print_usage(&program);
            return ExitCode::from(EXIT_ERROR);
        };

        if args.next().is_some() {
            print_usage(&program);
            return ExitCode::from(EXIT_ERROR);
        }

        let path = PathBuf::from(path);
        return verify_run_file_worker(path.as_path());
    }

    if args.next().is_some() {
        print_usage(&program);
        return ExitCode::from(EXIT_ERROR);
    }

    let path = PathBuf::from(first);
    verify_run_file(path.as_path())
}

fn verify_run_file(path: &Path) -> ExitCode {
    let output = match run_worker(path) {
        Ok(output) => output,
        Err(e) => {
            eprintln!("error path={} error=\"worker: {e}\"", path.display());
            return ExitCode::from(EXIT_ERROR);
        }
    };

    match output {
        WorkerOutput::Exited(output) => classify_worker_exit(path, &output),
        WorkerOutput::TimedOut { output, timeout } => {
            println!(
                "corrupt path={} worker_status=\"timeout after {}s\" quarantine=not-performed",
                path.display(),
                timeout.as_secs(),
            );
            relay_output(&output);
            ExitCode::from(EXIT_CORRUPT)
        }
    }
}

fn classify_worker_exit(path: &Path, output: &Output) -> ExitCode {
    match output.status.code() {
        Some(code)
            if code == i32::from(EXIT_CLEAN)
                || code == i32::from(EXIT_CORRUPT)
                || code == i32::from(EXIT_ERROR) =>
        {
            relay_output(output);
            ExitCode::from(u8::try_from(code).expect("known verifier exit code"))
        }
        _ => {
            println!(
                "corrupt path={} worker_status=\"{}\" quarantine=not-performed",
                path.display(),
                output.status,
            );
            relay_output(output);
            ExitCode::from(EXIT_CORRUPT)
        }
    }
}

fn verify_run_file_worker(path: &Path) -> ExitCode {
    abort_worker_when_requested();
    sleep_worker_when_requested();

    let previous_hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let result = panic::catch_unwind(AssertUnwindSafe(|| verify_run_file_inner(path)));
    panic::set_hook(previous_hook);

    match result {
        Ok(code) => code,
        Err(payload) => {
            println!(
                "corrupt path={} panic=\"{}\" quarantine=not-performed",
                path.display(),
                panic_message(payload.as_ref()),
            );
            ExitCode::from(EXIT_CORRUPT)
        }
    }
}

fn verify_run_file_inner(path: &Path) -> ExitCode {
    match Verifier::open_redb_file(path).and_then(|verifier| verifier.verify()) {
        Ok(report) => print_report(&report),
        Err(e) => print_error(path, &e),
    }
}

fn print_report(report: &VerifyReport) -> ExitCode {
    if report.is_clean() {
        println!(
            "clean run_id={} status={:?} high_watermark={} entries_scanned={}",
            report.run_id, report.status, report.high_watermark, report.entries_scanned,
        );
        return ExitCode::from(EXIT_CLEAN);
    }

    println!(
        "corrupt run_id={} status={:?} high_watermark={} entries_scanned={} findings={} quarantine=not-performed",
        report.run_id,
        report.status,
        report.high_watermark,
        report.entries_scanned,
        report.findings.len(),
    );

    for finding in &report.findings {
        print_finding(finding);
    }

    ExitCode::from(EXIT_CORRUPT)
}

fn print_error(path: &Path, err: &VerifyError) -> ExitCode {
    if matches!(err, VerifyError::Backend(EventStoreError::Corrupted(_))) {
        println!(
            "corrupt path={} error=\"{}\" quarantine=not-performed",
            path.display(),
            err,
        );
        return ExitCode::from(EXIT_CORRUPT);
    }

    eprintln!("error path={} error=\"{}\"", path.display(), err);
    ExitCode::from(EXIT_ERROR)
}

fn print_finding(finding: &VerifyFinding) {
    match finding {
        VerifyFinding::HashMismatch { seq } => {
            println!("- hash mismatch at seq {seq}");
        }
        VerifyFinding::Gap { range } => {
            println!("- gap from seq {} to {}", range.from, range.to);
        }
        VerifyFinding::SeqMismatch {
            table_key,
            embedded_seq,
        } => {
            println!("- seq mismatch at table key {table_key}: embedded seq was {embedded_seq}");
        }
        VerifyFinding::IndexDrift { kind, key, drift } => {
            print_index_drift(*kind, key, *drift);
        }
        VerifyFinding::ManifestMismatch { kind, reason } => {
            println!("- manifest mismatch {kind:?}: {reason}");
        }
    }
}

fn print_index_drift(kind: nautilus_event_store::IndexKind, key: &str, drift: IndexDrift) {
    match drift {
        IndexDrift::DanglingTarget { stored_seq } => {
            println!("- index drift {kind:?} key={key}: dangling target seq {stored_seq}");
        }
        IndexDrift::TargetCorrupted { stored_seq } => {
            println!("- index drift {kind:?} key={key}: corrupted target seq {stored_seq}");
        }
    }
}

fn print_usage(program: &OsStr) {
    eprintln!("usage: {} <run-file.redb>", program.to_string_lossy());
}

fn run_worker(path: &Path) -> io::Result<WorkerOutput> {
    let current_exe = env::current_exe()?;
    let mut child = Command::new(current_exe)
        .arg("--worker")
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let timeout = worker_timeout();
    let deadline = Instant::now() + timeout;

    loop {
        if child.try_wait()?.is_some() {
            return child.wait_with_output().map(WorkerOutput::Exited);
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            return child
                .wait_with_output()
                .map(|output| WorkerOutput::TimedOut { output, timeout });
        }

        thread::sleep(WORKER_POLL_INTERVAL);
    }
}

fn relay_output(output: &Output) {
    if !output.stdout.is_empty() {
        print!("{}", String::from_utf8_lossy(&output.stdout));
    }

    if !output.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
    }
}

fn panic_message(payload: &(dyn Any + Send)) -> &str {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        message
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.as_str()
    } else {
        "unknown panic"
    }
}

fn worker_timeout() -> Duration {
    env::var(TIMEOUT_ENV)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map_or(DEFAULT_WORKER_TIMEOUT, Duration::from_secs)
}

fn abort_worker_when_requested() {
    #[cfg(debug_assertions)]
    if env::var_os("NAUTILUS_EVENT_STORE_VERIFY_ABORT_WORKER").is_some() {
        std::process::abort();
    }
}

fn sleep_worker_when_requested() {
    #[cfg(debug_assertions)]
    if let Some(raw) = env::var_os(WORKER_SLEEP_ENV)
        && let Ok(ms) = raw.to_string_lossy().parse::<u64>()
    {
        thread::sleep(Duration::from_millis(ms));
    }
}
