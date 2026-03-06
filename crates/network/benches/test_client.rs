use nautilus_network::http::InnerHttpClient;
use reqwest::Method;

const CONCURRENCY: usize = 256;
const TOTAL: usize = 1_000_000;

#[tokio::main]
async fn main() {
    let client = InnerHttpClient::default();
    let mut reqs = Vec::new();
    for _ in 0..(TOTAL / CONCURRENCY) {
        for _ in 0..CONCURRENCY {
            reqs.push(client.send_request(
                Method::GET,
                "http://127.0.0.1:3000".to_string(),
                None,
                None,
                None,
                None,
            ));
        }

        let resp = futures::future::join_all(reqs.drain(0..)).await;
        assert!(resp.iter().all(|res| if let Ok(resp) = res {
            resp.status.is_success()
        } else {
            false
        }));
    }
}
