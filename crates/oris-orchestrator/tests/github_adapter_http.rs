use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

use oris_orchestrator::github_adapter::{
    GitHubAdapter, GitHubApiAdapter, IssueListQuery, PrPayload,
};

fn read_http_request(stream: &mut TcpStream) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0_u8; 1024];
    let mut headers_end = None;
    loop {
        let read = stream.read(&mut chunk).expect("read request");
        if read == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..read]);
        if headers_end.is_none() {
            headers_end = buf.windows(4).position(|window| window == b"\r\n\r\n");
        }
        if headers_end.is_some() {
            break;
        }
    }

    if let Some(header_start) = headers_end {
        let body_start = header_start + 4;
        let header_text = String::from_utf8_lossy(&buf[..body_start]);
        let mut content_length = 0usize;
        for line in header_text.lines() {
            if let Some(value) = line.strip_prefix("Content-Length:") {
                content_length = value.trim().parse::<usize>().unwrap_or(0);
                break;
            }
            if let Some(value) = line.strip_prefix("content-length:") {
                content_length = value.trim().parse::<usize>().unwrap_or(0);
                break;
            }
        }

        while buf.len().saturating_sub(body_start) < content_length {
            let read = stream.read(&mut chunk).expect("read request body");
            if read == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..read]);
        }
    }

    String::from_utf8_lossy(&buf).into_owned()
}

fn write_json_response(stream: &mut TcpStream, status: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        status,
        body.len(),
        body
    );
    stream
        .write_all(response.as_bytes())
        .expect("write response");
    stream.flush().expect("flush response");
}

#[tokio::test(flavor = "current_thread")]
async fn list_issues_falls_back_to_anonymous_when_token_is_rejected() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let addr = listener.local_addr().expect("local addr");
    let seen_auth = Arc::new(Mutex::new(Vec::<bool>::new()));
    let seen_auth_clone = Arc::clone(&seen_auth);

    let server = thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().expect("accept request");
            let request = read_http_request(&mut stream);
            let has_auth = request
                .lines()
                .any(|line| line.to_ascii_lowercase().starts_with("authorization:"));
            seen_auth_clone
                .lock()
                .expect("record auth header")
                .push(has_auth);

            if has_auth {
                write_json_response(
                    &mut stream,
                    "401 Unauthorized",
                    "{\"message\":\"bad credentials\"}",
                );
            } else {
                write_json_response(
                    &mut stream,
                    "200 OK",
                    r#"[{"number":110,"title":"[EVMAP-01]","state":"open","html_url":"https://github.com/Colin4k1024/Oris/issues/110","labels":[{"name":"priority/P0"}],"milestone":{"number":7,"title":"Sprint 1"},"created_at":"2026-03-05T14:42:22Z"}]"#,
                );
            }
        }
    });

    let adapter = GitHubApiAdapter::with_base_url(
        "Colin4k1024",
        "Oris",
        "invalid-token",
        format!("http://{}", addr),
    );
    let query = IssueListQuery {
        state: "open".to_string(),
        per_page: 100,
        max_pages: 1,
    };
    let issues = adapter.list_issues(&query).await.expect("list issues");

    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].number, 110);
    assert_eq!(issues[0].state, "OPEN");

    server.join().expect("server thread");
    let auth_trace = seen_auth.lock().expect("auth trace").clone();
    assert_eq!(auth_trace, vec![true, false]);
}

#[tokio::test(flavor = "current_thread")]
async fn create_pull_request_fails_when_token_is_invalid() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let addr = listener.local_addr().expect("local addr");
    let seen_auth = Arc::new(Mutex::new(Vec::<bool>::new()));
    let seen_auth_clone = Arc::clone(&seen_auth);

    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let request = read_http_request(&mut stream);
        let has_auth = request
            .lines()
            .any(|line| line.to_ascii_lowercase().starts_with("authorization:"));
        seen_auth_clone
            .lock()
            .expect("record auth header")
            .push(has_auth);
        write_json_response(
            &mut stream,
            "401 Unauthorized",
            "{\"message\":\"bad credentials\"}",
        );
    });

    let adapter = GitHubApiAdapter::with_base_url(
        "Colin4k1024",
        "Oris",
        "invalid-token",
        format!("http://{}", addr),
    );
    let payload = PrPayload::new(
        "issue-110",
        "codex/issue-110",
        "main",
        "evidence-110",
        "body",
    );
    let error = adapter
        .create_pull_request(&payload)
        .await
        .expect_err("expected token failure");

    assert!(error.to_string().contains("401"));
    server.join().expect("server thread");
    let auth_trace = seen_auth.lock().expect("auth trace").clone();
    assert_eq!(auth_trace, vec![true]);
}
