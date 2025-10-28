// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
    cmp,
    fs::{File, OpenOptions},
    io::{BufReader, BufWriter, Read, copy},
    path::Path,
    thread::sleep,
    time::{Duration, Instant},
};

use aws_lc_rs::digest::{self, Context};
use nautilus_network::retry::RetryConfig;
use rand::{Rng, rng};
use reqwest::blocking::Client;
use serde_json::Value;

#[derive(Debug)]
enum DownloadError {
    Retryable(String),
    NonRetryable(String),
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Retryable(msg) => write!(f, "Retryable error: {msg}"),
            Self::NonRetryable(msg) => write!(f, "Non-retryable error: {msg}"),
        }
    }
}

impl std::error::Error for DownloadError {}

fn execute_with_retry_blocking<T, E, F>(
    config: &RetryConfig,
    mut op: F,
    should_retry: impl Fn(&E) -> bool,
) -> Result<T, E>
where
    E: std::error::Error,
    F: FnMut() -> Result<T, E>,
{
    let start = Instant::now();
    let mut delay = Duration::from_millis(config.initial_delay_ms);

    for attempt in 0..=config.max_retries {
        if attempt > 0 && !config.immediate_first {
            let jitter = rng().random_range(0..=config.jitter_ms);
            let sleep_for = delay + Duration::from_millis(jitter);
            sleep(sleep_for);
            let next = (delay.as_millis() as f64 * config.backoff_factor) as u64;
            delay = cmp::min(
                Duration::from_millis(next),
                Duration::from_millis(config.max_delay_ms),
            );
        }

        if let Some(max_total) = config.max_elapsed_ms
            && start.elapsed() >= Duration::from_millis(max_total)
        {
            break;
        }

        match op() {
            Ok(v) => return Ok(v),
            Err(e) if attempt < config.max_retries && should_retry(&e) => continue,
            Err(e) => return Err(e),
        }
    }

    op()
}

/// Ensures that a file exists at the specified path by downloading it if necessary.
///
/// If the file already exists, it checks the integrity of the file using a SHA-256 checksum
/// from the optional `checksums` file. If the checksum is valid, the function exits early. If
/// the checksum is invalid or missing, the function updates the checksums file with the correct
/// hash for the existing file without redownloading it.
///
/// If the file does not exist, it downloads the file from the specified `url` and updates the
/// checksums file (if provided) with the calculated SHA-256 checksum of the downloaded file.
///
/// The `timeout_secs` parameter specifies the timeout in seconds for the HTTP request.
/// If `None` is provided, a default timeout of 30 seconds will be used.
///
/// # Errors
///
/// Returns an error if:
/// - The HTTP request cannot be sent or returns a non-success status code.
/// - Any I/O operation fails during file creation, reading, or writing.
/// - Checksum verification or JSON parsing fails.
pub fn ensure_file_exists_or_download_http(
    filepath: &Path,
    url: &str,
    checksums: Option<&Path>,
    timeout_secs: Option<u64>,
) -> anyhow::Result<()> {
    ensure_file_exists_or_download_http_with_config(
        filepath,
        url,
        checksums,
        timeout_secs.unwrap_or(30),
        None,
        None,
    )
}

/// Ensures that a file exists at the specified path by downloading it if necessary, with a custom timeout.
///
/// # Errors
///
/// Returns an error if:
/// - The HTTP request cannot be sent or returns a non-success status code after retries.
/// - Any I/O operation fails during file creation, reading, or writing.
/// - Checksum verification or JSON parsing fails.
pub fn ensure_file_exists_or_download_http_with_timeout(
    filepath: &Path,
    url: &str,
    checksums: Option<&Path>,
    timeout_secs: u64,
) -> anyhow::Result<()> {
    ensure_file_exists_or_download_http_with_config(
        filepath,
        url,
        checksums,
        timeout_secs,
        None,
        None,
    )
}

