//! Prueba de integración del flujo completo HTTP -> SSE -> StreamEvent
//! contra un servidor local que imita a la API (sin red ni claves reales).

use super::{
    anthropic::AnthropicStream, openai::OpenAiStream, run_stream, ProviderStream, StreamEvent,
};
use crate::settings::ProviderId;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Sirve una única respuesta HTTP enviando el cuerpo en trozos pequeños.
async fn serve_once(status: &'static str, body: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 16 * 1024];
        let _ = sock.read(&mut buf).await;
        let head = format!(
            "HTTP/1.1 {status}\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            body.len()
        );
        sock.write_all(head.as_bytes()).await.unwrap();
        for chunk in body.as_bytes().chunks(7) {
            sock.write_all(chunk).await.unwrap();
            sock.flush().await.unwrap();
        }
    });
    format!("http://{addr}/v1/messages")
}

async fn collect(
    url: String,
    provider: ProviderId,
    parser: Box<dyn ProviderStream>,
) -> Vec<StreamEvent> {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    let req = reqwest::Client::new().post(url).body("{}");
    run_stream(provider, "modelo-x".into(), req, parser, move |e| {
        sink.lock().unwrap().push(e)
    })
    .await;
    let out = events.lock().unwrap().clone();
    out
}

#[tokio::test]
async fn anthropic_stream_end_to_end() {
    let body = concat!(
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-opus-5-5\"}}\n\n",
        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"¡Guau, \"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ñandú! 🐾\"}}\n\n",
        "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n",
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
    );
    let url = serve_once("200 OK", body).await;
    let evs = collect(url, ProviderId::Anthropic, Box::new(AnthropicStream::new())).await;
    let text: String = evs
        .iter()
        .filter_map(|e| {
            if let StreamEvent::Delta { text } = e {
                Some(text.clone())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(text, "¡Guau, ñandú! 🐾");
    assert!(matches!(evs.last(), Some(StreamEvent::Done { .. })));
}

#[tokio::test]
async fn http_401_becomes_friendly_error() {
    let url = serve_once(
        "401 Unauthorized",
        "{\"error\":{\"type\":\"authentication_error\",\"message\":\"invalid x-api-key\"}}",
    )
    .await;
    let evs = collect(url, ProviderId::Anthropic, Box::new(AnthropicStream::new())).await;
    match evs.as_slice() {
        [StreamEvent::Error { message }] => assert!(message.contains("no es válida")),
        other => panic!("inesperado: {other:?}"),
    }
}

#[tokio::test]
async fn truncated_stream_reports_error() {
    let body = "data: {\"model\":\"gpt-5\",\"choices\":[{\"delta\":{\"content\":\"Ho\"}}]}\n\n";
    let url = serve_once("200 OK", body).await;
    let evs = collect(url, ProviderId::Openai, Box::new(OpenAiStream::new())).await;
    assert!(
        matches!(evs.last(), Some(StreamEvent::Error { message }) if message.contains("antes de completar"))
    );
}

#[tokio::test]
async fn connection_refused_is_reported() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    drop(listener);
    let evs = collect(url, ProviderId::Openai, Box::new(OpenAiStream::new())).await;
    assert!(matches!(evs.as_slice(), [StreamEvent::Error { .. }]));
}
