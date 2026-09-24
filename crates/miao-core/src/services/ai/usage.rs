//! Bounded passive inspection. Bytes delivered to clients are never changed.
use crate::{
    analytics::{now_ms, Analytics},
    models::ai::AiRequestRecord,
};
use serde_json::Value;
use std::{sync::Arc, time::Instant};

const MAX_INSPECTION: usize = 1024 * 1024;

#[derive(Default)]
struct Scanner {
    sse: bool,
    buffer: Vec<u8>,
    event: Vec<u8>,
    disabled: bool,
    terminal: bool,
    error: bool,
    usage: Option<(i64, i64, i64)>,
}

impl Scanner {
    fn feed(&mut self, bytes: &[u8]) {
        if self.disabled {
            return;
        }
        if !self.sse {
            if self.buffer.len() + bytes.len() > MAX_INSPECTION {
                self.disable();
            } else {
                self.buffer.extend_from_slice(bytes);
            }
            return;
        }
        for &byte in bytes {
            if byte == b'\n' {
                let mut line = std::mem::take(&mut self.buffer);
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                if line.is_empty() {
                    let event = std::mem::take(&mut self.event);
                    self.parse(&event);
                } else if let Some(data) = line.strip_prefix(b"data:") {
                    let data = data.strip_prefix(b" ").unwrap_or(data);
                    if self.event.len() + data.len() + 1 > MAX_INSPECTION {
                        self.disable();
                        return;
                    }
                    self.event.extend_from_slice(data);
                    self.event.push(b'\n');
                }
            } else {
                if self.buffer.len() + self.event.len() >= MAX_INSPECTION {
                    self.disable();
                    return;
                }
                self.buffer.push(byte);
            }
        }
    }

    fn disable(&mut self) {
        self.disabled = true;
        self.buffer.clear();
        self.event.clear();
        self.usage = None;
    }

    fn parse(&mut self, bytes: &[u8]) {
        if bytes.trim_ascii() == b"[DONE]" {
            self.terminal = true;
            return;
        }
        let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
            return;
        };
        let kind = value["type"].as_str().unwrap_or("");
        if value.get("error").is_some_and(|v| !v.is_null())
            || matches!(kind, "error" | "response.failed" | "response.incomplete")
        {
            self.error = true;
        }
        if kind == "response.completed" {
            self.terminal = true;
        }
        let root = value.get("response").unwrap_or(&value);
        if root.get("error").is_some_and(|v| !v.is_null())
            || matches!(root["status"].as_str(), Some("failed" | "incomplete"))
        {
            self.error = true;
        }
        let Some(usage) = root.get("usage") else {
            return;
        };
        let input = usage
            .get("prompt_tokens")
            .or_else(|| usage.get("input_tokens"))
            .and_then(Value::as_i64);
        let output = usage
            .get("completion_tokens")
            .or_else(|| usage.get("output_tokens"))
            .and_then(Value::as_i64);
        if let (Some(input), Some(output), Some(total)) =
            (input, output, usage["total_tokens"].as_i64())
        {
            if input >= 0 && output >= 0 && total >= 0 {
                self.usage = Some((input, output, total));
            }
        }
    }

    fn eof(&mut self) {
        if !self.sse && !self.disabled {
            let body = std::mem::take(&mut self.buffer);
            self.parse(&body);
        }
    }
}

pub(super) struct Observation {
    store: Option<Arc<Analytics>>,
    pub row: AiRequestRecord,
    started: Instant,
    scanner: Scanner,
    finished: bool,
}

impl Observation {
    pub fn new(store: Option<Arc<Analytics>>, mut row: AiRequestRecord) -> Self {
        row.timestamp = now_ms();
        row.outcome = "cancelled".into();
        Self {
            store,
            row,
            started: Instant::now(),
            scanner: Scanner::default(),
            finished: false,
        }
    }

