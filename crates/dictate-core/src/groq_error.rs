use crate::cancellation::{CancellationContext, Cancelled};
use crate::error::TranscriptionError;

const ERROR_BODY_TRUNCATION_CHARS: usize = 200;

fn truncated_body(body: &str) -> String {
    let mut truncated: String = body.chars().take(ERROR_BODY_TRUNCATION_CHARS).collect();
    if body.chars().count() > ERROR_BODY_TRUNCATION_CHARS {
        truncated.push_str("...");
    }
    truncated
}

fn extract_error_message(body: &str, error_label: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(body) {
        Ok(json) => json
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(|message| message.as_str())
            .map_or_else(
                || {
                    eprintln!(
                        "[dictate] warning: {error_label} JSON missing `error.message`, using truncated body"
                    );
                    truncated_body(body)
                },
                std::string::ToString::to_string,
            ),
        Err(err) => {
            eprintln!(
                "[dictate] warning: failed to parse {error_label} JSON, using truncated body: {err}"
            );
            truncated_body(body)
        }
    }
}

pub async fn api_error_from_failed_response(
    response: reqwest::Response,
    error_label: &str,
    cancellation: &CancellationContext,
) -> Result<TranscriptionError, Cancelled> {
    let status_code = response.status().as_u16();
    let body = cancellation
        .run_until_cancelled(response.text())
        .await?
        .unwrap_or_else(|err| {
            eprintln!("[dictate] warning: failed to read {error_label} response body: {err}");
            String::from("<failed to read body>")
        });

    let message = extract_error_message(&body, error_label);

    Ok(TranscriptionError::Api {
        status: status_code,
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn extract_error_message_reads_nested_error_message() {
        let body = r#"{"error":{"message":"invalid api key"}}"#;
        assert_eq!(extract_error_message(body, "Groq error"), "invalid api key");
    }

    #[test]
    fn extract_error_message_truncates_on_invalid_json() {
        let body = &format!("{{\"error\":{}", "x".repeat(260));
        let message = extract_error_message(body, "Groq error");

        assert_eq!(message.chars().count(), ERROR_BODY_TRUNCATION_CHARS + 3);
        assert!(message.ends_with("..."));
    }

    #[test]
    fn extract_error_message_truncates_when_schema_is_unexpected() {
        let body = &format!("{{\"message\":\"{}\"}}", "x".repeat(240));
        let message = extract_error_message(body, "Groq error");

        assert_eq!(message.chars().count(), ERROR_BODY_TRUNCATION_CHARS + 3);
        assert!(message.ends_with("..."));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn api_error_from_failed_response_returns_cancelled_while_body_stalls() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test listener should bind");
        let address = listener
            .local_addr()
            .expect("test listener should expose local address");

        let server = tokio::spawn(async move {
            let (mut stream, _) = listener
                .accept()
                .await
                .expect("test server should accept one connection");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).await;

            stream
                .write_all(
                    b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 1024\r\nConnection: close\r\n\r\n",
                )
                .await
                .expect("test server should write headers");

            tokio::time::sleep(Duration::from_secs(5)).await;
        });

        let response = reqwest::Client::new()
            .get(format!("http://{address}"))
            .send()
            .await
            .expect("request should receive response headers");
        assert_eq!(response.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);

        let cancellation = CancellationContext::new();
        let cancellation_for_task = cancellation.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            cancellation_for_task.cancel();
        });

        let started = Instant::now();
        let result = api_error_from_failed_response(response, "Groq error", &cancellation).await;
        let elapsed = started.elapsed();

        assert!(matches!(result, Err(Cancelled)));
        assert!(elapsed < Duration::from_secs(1));

        server.abort();
    }
}
