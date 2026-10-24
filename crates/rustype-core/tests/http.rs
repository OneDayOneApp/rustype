//! End-to-end HTTP tests against a tiny in-process server: verifies the
//! multipart ASR request shape and the chat-completions round trip.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

use rustype_core::asr::AsrClient;
use rustype_core::config::{Llm, Provider};
use rustype_core::llm::LlmClient;
use rustype_core::prompt::Message;

fn read_request(stream: &mut TcpStream) -> String {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    let header_end = loop {
        let n = stream.read(&mut tmp).expect("read");
        if n == 0 {
            return String::from_utf8_lossy(&buf).to_string();
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
    };
    let headers = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (k, v) = line.split_once(':')?;
            if k.eq_ignore_ascii_case("content-length") {
                v.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);
    while buf.len() < header_end + content_length {
        let n = stream.read(&mut tmp).expect("read body");
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    String::from_utf8_lossy(&buf).to_string()
}

fn serve_once(response: &'static str) -> (String, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let request = read_request(&mut stream);
        let reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response.len(),
            response
        );
        stream.write_all(reply.as_bytes()).expect("write");
        request
    });
    (format!("http://{addr}"), handle)
}

fn provider(base: String) -> Provider {
    Provider {
        base_url: base,
        api_key: "test-key".into(),
        asr_model: "whisper-test".into(),
        llm_model: "chat-test".into(),
        language: "en".into(),
        timeout_secs: 10,
        proxy: String::new(),
    }
}

#[test]
fn asr_posts_multipart_and_reads_text() {
    let (base, server) = serve_once(r#"{"text":"  hello from asr  "}"#);
    let client = AsrClient::new(&provider(base)).unwrap();
    let text = client
        .transcribe_wav(b"RIFFfakewav".to_vec(), Some("Rustype"))
        .unwrap();
    assert_eq!(text, "hello from asr");

    let request = server.join().unwrap();
    assert!(
        request.starts_with("POST /audio/transcriptions"),
        "{request}"
    );
    assert!(request.contains("audio.wav"));
    assert!(request.contains("whisper-test"));
    assert!(request.contains("language"));
    assert!(request.contains("Rustype"));
}

#[test]
fn llm_posts_json_and_parses_choice() {
    let (base, server) =
        serve_once(r#"{"choices":[{"message":{"role":"assistant","content":"\"Clean text.\""}}]}"#);
    let client = LlmClient::new(&provider(base), &Llm::default()).unwrap();
    let out = client
        .complete(&[Message::system("sys"), Message::user("raw")])
        .unwrap();
    assert_eq!(out, "Clean text.");

    let request = server.join().unwrap();
    assert!(request.starts_with("POST /chat/completions"), "{request}");
    assert!(request.contains("\"model\":\"chat-test\""));
    assert!(request.contains("\"messages\""));
}
