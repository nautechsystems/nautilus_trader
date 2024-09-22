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
    fs,
    fs::File,
    io::{
        copy, {self},
    },
    path::Path,
};

use reqwest::blocking::Client;

pub fn ensure_file_exists_or_download_http(path: &str, url: &str) -> io::Result<()> {
    let file_path = Path::new(path);
    if Path::new(path).exists() {
        return Ok(());
    }

    println!("File not found at {path}. Downloading from {url}");

    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let response = Client::new().get(url).send().map_err(|e| {
        eprintln!("Network error: {e}");
        io::Error::new(io::ErrorKind::Other, format!("Network error: {e}"))
    })?;

    if response.status().is_success() {
        let mut out = File::create(file_path)?;
        let mut content = io::Cursor::new(response.bytes().map_err(|e| {
            eprintln!("Error reading response bytes: {e}");
            io::Error::new(io::ErrorKind::Other, e)
        })?);
        copy(&mut content, &mut out)?;
        println!("File downloaded to {path}");
    } else {
        eprintln!("Failed to download file: HTTP {}", response.status());
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("Failed to download file: HTTP {}", response.status()),
        ));
    }

    Ok(())
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::{fs, net::SocketAddr, sync::Arc};

    use axum::{http::StatusCode, routing::get, serve, Router};
    use rstest::rstest;
    use tempfile::TempDir;
    use tokio::{
        net::TcpListener,
        task,
        time::{sleep, Duration},
    };

    use super::*;

    async fn setup_test_server(
        server_content: Option<String>,
        status_code: StatusCode,
    ) -> SocketAddr {
        let server_content = Arc::new(server_content);
        let status_code = status_code;
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

    #[rstest]
    #[case::file_exists(true, true, StatusCode::OK, Some("Server file content".into()))]
    #[case::download_success(false, true, StatusCode::OK, Some("Server file content".into()))]
    #[case::download_404(false, true, StatusCode::NOT_FOUND, None)]
    #[case::network_error(false, false, StatusCode::OK, Some("Server file content".into()))]
    #[tokio::test]
    async fn test_ensure_file_exists(
        #[case] file_exists: bool,
        #[case] start_server: bool,
        #[case] status_code: StatusCode,
        #[case] server_content: Option<String>,
    ) {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("testfile.txt");

        if file_exists {
            fs::write(&file_path, "Existing file content").unwrap();
        }

        let url: String;

        if start_server {
            let addr = setup_test_server(server_content.clone(), status_code).await;
            url = format!("http://{}/testfile.txt", addr);
        } else {
            // Use an unreachable address to simulate a network error
            let addr = SocketAddr::from(([127, 0, 0, 1], 0));
            url = format!("http://{}/testfile.txt", addr);
        }

        let file_path_clone = file_path.to_str().unwrap().to_string();
        let url_clone = url.clone();
        let result = tokio::task::spawn_blocking(move || {
            ensure_file_exists_or_download_http(&file_path_clone, &url_clone)
        })
        .await
        .unwrap();

        if !start_server {
            // Expect a network error
            assert!(result.is_err());
            let err = result.unwrap_err();
            let err_msg = format!("{}", err);
            assert!(
                err_msg.contains("Network error"),
                "Unexpected error message: {}",
                err_msg
            );
        } else {
            match status_code {
                StatusCode::OK => {
                    if file_exists {
                        // The function should not attempt to download
                        assert!(result.is_ok());
                        let content = fs::read_to_string(&file_path).unwrap();
                        assert_eq!(content, "Existing file content");
                    } else {
                        // The function should download the file
                        assert!(result.is_ok());
                        let content = fs::read_to_string(&file_path).unwrap();
                        let expected_content =
                            server_content.unwrap_or_else(|| "File not found".to_string());
                        assert_eq!(content, expected_content);
                    }
                }
                StatusCode::NOT_FOUND => {
                    assert!(result.is_err());
                    let err = result.unwrap_err();
                    let err_msg = format!("{}", err);
                    assert!(
                        err_msg.contains("Failed to download file"),
                        "Unexpected error message: {}",
                        err_msg
                    );
                }
                _ => {}
            }
        }
    }
}
