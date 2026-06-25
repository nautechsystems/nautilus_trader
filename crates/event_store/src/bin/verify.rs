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
    EventStoreError, IndexDrift, MarkerCountKind, MarkerFinding, MarkerRecordKind, MarkerVerifier,
    MarkerVerifyReport, RedbMarkerBackend, Verifier, VerifyError, VerifyFinding, VerifyReport,
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

enum MarkerScan {
    Absent,
    Present(MarkerVerifyReport),
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
        Ok(report) => match scan_marker_sidecar(path, &report) {
            Ok(markers) => print_report(&report, &markers),
            Err((marker_path, err)) => print_marker_error(marker_path.as_path(), &err),
        },
        Err(e) => print_error(path, &e),
    }
}

fn scan_marker_sidecar(
    path: &Path,
    entry_report: &VerifyReport,
) -> Result<MarkerScan, (PathBuf, VerifyError)> {
    let Some(marker_path) = marker_sidecar_path(path) else {
        return Ok(MarkerScan::Absent);
    };

    if !marker_path.exists() {
        return Ok(MarkerScan::Absent);
    }

    let backend = RedbMarkerBackend::open_read_only_file(&marker_path)
        .map_err(|e| (marker_path.clone(), VerifyError::Backend(e)))?;
    let report = MarkerVerifier::scan(&backend, entry_report.high_watermark)
        .map_err(|e| (marker_path.clone(), VerifyError::Backend(e)))?;
    Ok(MarkerScan::Present(report))
}

fn marker_sidecar_path(path: &Path) -> Option<PathBuf> {
    let stem = path.file_stem()?;
    let mut file_name = stem.to_os_string();
    file_name.push(".markers.redb");
    Some(path.with_file_name(file_name))
}

fn print_report(report: &VerifyReport, markers: &MarkerScan) -> ExitCode {
    let marker_findings = marker_finding_count(markers);

    if report.is_clean() && marker_findings == 0 {
        println!(
            "clean run_id={} status={:?} high_watermark={} entries_scanned={} {}",
            report.run_id,
            report.status,
            report.high_watermark,
            report.entries_scanned,
            marker_summary(markers),
        );
        return ExitCode::from(EXIT_CLEAN);
    }

    println!(
        "corrupt run_id={} status={:?} high_watermark={} entries_scanned={} findings={} marker_findings={} {} quarantine=not-performed",
        report.run_id,
        report.status,
        report.high_watermark,
        report.entries_scanned,
        report.findings.len() + marker_findings,
        marker_findings,
        marker_summary(markers),
    );

    for finding in &report.findings {
        print_finding(finding);
    }

    if let MarkerScan::Present(report) = markers {
        for finding in &report.findings {
            print_marker_finding(finding);
        }
    }

    ExitCode::from(EXIT_CORRUPT)
}

fn marker_finding_count(markers: &MarkerScan) -> usize {
    match markers {
        MarkerScan::Absent => 0,
        MarkerScan::Present(report) => report.findings.len(),
    }
}

