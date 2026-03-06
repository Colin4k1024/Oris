use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use oris_orchestrator::runtime_client::{
    default_handshake_request, A2aSessionCompletion, A2aSessionRequest, HttpRuntimeA2aClient,
    RuntimeA2aClient, EXPECTED_PROTOCOL_VERSION,
};

#[derive(Clone, Debug)]
struct CapturedRequest {
    method: String,
    path: String,
    body: String,
}

fn parse_content_length(headers: &str) -> usize {
    headers
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            if key.trim().eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0)
}

fn spawn_single_response_server(
    status_line: &str,
    response_body: &str,
) -> (
    String,
    Arc<Mutex<Option<CapturedRequest>>>,
    thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let addr = listener.local_addr().expect("mock server local addr");
    let captured = Arc::new(Mutex::new(None));
    let captured_clone = Arc::clone(&captured);
    let status_line = status_line.to_string();
    let response_body = response_body.to_string();

    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept mock request");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("set mock read timeout");

        let mut raw = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            let read = stream.read(&mut chunk).expect("read mock request");
            if read == 0 {
                break;
            }
            raw.extend_from_slice(&chunk[..read]);
            if raw.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }

        let header_end = raw
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|idx| idx + 4)
            .expect("mock header terminator");
        let header_text = String::from_utf8_lossy(&raw[..header_end]).to_string();
        let content_length = parse_content_length(&header_text);
        let mut body = raw[header_end..].to_vec();
        while body.len() < content_length {
            let read = stream.read(&mut chunk).expect("read mock request body");
            if read == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..read]);
        }
        body.truncate(content_length);
        let body_text = String::from_utf8_lossy(&body).to_string();

        let request_line = header_text.lines().next().unwrap_or_default().to_string();
        let mut request_parts = request_line.split_whitespace();
        let method = request_parts.next().unwrap_or_default().to_string();
        let path = request_parts.next().unwrap_or_default().to_string();

        if let Ok(mut slot) = captured_clone.lock() {
            *slot = Some(CapturedRequest {
                method,
                path,
                body: body_text,
            });
        }

        let response = format!(
            "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            status_line,
            response_body.len(),
            response_body
        );
        stream
            .write_all(response.as_bytes())
            .expect("write mock response");
        stream.flush().expect("flush mock response");
    });

    (format!("http://{}", addr), captured, handle)
}

#[test]
fn start_session_rejects_invalid_protocol_version() {
    let req = A2aSessionRequest::start("sender-a", "0.0.1", "task-1", "summary");
    assert!(req.validate().is_err());
}

#[tokio::test]
async fn http_runtime_client_handshake_posts_expected_path_and_unwraps_data() {
    let (base_url, captured, handle) = spawn_single_response_server(
        "200 OK",
        r#"{"data":{"accepted":true,"negotiated_protocol":{"name":"oris.a2a","version":"0.1.0-experimental"},"enabled_capabilities":["Coordination"],"message":"ok","error":null}}"#,
    );
    let client = HttpRuntimeA2aClient::new(base_url);
    let response = client
        .handshake(default_handshake_request("sender-handshake"))
        .await
        .expect("handshake response");
    assert!(response.accepted);

    handle.join().expect("join mock handshake server");
    let captured = captured
        .lock()
        .expect("lock captured handshake")
        .clone()
        .expect("captured handshake request");
    assert_eq!(captured.method, "POST");
    assert_eq!(captured.path, "/v1/evolution/a2a/handshake");
}

#[tokio::test]
async fn http_runtime_client_start_session_posts_expected_path_and_body() {
    let (base_url, captured, handle) = spawn_single_response_server(
        "200 OK",
        r#"{"data":{"session_id":"session-1","task_id":"task-1","state":"Started","summary":"summary","retryable":false,"retry_after_ms":null,"updated_at_ms":123}}"#,
    );
    let client = HttpRuntimeA2aClient::new(base_url);
    let ack = client
        .start_session(A2aSessionRequest::start(
            "sender-a",
            EXPECTED_PROTOCOL_VERSION,
            "task-1",
            "summary",
        ))
        .await
        .expect("start session ack");
    assert_eq!(ack.session_id, "session-1");
    assert_eq!(ack.task_id, "task-1");

    handle.join().expect("join mock start server");
    let captured = captured
        .lock()
        .expect("lock captured start")
        .clone()
        .expect("captured start request");
    assert_eq!(captured.method, "POST");
    assert_eq!(captured.path, "/v1/evolution/a2a/sessions/start");
    let body: serde_json::Value =
        serde_json::from_str(&captured.body).expect("decode start request body");
    assert_eq!(body["sender_id"], "sender-a");
    assert_eq!(body["protocol_version"], EXPECTED_PROTOCOL_VERSION);
    assert_eq!(body["task_id"], "task-1");
}

#[tokio::test]
async fn http_runtime_client_complete_session_posts_session_scoped_path() {
    let (base_url, captured, handle) = spawn_single_response_server(
        "200 OK",
        r#"{"data":{"ack":{"session_id":"session-42","task_id":"task-1","state":"Started","summary":"summary","retryable":false,"retry_after_ms":null,"updated_at_ms":99},"result":{"terminal_state":"Succeeded","summary":"done","retryable":false,"retry_after_ms":null,"failure_code":null,"failure_details":null,"replay_feedback":{"used_capsule":true,"capsule_id":null,"planner_directive":"SkipPlanner","reasoning_steps_avoided":1,"fallback_reason":null,"task_class_id":"issue-automation","task_label":"issue-automation","summary":"done"}}}}"#,
    );
    let client = HttpRuntimeA2aClient::new(base_url);
    let completion = A2aSessionCompletion::succeeded("sender-a", "done", true);
    let response = client
        .complete_session("session-42", completion)
        .await
        .expect("complete session response");
    assert_eq!(response.ack.session_id, "session-42");
    assert_eq!(response.result.summary, "done");

    handle.join().expect("join mock complete server");
    let captured = captured
        .lock()
        .expect("lock captured complete")
        .clone()
        .expect("captured complete request");
    assert_eq!(captured.method, "POST");
    assert_eq!(
        captured.path,
        "/v1/evolution/a2a/sessions/session-42/complete"
    );
}

#[tokio::test]
async fn http_runtime_client_surfaces_non_success_response_details() {
    let (base_url, _captured, handle) =
        spawn_single_response_server("409 Conflict", r#"{"error":"already claimed"}"#);
    let client = HttpRuntimeA2aClient::new(base_url);
    let error = client
        .start_session(A2aSessionRequest::start(
            "sender-a",
            EXPECTED_PROTOCOL_VERSION,
            "task-1",
            "summary",
        ))
        .await
        .expect_err("expected start_session error");
    assert!(error.to_string().contains("runtime api returned 409"));
    assert!(error.to_string().contains("already claimed"));
    handle.join().expect("join mock error server");
}

#[tokio::test]
async fn http_runtime_client_complete_session_rejects_protocol_before_http_call() {
    let client = HttpRuntimeA2aClient::new("http://127.0.0.1:1");
    let mut completion = A2aSessionCompletion::succeeded("sender-a", "done", true);
    completion.protocol_version = "0.0.1".to_string();
    let error = client
        .complete_session("session-1", completion)
        .await
        .expect_err("expected protocol precheck error");
    assert_eq!(
        error.to_string(),
        "incompatible a2a task session protocol version"
    );
}
