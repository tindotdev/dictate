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
) -> TranscriptionError {
    let status_code = response.status().as_u16();
    let body = response.text().await.unwrap_or_else(|err| {
        eprintln!("[dictate] warning: failed to read {error_label} response body: {err}");
        String::from("<failed to read body>")
    });

    let message = extract_error_message(&body, error_label);

    TranscriptionError::Api {
        status: status_code,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
