use std::{
    io::{self, Read, Write},
    process::{Command, Stdio},
    time::Duration,
};

use reqwest::header::CONTENT_TYPE;

use super::event::Event;
use crate::api::client::{get_api_url, CLI_USER_AGENT};

/// When set to `1`, this binary acts as the short-lived sender instead of
/// running a command (see `is_worker`).
pub const WORKER_ENV: &str = "STASHBASE_TELEMETRY_WORKER";

/// Generous on purpose: nothing waits for the worker, so a slow connection
/// costs the user nothing.
pub const WORKER_TIMEOUT: Duration = Duration::from_secs(5);

/// Events are a few hundred bytes; anything larger is not ours.
const MAX_PAYLOAD: u64 = 16 * 1024;

/// The path events are posted to on the Stashbase API host. The agent proxy
/// matches the same path to refuse these requests from inside an agent
/// session, so it is defined once, here.
pub const ENDPOINT_PATH: &str = "/v1/telemetry";

/// Setting this sends telemetry to that server instead of the API URL. It is
/// the explicit opt-in to send anywhere other than Stashbase's own service
/// (for example a local backend or staging while developing).
pub const TELEMETRY_URL_ENV: &str = "STASHBASE_TELEMETRY_URL";

/// The server events go to: the telemetry URL if given, else the API URL.
pub fn destination_url_from(telemetry_url: Option<&str>, api_url: &str) -> String {
    telemetry_url
        .map(|url| url.trim().trim_end_matches('/'))
        .filter(|url| !url.is_empty())
        .unwrap_or(api_url)
        .to_owned()
}

pub fn destination_url() -> String {
    destination_url_from(
        std::env::var(TELEMETRY_URL_ENV).ok().as_deref(),
        &get_api_url(),
    )
}

/// The host of a destination URL, lower-cased and without a trailing dot.
pub fn destination_host_of(url: &str) -> Option<String> {
    reqwest::Url::parse(url)
        .ok()?
        .host_str()
        .map(|host| host.trim_end_matches('.').to_ascii_lowercase())
}

/// The host events are sent to. The agent proxy refuses telemetry POSTs to it.
pub fn destination_host() -> Option<String> {
    destination_host_of(&destination_url())
}

pub fn endpoint() -> String {
    format!("{}{}", destination_url(), ENDPOINT_PATH)
}

/// Whether a request is the CLI's telemetry POST: `POST /v1/telemetry` on the
/// Stashbase API host. The agent proxy uses this to refuse such requests from
/// inside an agent session regardless of the profile's egress policy, which
/// holds even if the process removed `STASHBASE_SANDBOX` from its own
/// environment. Hosts are compared ignoring case and a trailing dot.
pub fn is_telemetry_request(
    api_host: Option<&str>,
    host: Option<&str>,
    method: &str,
    path: &str,
) -> bool {
    let (Some(api_host), Some(host)) = (api_host, host) else {
        return false;
    };
    method.eq_ignore_ascii_case("POST")
        && path == ENDPOINT_PATH
        && api_host
            .trim_end_matches('.')
            .eq_ignore_ascii_case(host.trim_end_matches('.'))
}

/// Prints the exact allowlisted event to stderr instead of sending it.
pub fn print_debug(event: &Event) {
    if let Ok(json) = serde_json::to_string(event) {
        eprintln!("telemetry (debug, not sent): {json}");
    }
}

pub fn is_worker() -> bool {
    std::env::var_os(WORKER_ENV).is_some_and(|value| value == "1")
}

/// Hands the finished event to a detached copy of this binary and returns
/// immediately, so the command never waits on the network. The caller has
/// already decided that sending is allowed. Every failure is ignored and the
/// event is simply dropped.
pub fn dispatch(event: &Event) {
    let Ok(payload) = serde_json::to_vec(event) else {
        return;
    };
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut command = Command::new(exe);
    command.env(WORKER_ENV, "1");
    let _ = spawn_with_payload(command, &payload);
}

/// Starts `command` detached from this process and terminal, writes
/// `payload` to its stdin and returns without waiting for it. Stdout and
/// stderr go to the null device so the child never keeps a pipe open (for
/// example in `stashbase pull | cat`).
fn spawn_with_payload(mut command: Command, payload: &[u8]) -> io::Result<()> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    detach(&mut command);
    let mut child = command.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        // Far below the pipe buffer size, so this cannot block. Dropping
        // stdin closes the pipe, which is the child's end-of-input.
        let _ = stdin.write_all(payload);
    }
    // Deliberately not waited on.
    Ok(())
}

#[cfg(unix)]
fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    // Own process group, so Ctrl-C in the terminal does not kill the sender.
    command.process_group(0);
}

#[cfg(windows)]
fn detach(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW);
}

pub fn run_worker_from_stdin() {
    run_worker(io::stdin(), &endpoint(), WORKER_TIMEOUT);
}

/// The sender side: reads one event from `input` and posts it. Input that is
/// empty, too large or not a JSON object is dropped without any request.
pub fn run_worker(input: impl Read, endpoint: &str, timeout: Duration) {
    let mut body = Vec::new();
    if input.take(MAX_PAYLOAD + 1).read_to_end(&mut body).is_err() {
        return;
    }
    if body.is_empty() || body.len() as u64 > MAX_PAYLOAD {
        return;
    }
    if !serde_json::from_slice::<serde_json::Value>(&body).is_ok_and(|value| value.is_object()) {
        return;
    }
    post_json(endpoint, body, timeout);
}