/// Ensures that a file exists at the specified path by downloading it if necessary,
/// with custom timeout, retry config, and initial jitter delay.
///
/// # Parameters
///
/// - `filepath`: The path where the file should exist.
/// - `url`: The URL to download from if the file doesn't exist.
/// - `checksums`: Optional path to checksums file for verification.
/// - `timeout_secs`: Timeout in seconds for HTTP requests.
/// - `retry_config`: Optional custom retry configuration (uses sensible defaults if None).
/// - `initial_jitter_ms`: Optional initial jitter delay in milliseconds before download (defaults to 100-600ms if None).
///
/// # Errors
///
/// Returns an error if:
/// - The HTTP request cannot be sent or returns a non-success status code after retries.
/// - Any I/O operation fails during file creation, reading, or writing.
/// - Checksum verification or JSON parsing fails.
pub fn ensure_file_exists_or_download_http_with_config(
    filepath: &Path,
    url: &str,
    checksums: Option<&Path>,
    timeout_secs: u64,
    retry_config: Option<RetryConfig>,
    initial_jitter_ms: Option<u64>,
) -> anyhow::Result<()> {
    if filepath.exists() {
        println!("File already exists: {filepath:?}");

        if let Some(checksums_file) = checksums {
            if verify_sha256_checksum(filepath, checksums_file)? {
                println!("File is valid");
                return Ok(());
            } else {
                let new_checksum = calculate_sha256(filepath)?;
                println!("Adding checksum for existing file: {new_checksum}");
                update_sha256_checksums(filepath, checksums_file, &new_checksum)?;
                return Ok(());
            }
        }
        return Ok(());
    }

    // Add a small random delay to avoid bursting the remote server when
    // many downloads start concurrently. Can be disabled by passing Some(0).
    if let Some(jitter_ms) = initial_jitter_ms {
        if jitter_ms > 0 {
            sleep(Duration::from_millis(jitter_ms));
        }
    } else {
        let jitter_delay = {
            let mut r = rng();
            Duration::from_millis(r.random_range(100..=600))
        };
        sleep(jitter_delay);
    }

    download_file(filepath, url, timeout_secs, retry_config)?;

    if let Some(checksums_file) = checksums {
        let new_checksum = calculate_sha256(filepath)?;
        update_sha256_checksums(filepath, checksums_file, &new_checksum)?;
    }

    Ok(())
}

fn download_file(
    filepath: &Path,
    url: &str,
    timeout_secs: u64,
    retry_config: Option<RetryConfig>,
) -> anyhow::Result<()> {
    println!("Downloading file from {url} to {filepath:?}");

    if let Some(parent) = filepath.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let client = Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()?;

    let cfg = if let Some(config) = retry_config {
        config
    } else {
        // Default production config
        let max_retries = 5u32;
        let op_timeout_ms = timeout_secs.saturating_mul(1000);
        // Make the provided timeout a hard ceiling for total elapsed time.
        // Split it across attempts (at least 1000 ms per attempt) and cap total at op_timeout_ms.
        let per_attempt_ms = std::cmp::max(1000u64, op_timeout_ms / (max_retries as u64 + 1));
        RetryConfig {
            max_retries,
            initial_delay_ms: 1_000,
            max_delay_ms: 10_000,
            backoff_factor: 2.0,
            jitter_ms: 1_000,
            operation_timeout_ms: Some(per_attempt_ms),
            immediate_first: false,
            max_elapsed_ms: Some(op_timeout_ms),
        }
    };

    let op = || -> Result<(), DownloadError> {
        match client.get(url).send() {
            Ok(mut response) => {
                let status = response.status();
                if status.is_success() {
                    let mut out = File::create(filepath)
                        .map_err(|e| DownloadError::NonRetryable(e.to_string()))?;
                    // Stream the response body directly to disk to avoid large allocations
                    copy(&mut response, &mut out)
                        .map_err(|e| DownloadError::NonRetryable(e.to_string()))?;
                    println!("File downloaded to {filepath:?}");
                    Ok(())
                } else if status.is_server_error()
                    || status.as_u16() == 429
                    || status.as_u16() == 408
                {
                    println!("HTTP error {status}, retrying...");
                    Err(DownloadError::Retryable(format!("HTTP {status}")))
                } else {
                    // Preserve existing error text used by tests
                    Err(DownloadError::NonRetryable(format!(
                        "Client error: HTTP {status}"
                    )))
                }
            }
            Err(e) => {
                println!("Request failed: {e}");
                Err(DownloadError::Retryable(e.to_string()))
            }
        }
    };

    let should_retry = |e: &DownloadError| matches!(e, DownloadError::Retryable(_));

    execute_with_retry_blocking(&cfg, op, should_retry).map_err(|e| anyhow::anyhow!(e.to_string()))
}

