use std::collections::HashMap;

use hyper::Method;
use nautilus_network::HttpClient;

// Testing with nginx docker container
// `docker run --publish 8080:80 nginx`
#[tokio::main]
async fn main() {
    let client = HttpClient::default();
    let mut success = 0;
    for _ in 0..100_000 {
        if let Ok(resp) = client
            .send_request(
                Method::GET,
                "http://localhost:8080".to_string(),
                HashMap::new(),
            )
            .await
        {
            if resp.status == 200 {
                success += 1;
            }
        }
    }

    println!("successful requests: {}", success);
}