/// Posts `body` and returns. Never panics, never returns an error, and never
/// takes longer than roughly `timeout`.
///
/// The request runs on its own thread with its own runtime because callers
/// may already be inside the `#[tokio::main]` runtime, where starting a
/// second one on the same thread would panic.
pub fn post_json(endpoint: &str, body: Vec<u8>, timeout: Duration) {
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

    fn body(event: &Event) -> Vec<u8> {
        serde_json::to_vec(event).unwrap()
    }

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

        post_json(
            &format!("http://{addr}/v1/telemetry"),
            body(&Event::sample()),
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
        post_json(
            &format!("http://{addr}/v1/telemetry"),
            body(&Event::sample()),
            Duration::from_millis(300),
        );
        assert!(
            started.elapsed() < Duration::from_millis(1500),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_refused_connection_is_swallowed() {
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        }; // listener dropped: nothing is listening on this port any more
        let started = Instant::now();
        post_json(
            &format!("http://127.0.0.1:{port}/v1/telemetry"),
            body(&Event::sample()),
            Duration::from_secs(1),
        );
        assert!(started.elapsed() < Duration::from_millis(1500));
    }

    #[test]
    fn endpoint_uses_the_api_url() {
        assert!(endpoint().ends_with("/v1/telemetry"));
    }

    #[test]
    fn worker_forwards_its_input_to_the_endpoint() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = serve_once(listener);

        run_worker(
            std::io::Cursor::new(body(&Event::sample())),
            &format!("http://{addr}/v1/telemetry"),
            Duration::from_secs(3),
        );

        let request = server.join().unwrap();
        assert!(request.starts_with("POST /v1/telemetry"), "{request}");
        assert!(request.contains("\"event\":\"cli_command\""));
    }

    #[test]
    fn worker_sends_nothing_for_empty_or_oversized_input() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/v1/telemetry", listener.local_addr().unwrap());

        run_worker(
            std::io::Cursor::new(Vec::new()),
            &url,
            Duration::from_secs(1),
        );
        run_worker(
            std::io::Cursor::new(vec![b'x'; (MAX_PAYLOAD as usize) + 1]),
            &url,
            Duration::from_secs(1),
        );

        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock,
            "no connection should have been made"
        );
    }

    #[cfg(unix)]
    #[test]
    fn spawning_returns_immediately_and_the_payload_arrives_later() {
        let out = std::env::temp_dir().join(format!("stashbase-spawn-{}", uuid::Uuid::new_v4()));
        let mut command = std::process::Command::new("sh");
        command
            .arg("-c")
            .arg("sleep 1; cat > \"$OUT\"")
            .env("OUT", &out);

        let started = Instant::now();
        spawn_with_payload(command, b"hello").unwrap();
        assert!(
            started.elapsed() < Duration::from_millis(400),
            "the caller must not wait for the child: {:?}",
            started.elapsed()
        );

        let deadline = Instant::now() + Duration::from_secs(5);
        while !out.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(50));
        }
        thread::sleep(Duration::from_millis(100));
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "hello");
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn a_missing_binary_is_an_error_not_a_panic() {
        let command = std::process::Command::new("/definitely/not/a/stashbase/binary");
        assert!(spawn_with_payload(command, b"x").is_err());
    }

    #[test]
    fn recognises_the_telemetry_post_on_the_api_host() {
        let api = Some("api.stashbase.dev");
        assert!(is_telemetry_request(
            api,
            Some("api.stashbase.dev"),
            "POST",
            "/v1/telemetry"
        ));
        // Host case and a trailing dot are the same host; the method is case-insensitive.
        assert!(is_telemetry_request(
            api,
            Some("API.Stashbase.Dev."),
            "post",
            "/v1/telemetry"
        ));
    }

    #[test]
    fn leaves_every_other_request_alone() {
        let api = Some("api.stashbase.dev");
        let host = Some("api.stashbase.dev");
        assert!(!is_telemetry_request(api, host, "GET", "/v1/telemetry"));
        assert!(!is_telemetry_request(api, host, "POST", "/v1/secrets"));
        assert!(!is_telemetry_request(
            api,
            host,
            "POST",
            "/v1/telemetry/other"
        ));
        assert!(!is_telemetry_request(api, host, "POST", "/v1/telemetryx"));
        assert!(!is_telemetry_request(
            api,
            Some("example.com"),
            "POST",
            "/v1/telemetry"
        ));
        assert!(!is_telemetry_request(api, None, "POST", "/v1/telemetry"));
        assert!(!is_telemetry_request(None, host, "POST", "/v1/telemetry"));
    }

    #[test]
    fn the_endpoint_path_is_the_one_the_proxy_matches() {
        assert!(endpoint().ends_with(ENDPOINT_PATH));
    }

    #[test]
    fn the_destination_is_the_api_url_unless_a_telemetry_url_is_given() {
        let api = "https://api.stashbase.dev";
        assert_eq!(destination_url_from(None, api), api);
        assert_eq!(destination_url_from(Some("  "), api), api);
        assert_eq!(
            destination_url_from(Some("http://127.0.0.1:9/"), api),
            "http://127.0.0.1:9"
        );
    }

    #[test]
    fn the_destination_host_ignores_scheme_port_and_case() {
        assert_eq!(
            destination_host_of("HTTPS://API.Stashbase.Dev:8443/"),
            Some("api.stashbase.dev".to_owned())
        );
        assert_eq!(destination_host_of("not a url"), None);
    }
}
