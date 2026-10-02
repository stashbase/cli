use std::time::Duration;

use reqwest::header::CONTENT_TYPE;

use super::event::Event;
use crate::api::client::{get_api_url, CLI_USER_AGENT};

/// Hard cap on the whole request, connect included. Short on purpose: this
/// runs on every tracked exit, and slow connections simply drop the event.
pub const TIMEOUT: Duration = Duration::from_millis(500);

pub fn endpoint() -> String {
    format!("{}/v1/telemetry", get_api_url())
}

/// Prints the exact allowlisted event to stderr instead of sending it.
pub fn print_debug(event: &Event) {
    if let Ok(json) = serde_json::to_string(event) {
        eprintln!("telemetry (debug, not sent): {json}");
    }
}

/// Sends one event and returns. Never panics, never returns an error, and
/// never takes longer than roughly `timeout`.
///
/// The request runs on its own thread with its own runtime because callers
/// may already be inside the `#[tokio::main]` runtime, where starting a
/// second one on the same thread would panic.
pub fn post(endpoint: &str, event: &Event, timeout: Duration) {
    let Ok(body) = serde_json::to_vec(event) else {
        return;
    };
    let endpoint = endpoint.to_owned();
    let handle = std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return;
        };
        runtime.block_on(async {
            let Ok(client) = reqwest::Client::builder()
                .user_agent(CLI_USER_AGENT)
                .timeout(timeout)
                .build()
            else {
                return;
            };
            let _ = tokio::time::timeout(
                timeout,
                client
                    .post(endpoint)
                    .header(CONTENT_TYPE, "application/json")
                    .body(body)
                    .send(),
            )
            .await;
        });
        // A resolver call stuck in a blocking thread must not delay exit.
        runtime.shutdown_timeout(Duration::from_millis(0));
    });
    let _ = handle.join();
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::{Duration, Instant},
    };

    use super::*;
    use crate::telemetry::event::Event;

    /// Accepts one connection, reads a full HTTP request, answers 204 and
    /// returns the raw request text.
    fn serve_once(listener: TcpListener) -> thread::JoinHandle<String> {
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = vec![0u8; 16 * 1024];
            let mut total = 0;
            loop {
                let n = stream.read(&mut buf[total..]).unwrap();
                if n == 0 {
                    break;
                }
                total += n;
                let text = String::from_utf8_lossy(&buf[..total]).to_ascii_lowercase();
                if let Some(head_end) = text.find("\r\n\r\n") {
                    let length = text
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if total >= head_end + 4 + length {
                        break;
                    }
                }
            }
            stream
                .write_all(b"HTTP/1.1 204 No Content\r\ncontent-length: 0\r\n\r\n")
                .unwrap();
            String::from_utf8_lossy(&buf[..total]).to_string()
        })
    }

    #[test]
    fn posts_the_event_as_json() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = serve_once(listener);

        post(
            &format!("http://{addr}/v1/telemetry"),
            &Event::sample(),
            Duration::from_secs(3),
        );

        let request = server.join().unwrap();
        assert!(request.starts_with("POST /v1/telemetry"), "{request}");
        assert!(request.to_ascii_lowercase().contains("application/json"));
        assert!(request.contains("\"event\":\"cli_command\""));
    }

    #[test]
    fn a_hanging_server_costs_at_most_the_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let _server = thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
            thread::sleep(Duration::from_secs(5));
        });

        let started = Instant::now();
        post(
            &format!("http://{addr}/v1/telemetry"),
            &Event::sample(),
            Duration::from_millis(300),
        );
        assert!(started.elapsed() < Duration::from_millis(1500), "{:?}", started.elapsed());
    }

    #[test]
    fn a_refused_connection_is_swallowed() {
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        }; // listener dropped: nothing is listening on this port any more
        let started = Instant::now();
        post(
            &format!("http://127.0.0.1:{port}/v1/telemetry"),
            &Event::sample(),
            Duration::from_secs(1),
        );
        assert!(started.elapsed() < Duration::from_millis(1500));
    }

    #[test]
    fn default_timeout_is_half_a_second() {
        assert_eq!(TIMEOUT, Duration::from_millis(500));
    }

    #[test]
    fn endpoint_uses_the_api_url() {
        assert!(endpoint().ends_with("/v1/telemetry"));
    }
}
