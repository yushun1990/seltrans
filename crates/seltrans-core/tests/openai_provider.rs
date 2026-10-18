//! Provider integration tests against a minimal local HTTP server. These
//! exercise the real request/response path (headers, status mapping,
//! timeout, payload validation) without any network access.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

use seltrans_core::analysis::{ProviderError, TranslationProvider};
use seltrans_core::config::OpenAiCompatibleConfig;
use seltrans_core::openai::OpenAiCompatibleProvider;

/// Per-test unique env var; edition 2024 requires an unsafe block because
/// mutating the process environment is unsound under parallelism. The name
/// is unique to this test binary so it cannot race with anything.
const KEY_ENV: &str = "SELTRANS_M2_TEST_API_KEY";

struct MockServer {
    base_url: String,
    request: std::sync::mpsc::Receiver<String>,
}

/// Serves exactly one connection, then closes. `response` is a complete
/// HTTP/1.1 response; if it is `None` the server stalls past the client
/// timeout to exercise the timeout path.
fn mock_server(response: Option<String>) -> MockServer {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let (sender, request) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let (mut stream, _) = match listener.accept() {
            Ok(connection) => connection,
            Err(_) => return,
        };
        if let Some(request) = read_request(&mut stream) {
            let _ = sender.send(request);
        }
        match response {
            Some(payload) => {
                let _ = stream.write_all(payload.as_bytes());
                let _ = stream.flush();
            }
            None => thread::sleep(Duration::from_secs(3)),
        }
    });
    MockServer { base_url, request }
}

/// Reads headers plus the content-length body so assertions can see the
/// JSON the provider actually sent.
fn read_request(stream: &mut std::net::TcpStream) -> Option<String> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let text = String::from_utf8_lossy(&bytes);
        if let Some(header_end) = text.find("\r\n\r\n") {
            let length = text
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.trim()
                        .eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            if bytes.len() >= header_end + 4 + length {
                return Some(text.into_owned());
            }
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return Some(String::from_utf8_lossy(&bytes).into_owned()),
            Ok(n) => bytes.extend_from_slice(&chunk[..n]),
        }
    }
}

fn http_response(status_line: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

fn completion_body(content: &str) -> String {
    format!(
        "{{\"choices\":[{{\"message\":{{\"content\":{}}}}}]}}",
        serde_json::to_string(content).unwrap()
    )
}

fn provider(base_url: &str, timeout_secs: u64) -> OpenAiCompatibleProvider {
    OpenAiCompatibleProvider::new(OpenAiCompatibleConfig {
        base_url: base_url.to_owned(),
        model: "test-model".to_owned(),
        api_key_env: KEY_ENV.to_owned(),
        timeout_secs,
    })
}

fn set_test_key() {
    // Unique to this test binary; other tests in the process never read it.
    unsafe { std::env::set_var(KEY_ENV, "test-secret") };
}

#[test]
fn translates_passage_and_sends_expected_request() {
    set_test_key();
    let content =
        "{\"schema_version\":1,\"translation\":\"这不是一个性能问题，而是一个正确性问题。\"}";
    let server = mock_server(Some(http_response("200 OK", &completion_body(content))));

    let translation = provider(&server.base_url, 10)
        .translate_passage("This is not a performance problem.")
        .unwrap();

    assert_eq!(translation.text, "这不是一个性能问题，而是一个正确性问题。");
    let request = server.request.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(
        request.starts_with("POST /chat/completions HTTP/1.1"),
        "{request}"
    );
    assert!(request.contains("Authorization: Bearer test-secret"));
    let body_start = request.find("\r\n\r\n").unwrap() + 4;
    let body: serde_json::Value = serde_json::from_str(&request[body_start..]).unwrap();
    assert_eq!(body["model"], "test-model");
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][1]["role"], "user");
    assert_eq!(
        body["messages"][1]["content"],
        "This is not a performance problem."
    );
    assert!(
        body["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("Simplified Chinese")
    );
}

#[test]
fn recovers_translation_from_fenced_model_output() {
    set_test_key();
    let content = "```json\n{\"schema_version\":1,\"translation\":\"好的\"}\n```";
    let server = mock_server(Some(http_response("200 OK", &completion_body(content))));

    let translation = provider(&server.base_url, 10)
        .translate_passage("OK")
        .unwrap();
    assert_eq!(translation.text, "好的");
}

#[test]
fn maps_auth_rate_limit_and_api_errors() {
    set_test_key();

    let unauthorized = mock_server(Some(http_response(
        "401 Unauthorized",
        "{\"error\":{\"message\":\"bad key\"}}",
    )));
    assert_eq!(
        provider(&unauthorized.base_url, 10).translate_passage("hi"),
        Err(ProviderError::Auth { status: 401 })
    );

    let limited = mock_server(Some(http_response("429 Too Many Requests", "{}")));
    assert_eq!(
        provider(&limited.base_url, 10).translate_passage("hi"),
        Err(ProviderError::RateLimited)
    );

    let broken = mock_server(Some(http_response(
        "500 Internal Server Error",
        "{\"error\":{\"message\":\"upstream exploded\"}}",
    )));
    assert_eq!(
        provider(&broken.base_url, 10).translate_passage("hi"),
        Err(ProviderError::Api {
            status: 500,
            message: "upstream exploded".to_owned()
        })
    );
}

#[test]
fn reports_malformed_payloads() {
    set_test_key();
    let cases = [
        String::from("{\"schema_version\":2,\"translation\":\"x\"}"),
        String::from("{\"schema_version\":1}"),
        String::from("{\"schema_version\":1,\"translation\":\"\"}"),
        String::from("totally not json"),
    ];
    for content in cases {
        let server = mock_server(Some(http_response("200 OK", &completion_body(&content))));
        let result = provider(&server.base_url, 10).translate_passage("hi");
        assert!(
            matches!(result, Err(ProviderError::MalformedResponse(_))),
            "expected malformed response for {content}"
        );
    }
}

#[test]
fn times_out_when_the_provider_stalls() {
    set_test_key();
    let server = mock_server(None);
    let result = provider(&server.base_url, 1).translate_passage("hi");
    assert_eq!(result, Err(ProviderError::Timeout { after_secs: 1 }));
}

#[test]
fn requires_the_configured_api_key_before_any_request() {
    let absent = "SELTRANS_M2_KEY_THAT_IS_DEFINITELY_NOT_SET_7F3A";
    // Not set by this binary; if someone exported it the test would still
    // pass the NotConfigured path only when the variable is truly absent,
    // so assert on the variant, not the whole value.
    let config = OpenAiCompatibleConfig {
        base_url: "http://127.0.0.1:9".to_owned(),
        model: "m".to_owned(),
        api_key_env: absent.to_owned(),
        timeout_secs: 2,
    };
    let error = OpenAiCompatibleProvider::new(config)
        .translate_passage("hi")
        .unwrap_err();
    assert!(matches!(error, ProviderError::MissingApiKey { ref env_var } if env_var == absent));
}

#[test]
fn reports_unreachable_endpoints_as_network_errors() {
    set_test_key();
    // Port 9 (discard) on loopback is closed in containers/CI.
    let error = provider("http://127.0.0.1:9", 2)
        .translate_passage("hi")
        .unwrap_err();
    assert!(matches!(error, ProviderError::Network(_)), "{error}");
}
