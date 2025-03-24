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
    fs::{File, OpenOptions},
    io::{BufReader, BufWriter, Read, copy},
    path::Path,
};

use reqwest::blocking::Client;
use ring::digest;
use serde_json::Value;

/// Ensures that a file exists at the specified path by downloading it if necessary.
///
/// If the file already exists, it checks the integrity of the file using a SHA-256 checksum
/// from the optional `checksums` file. If the checksum is valid, the function exits early. If
/// the checksum is invalid or missing, the function updates the checksums file with the correct
/// hash for the existing file without redownloading it.
///
/// If the file does not exist, it downloads the file from the specified `url` and updates the
/// checksums file (if provided) with the calculated SHA-256 checksum of the downloaded file.
pub fn ensure_file_exists_or_download_http(
    filepath: &Path,
    url: &str,
    checksums: Option<&Path>,
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

    download_file(filepath, url)?;

    if let Some(checksums_file) = checksums {
        let new_checksum = calculate_sha256(filepath)?;
        update_sha256_checksums(filepath, checksums_file, &new_checksum)?;
    }

    Ok(())
}

fn download_file(filepath: &Path, url: &str) -> anyhow::Result<()> {
    println!("Downloading file from {url} to {filepath:?}");

    if let Some(parent) = filepath.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut response = Client::new().get(url).send()?;
    if !response.status().is_success() {
        anyhow::bail!("Failed to download file: HTTP {}", response.status());
    }

    let mut out = File::create(filepath)?;
    copy(&mut response, &mut out)?;

    println!("File downloaded to {filepath:?}");
    Ok(())
}

fn calculate_sha256(filepath: &Path) -> anyhow::Result<String> {
    let mut file = File::open(filepath)?;
    let mut context = digest::Context::new(&digest::SHA256);
    let mut buffer = [0; 4096];

    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        context.update(&buffer[..count]);
    }

    let digest = context.finish();
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
        sync::Arc,
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
        let result = ensure_file_exists_or_download_http(&file_path, &url, None);

        assert!(result.is_ok());
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "Existing file content");
    }

    #[tokio::test]
    async fn test_download_file_success() {
        let temp_dir = TempDir::new().unwrap();
        let filepath = temp_dir.path().join("testfile.txt");
        let filepath_clone = filepath.clone();

        let server_content = Some("Server file content".to_string());
        let status_code = StatusCode::OK;
        let addr = setup_test_server(server_content.clone(), status_code).await;
        let url = format!("http://{addr}/testfile.txt");

        let result = tokio::task::spawn_blocking(move || {
            ensure_file_exists_or_download_http(&filepath_clone, &url, None)
        })
        .await
        .unwrap();

        assert!(result.is_ok());
        let content = fs::read_to_string(&filepath).unwrap();
        assert_eq!(content, server_content.unwrap());
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
            ensure_file_exists_or_download_http(&file_path, &url, None)
        })
        .await
        .unwrap();

        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("Failed to download file"),
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
            ensure_file_exists_or_download_http(&file_path, &url, None)
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