fn marker_summary(markers: &MarkerScan) -> String {
    match markers {
        MarkerScan::Absent => "markers=absent".to_string(),
        MarkerScan::Present(report) if report.is_clean() => format!(
            "markers=clean marker_run_id={} marker_status={:?} marker_snapshots_scanned={} marker_hifi_scanned={} marker_gaps_scanned={} marker_dict_entries_scanned={}",
            report.run_id,
            report.status,
            report.snapshots_scanned,
            report.hifi_scanned,
            report.gaps_scanned,
            report.dict_entries_scanned,
        ),
        MarkerScan::Present(report) => format!(
            "markers=corrupt marker_run_id={} marker_status={:?} marker_snapshots_scanned={} marker_hifi_scanned={} marker_gaps_scanned={} marker_dict_entries_scanned={} marker_findings={}",
            report.run_id,
            report.status,
            report.snapshots_scanned,
            report.hifi_scanned,
            report.gaps_scanned,
            report.dict_entries_scanned,
            report.findings.len(),
        ),
    }
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

fn print_marker_error(path: &Path, err: &VerifyError) -> ExitCode {
    if matches!(err, VerifyError::Backend(EventStoreError::Corrupted(_))) {
        println!(
            "corrupt path={} markers=error error=\"{}\" quarantine=not-performed",
            path.display(),
            err,
        );
        return ExitCode::from(EXIT_CORRUPT);
    }

    eprintln!(
        "error path={} markers=error error=\"{}\"",
        path.display(),
        err
    );
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
        VerifyFinding::SnapshotAnchorInvalid { reason } => {
            println!("- snapshot anchor invalid: {reason}");
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

fn print_marker_finding(finding: &MarkerFinding) {
    match finding {
        MarkerFinding::ManifestCountMismatch {
            kind,
            manifest_count,
            scanned_count,
        } => {
            println!(
                "- marker manifest count mismatch {}: manifest={manifest_count} scanned={scanned_count}",
                marker_count_name(*kind),
            );
        }
        MarkerFinding::MarkerSeqGap {
            from_marker_seq,
            to_marker_seq,
        } => {
            println!("- marker seq gap from {from_marker_seq} to {to_marker_seq}");
        }
        MarkerFinding::MarkerSeqOverlap {
            from_marker_seq,
            to_marker_seq,
        } => {
            println!("- marker seq overlap from {from_marker_seq} to {to_marker_seq}");
        }
        MarkerFinding::InvalidMarkerGap {
            from_marker_seq,
            to_marker_seq,
        } => {
            println!("- invalid marker gap from {from_marker_seq} to {to_marker_seq}");
        }
        MarkerFinding::EventSeqRegressed {
            marker_seq,
            previous_event_seq_before,
            event_seq_before,
        } => {
            println!(
                "- marker event seq regressed marker_seq={marker_seq}: previous={previous_event_seq_before} current={event_seq_before}",
            );
        }
        MarkerFinding::EventSeqExceedsHighWatermark {
            marker_seq,
            event_seq_before,
            high_watermark,
        } => {
            println!(
                "- marker event seq exceeds high watermark marker_seq={marker_seq}: event_seq_before={event_seq_before} high_watermark={high_watermark}",
            );
        }
        MarkerFinding::CursorCountRegressed {
            marker_seq,
            slot,
            previous_count,
            count,
        } => {
            println!(
                "- marker cursor count regressed marker_seq={marker_seq} slot={slot}: previous={previous_count} current={count}",
            );
        }
        MarkerFinding::CursorTsInitRegressed {
            marker_seq,
            slot,
            previous_ts_init_hi,
            ts_init_hi,
        } => {
            println!(
                "- marker cursor ts_init_hi regressed marker_seq={marker_seq} slot={slot}: previous={} current={}",
                previous_ts_init_hi.as_u64(),
                ts_init_hi.as_u64(),
            );
        }
        MarkerFinding::HashMismatch {
            record,
            marker_seq,
            slot,
        } => {
            print_marker_hash_mismatch(*record, *marker_seq, *slot);
        }
    }
}

fn print_marker_hash_mismatch(
    record: MarkerRecordKind,
    marker_seq: Option<u64>,
    slot: Option<u32>,
) {
    match (marker_seq, slot) {
        (Some(marker_seq), Some(slot)) => println!(
            "- marker hash mismatch {} marker_seq={marker_seq} slot={slot}",
            marker_record_name(record),
        ),
        (Some(marker_seq), None) => println!(
            "- marker hash mismatch {} marker_seq={marker_seq}",
            marker_record_name(record),
        ),
        (None, Some(slot)) => println!(
            "- marker hash mismatch {} slot={slot}",
            marker_record_name(record),
        ),
        (None, None) => println!("- marker hash mismatch {}", marker_record_name(record)),
    }
}

fn marker_record_name(record: MarkerRecordKind) -> &'static str {
    match record {
        MarkerRecordKind::Snapshot => "snapshot",
        MarkerRecordKind::HiFi => "hifi",
        MarkerRecordKind::Gap => "gap",
        MarkerRecordKind::Dict => "dict",
    }
}

fn marker_count_name(kind: MarkerCountKind) -> &'static str {
    match kind {
        MarkerCountKind::Snapshot => "snapshot",
        MarkerCountKind::HiFi => "hifi",
        MarkerCountKind::Gap => "gap",
        MarkerCountKind::Dict => "dict",
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