    pub fn response(&mut self, status: u16, sse: bool) {
        self.row.http_status = Some(status.into());
        self.scanner.sse = sse;
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        if !bytes.is_empty() && self.row.first_byte_ms.is_none() {
            self.row.first_byte_ms = Some(self.started.elapsed().as_millis() as i64);
        }
        self.scanner.feed(bytes);
    }

    pub fn eof(&mut self) {
        self.scanner.eof();
        let outcome = if self
            .row
            .http_status
            .is_some_and(|s| !(200..300).contains(&s))
        {
            "http_error"
        } else if self.scanner.error {
            "api_error"
        } else if self.scanner.sse && !self.scanner.terminal && !self.scanner.disabled {
            "stream_interrupted"
        } else {
            "success"
        };
        self.row.usage_complete =
            outcome == "success" && self.scanner.usage.is_some() && !self.scanner.disabled;
        self.finish(outcome);
    }

    pub fn finish(&mut self, outcome: &str) {
        if self.finished {
            return;
        }
        self.finished = true;
        self.row.outcome = outcome.into();
        self.row.duration_ms = self.started.elapsed().as_millis() as i64;
        if let Some((input, output, total)) = self.scanner.usage {
            self.row.input_tokens = Some(input);
            self.row.output_tokens = Some(output);
            self.row.total_tokens = Some(total);
        }
        if let Some(store) = &self.store {
            store.record_ai(self.row.clone());
        }
    }
}

impl Drop for Observation {
    fn drop(&mut self) {
        self.finish("cancelled");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fragmented_multiline_sse_usage_and_terminal_are_required() {
        let bytes=b"data: {\r\ndata: \"usage\": {\"prompt_tokens\":17,\"completion_tokens\":3,\"total_tokens\":20}}\r\n\r\ndata: [DONE]\n\n";
        for size in 1..bytes.len() {
            let mut obs = Observation::new(None, AiRequestRecord::default());
            obs.response(200, true);
            for chunk in bytes.chunks(size) {
                obs.feed(chunk);
            }
            obs.eof();
            assert_eq!(obs.row.total_tokens, Some(20));
            assert!(obs.row.usage_complete);
            assert_eq!(obs.row.outcome, "success");
        }
        let mut obs = Observation::new(None, AiRequestRecord::default());
        obs.response(200, true);
        obs.feed(b"data: {\"usage\":{\"prompt_tokens\":17,\"completion_tokens\":3,\"total_tokens\":20}}\n\n");
        obs.eof();
        assert_eq!(obs.row.outcome, "stream_interrupted");
        assert_eq!(obs.row.input_tokens, Some(17));
        assert!(!obs.row.usage_complete);
    }

    #[test]
    fn json_responses_errors_missing_and_oversized_usage() {
        for (body, expected, complete) in [
            (
                r#"{"usage":{"input_tokens":11,"output_tokens":2,"total_tokens":13}}"#,
                "success",
                true,
            ),
            (r#"{"choices":[]}"#, "success", false),
            (r#"{"error":{"message":"secret"}}"#, "api_error", false),
            (
                r#"{"usage":{"prompt_tokens":-1,"completion_tokens":2,"total_tokens":1}}"#,
                "success",
                false,
            ),
        ] {
            let mut obs = Observation::new(None, AiRequestRecord::default());
            obs.response(200, false);
            obs.feed(body.as_bytes());
            obs.eof();
            assert_eq!(obs.row.outcome, expected);
            assert_eq!(obs.row.usage_complete, complete);
        }
        let mut scanner = Scanner {
            sse: true,
            ..Default::default()
        };
        scanner.feed(&vec![b'x'; MAX_INSPECTION + 1]);
        assert!(scanner.disabled);
        assert!(scanner.buffer.is_empty());
        let mut scanner = Scanner {
            sse: true,
            ..Default::default()
        };
        scanner.feed(b"data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":7,\"output_tokens\":2,\"total_tokens\":9}}}\n\n");
        assert!(scanner.terminal);
        assert_eq!(scanner.usage, Some((7, 2, 9)));
    }
}