fn calculate_sha256(filepath: &Path) -> anyhow::Result<String> {
    let mut file = File::open(filepath)?;
    let mut ctx = Context::new(&digest::SHA256);
    let mut buffer = [0u8; 4096];

    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        ctx.update(&buffer[..count]);
    }

    let digest = ctx.finish();
    Ok(hex::encode(digest.as_ref()))
}

fn verify_sha256_checksum(filepath: &Path, checksums: &Path) -> anyhow::Result<bool> {
    let file = File::open(checksums)?;
    let reader = BufReader::new(file);
    let checksums: Value = serde_json::from_reader(reader)?;

    let filename = filepath.file_name().unwrap().to_str().unwrap();
    if let Some(expected_checksum) = checksums.get(filename) {
        let expected_checksum_str = expected_checksum.as_str().unwrap();
        let expected_hash = expected_checksum_str
            .strip_prefix("sha256:")
            .unwrap_or(expected_checksum_str);
        let calculated_checksum = calculate_sha256(filepath)?;
        if expected_hash == calculated_checksum {
            return Ok(true);
        }
    }

    Ok(false)
}

fn update_sha256_checksums(
    filepath: &Path,
    checksums_file: &Path,
    new_checksum: &str,
) -> anyhow::Result<()> {
    let checksums: Value = if checksums_file.exists() {
        let file = File::open(checksums_file)?;
        let reader = BufReader::new(file);
        serde_json::from_reader(reader)?
    } else {
        serde_json::json!({})
    };

    let mut checksums_map = checksums.as_object().unwrap().clone();

    // Add or update the checksum
    let filename = filepath.file_name().unwrap().to_str().unwrap().to_string();
    let prefixed_checksum = format!("sha256:{new_checksum}");
    checksums_map.insert(filename, Value::String(prefixed_checksum));

    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(checksums_file)?;
    let writer = BufWriter::new(file);
    serde_json::to_writer_pretty(writer, &serde_json::Value::Object(checksums_map))?;

    Ok(())
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{BufWriter, Write},
        net::SocketAddr,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use axum::{Router, http::StatusCode, routing::get, serve};
    use rstest::*;
    use serde_json::{json, to_writer};
    use tempfile::TempDir;
    use tokio::{
        net::TcpListener,
        task,
        time::{Duration, sleep},
    };

    use super::*;

    /// Creates a fast, deterministic retry config for tests.
    /// Uses very short delays to make tests run quickly without introducing flakiness.
    fn test_retry_config() -> RetryConfig {
        RetryConfig {
            max_retries: 5,
            initial_delay_ms: 10,
            max_delay_ms: 50,
            backoff_factor: 2.0,
            jitter_ms: 5,
            operation_timeout_ms: Some(500),
            immediate_first: false,
            max_elapsed_ms: Some(2000),
        }
    }

    async fn setup_test_server(
        server_content: Option<String>,
        status_code: StatusCode,
    ) -> SocketAddr {
        let server_content = Arc::new(server_content);
        let server_content_clone = server_content.clone();
        let app = Router::new().route(
            "/testfile.txt",
            get(move || {
                let server_content = server_content_clone.clone();
                async move {
                    let response_body = match &*server_content {
                        Some(content) => content.clone(),
                        None => "File not found".to_string(),
                    };
                    (status_code, response_body)
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = serve(listener, app);

        task::spawn(async move {
            if let Err(e) = server.await {
                eprintln!("server error: {e}");
            }
        });

        sleep(Duration::from_millis(100)).await;

        addr
    }

    #[tokio::test]
    async fn test_file_already_exists() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("testfile.txt");
        fs::write(&file_path, "Existing file content").unwrap();

        let url = "http://example.com/testfile.txt".to_string();
        let result = ensure_file_exists_or_download_http(&file_path, &url, None, Some(5));

        assert!(result.is_ok());
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "Existing file content");
    }

    #[tokio::test]
    async fn test_download_file_success() {
        let temp_dir = TempDir::new().unwrap();
        let filepath = temp_dir.path().join("testfile.txt");
        let filepath_clone = filepath.clone();

        let server_content = "Server file content".to_string();
        let status_code = StatusCode::OK;
        let addr = setup_test_server(Some(server_content.clone()), status_code).await;
        let url = format!("http://{addr}/testfile.txt");

        let result = tokio::task::spawn_blocking(move || {
            ensure_file_exists_or_download_http_with_config(
                &filepath_clone,
                &url,
                None,
                5,
                Some(test_retry_config()),
                Some(0),
            )
        })
        .await
        .unwrap();

        assert!(result.is_ok());
        let content = fs::read_to_string(&filepath).unwrap();
        assert_eq!(content, server_content);
    }

    #[tokio::test]
    async fn test_download_file_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("testfile.txt");

        let server_content = None;
        let status_code = StatusCode::NOT_FOUND;
        let addr = setup_test_server(server_content, status_code).await;
        let url = format!("http://{addr}/testfile.txt");

        let result = tokio::task::spawn_blocking(move || {
            ensure_file_exists_or_download_http_with_config(
                &file_path,
                &url,
                None,
                1,
                Some(test_retry_config()),
                Some(0),
            )
        })
        .await
        .unwrap();

        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("Client error: HTTP"),
            "Unexpected error message: {err_msg}"
        );
    }

    #[tokio::test]
    async fn test_network_error() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("testfile.txt");

        // Use an unreachable address to simulate a network error
        let url = "http://127.0.0.1:0/testfile.txt".to_string();

        let result = tokio::task::spawn_blocking(move || {
            ensure_file_exists_or_download_http_with_config(
                &file_path,
                &url,
                None,
                2,
                Some(test_retry_config()),
                Some(0),
            )
        })
        .await
        .unwrap();

        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("error"),
            "Unexpected error message: {err_msg}"
        );
    }

    #[tokio::test]
    async fn test_retry_then_success_on_500() {
        let temp_dir = TempDir::new().unwrap();
        let filepath = temp_dir.path().join("testfile.txt");
        let filepath_clone = filepath.clone();

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let app = Router::new().route(
            "/testfile.txt",
            get(move || {
                let c = counter_clone.clone();
                async move {
                    let n = c.fetch_add(1, Ordering::SeqCst);
                    if n < 2 {
                        (StatusCode::INTERNAL_SERVER_ERROR, "temporary error")
                    } else {
                        (StatusCode::OK, "eventual success")
                    }
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = serve(listener, app);
        task::spawn(async move {
            let _ = server.await;
        });
        sleep(Duration::from_millis(100)).await;

        let url = format!("http://{addr}/testfile.txt");
        let result = tokio::task::spawn_blocking(move || {
            ensure_file_exists_or_download_http_with_config(
                &filepath_clone,
                &url,
                None,
                5,
                Some(test_retry_config()),
                Some(0),
            )
        })
        .await
        .unwrap();

        assert!(result.is_ok());
        let content = std::fs::read_to_string(&filepath).unwrap();
        assert_eq!(content, "eventual success");
        assert!(counter.load(Ordering::SeqCst) >= 2);
    }

    #[tokio::test]
    async fn test_retry_then_success_on_429() {
        let temp_dir = TempDir::new().unwrap();
        let filepath = temp_dir.path().join("testfile.txt");
        let filepath_clone = filepath.clone();

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let app = Router::new().route(
            "/testfile.txt",
            get(move || {
                let c = counter_clone.clone();
                async move {
                    let n = c.fetch_add(1, Ordering::SeqCst);
                    if n < 1 {
                        (StatusCode::TOO_MANY_REQUESTS, "rate limited")
                    } else {
                        (StatusCode::OK, "ok after retry")
                    }
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = serve(listener, app);
        task::spawn(async move {
            let _ = server.await;
        });
        sleep(Duration::from_millis(100)).await;

        let url = format!("http://{addr}/testfile.txt");
        let result = tokio::task::spawn_blocking(move || {
            ensure_file_exists_or_download_http_with_config(
                &filepath_clone,
                &url,
                None,
                5,
                Some(test_retry_config()),
                Some(0),
            )
        })
        .await
        .unwrap();

        assert!(result.is_ok());
        let content = std::fs::read_to_string(&filepath).unwrap();
        assert_eq!(content, "ok after retry");
        assert!(counter.load(Ordering::SeqCst) >= 2);
    }

    #[tokio::test]
    async fn test_no_retry_on_404() {
        let temp_dir = TempDir::new().unwrap();
        let filepath = temp_dir.path().join("testfile.txt");
        let filepath_clone = filepath.clone();

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let app = Router::new().route(
            "/testfile.txt",
            get(move || {
                let c = counter_clone.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    (StatusCode::NOT_FOUND, "missing")
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = serve(listener, app);
        task::spawn(async move {
            let _ = server.await;
        });
        sleep(Duration::from_millis(100)).await;

        let url = format!("http://{addr}/testfile.txt");
        let result = tokio::task::spawn_blocking(move || {
            ensure_file_exists_or_download_http_with_config(
                &filepath_clone,
                &url,
                None,
                5,
                Some(test_retry_config()),
                Some(0),
            )
        })
        .await
        .unwrap();

        assert!(result.is_err());
        assert_eq!(counter.load(Ordering::SeqCst), 1, "should not retry on 404");
    }

    #[rstest]
    fn test_calculate_sha256() -> anyhow::Result<()> {
        let temp_dir = TempDir::new()?;
        let test_file_path = temp_dir.path().join("test_file.txt");
        let mut test_file = File::create(&test_file_path)?;
        let content = b"Hello, world!";
        test_file.write_all(content)?;

        let expected_hash = "315f5bdb76d078c43b8ac0064e4a0164612b1fce77c869345bfc94c75894edd3";
        let calculated_hash = calculate_sha256(&test_file_path)?;

        assert_eq!(calculated_hash, expected_hash);
        Ok(())
    }

    #[rstest]
    fn test_verify_sha256_checksum() -> anyhow::Result<()> {
        let temp_dir = TempDir::new()?;
        let test_file_path = temp_dir.path().join("test_file.txt");
        let mut test_file = File::create(&test_file_path)?;
        let content = b"Hello, world!";
        test_file.write_all(content)?;

        let calculated_checksum = calculate_sha256(&test_file_path)?;

        // Create checksums.json containing the checksum
        let checksums_path = temp_dir.path().join("checksums.json");
        let checksums_data = json!({
            "test_file.txt": format!("sha256:{}", calculated_checksum)
        });
        let checksums_file = File::create(&checksums_path)?;
        let writer = BufWriter::new(checksums_file);
        to_writer(writer, &checksums_data)?;

        let is_valid = verify_sha256_checksum(&test_file_path, &checksums_path)?;
        assert!(is_valid, "The checksum should be valid");
        Ok(())
    }
}
