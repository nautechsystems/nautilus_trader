// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
    fs::File,
    io::{
        copy, {self},
    },
    path::Path,
};

use reqwest::blocking::Client;

pub fn ensure_file_exists_or_download_http(path: &str, url: &str) -> anyhow::Result<()> {
    let file_path = Path::new(path);
    if file_path.exists() {
        return Ok(());
    }

    println!("File not found at {path}. Downloading from {url}");

    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let response = Client::new().get(url).send()?;

    if response.status().is_success() {
        let mut out = File::create(file_path)?;
        let mut content = io::Cursor::new(response.bytes()?);
        copy(&mut content, &mut out)?;
        println!("File downloaded to {path}");
    } else {
        anyhow::bail!("Failed to download file: HTTP {}", response.status());
    }

    Ok(())
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, net::SocketAddr, sync::Arc};

    use axum::{http::StatusCode, routing::get, serve, Router};
    use tempfile::TempDir;
    use tokio::{
        net::TcpListener,
        task,
        time::{sleep, Duration},
    };

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
        let result = ensure_file_exists_or_download_http(file_path.to_str().unwrap(), &url);

        assert!(result.is_ok());
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "Existing file content");
    }

    #[tokio::test]
    async fn test_download_file_success() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("testfile.txt");

        let server_content = Some("Server file content".to_string());
        let status_code = StatusCode::OK;
        let addr = setup_test_server(server_content.clone(), status_code).await;
        let url = format!("http://{}/testfile.txt", addr);

        let file_path_str = file_path.to_str().unwrap().to_string();
        let result = tokio::task::spawn_blocking(move || {
            ensure_file_exists_or_download_http(&file_path_str, &url)
        })
        .await
        .unwrap();

        assert!(result.is_ok());
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, server_content.unwrap());
    }

    #[tokio::test]
    async fn test_download_file_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("testfile.txt");

        let server_content = None;
        let status_code = StatusCode::NOT_FOUND;
        let addr = setup_test_server(server_content, status_code).await;
        let url = format!("http://{}/testfile.txt", addr);

        let result = tokio::task::spawn_blocking(move || {
            ensure_file_exists_or_download_http(file_path.to_str().unwrap(), &url)
        })
        .await
        .unwrap();

        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("Failed to download file"),
            "Unexpected error message: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_network_error() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("testfile.txt");

        // Use an unreachable address to simulate a network error
        let url = "http://127.0.0.1:0/testfile.txt".to_string();

        let result = tokio::task::spawn_blocking(move || {
            ensure_file_exists_or_download_http(file_path.to_str().unwrap(), &url)
        })
        .await
        .unwrap();

        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("error"),
            "Unexpected error message: {}",
            err_msg
        );
    }
}
