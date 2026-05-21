use std::{collections::VecDeque, pin::Pin};

use futures::StreamExt;
use tokio_stream::Stream;

use super::errors::DeepSeekError;
use super::models::{StreamChunk, StreamResult, ToolCall, ToolCallDelta};

/// Parse an SSE byte stream into `StreamEvent` items.
/// Skips keep-alive comments (`: ...`) and empty lines.
/// Handles `data: [DONE]` as a terminal signal.
pub fn parse_stream<S>(stream: S) -> impl Stream<Item = Result<StreamEvent, DeepSeekError>>
where
    S: Stream<Item = reqwest::Result<bytes::Bytes>> + Send + 'static,
{
    struct ParseState<S> {
        stream: Pin<Box<S>>,
        buffer: Vec<u8>,
        event: SseEvent,
        queued: VecDeque<Result<StreamEvent, DeepSeekError>>,
        eof: bool,
    }

    #[derive(Debug, Default)]
    struct SseEvent {
        data_lines: Vec<String>,
    }

    fn dispatch_event(
        event: &mut SseEvent,
        queued: &mut VecDeque<Result<StreamEvent, DeepSeekError>>,
    ) {
        if event.data_lines.is_empty() {
            return;
        }

        let data = event.data_lines.join("\n");
        event.data_lines.clear();
        if data == "[DONE]" {
            queued.push_back(Ok(StreamEvent::Done));
        } else {
            match serde_json::from_str::<StreamChunk>(&data) {
                Ok(chunk) => queued.push_back(Ok(StreamEvent::Chunk(chunk))),
                Err(e) => queued.push_back(Err(DeepSeekError::Parse(e))),
            }
        }
    }

    fn process_line(
        event: &mut SseEvent,
        queued: &mut VecDeque<Result<StreamEvent, DeepSeekError>>,
        line: &[u8],
    ) {
        let line = String::from_utf8_lossy(line);
        if line.is_empty() {
            dispatch_event(event, queued);
            return;
        }
        if line.starts_with(':') {
            return;
        }

        let Some((field, value)) = line.split_once(':') else {
            return;
        };
        if field != "data" {
            return;
        }

        let value = value.strip_prefix(' ').unwrap_or(value);
        event.data_lines.push(value.to_string());
    }

    fn drain_complete_lines(
        buffer: &mut Vec<u8>,
        event: &mut SseEvent,
        queued: &mut VecDeque<Result<StreamEvent, DeepSeekError>>,
        eof: bool,
    ) {
        while let Some(pos) = buffer
            .iter()
            .position(|byte| *byte == b'\n' || *byte == b'\r')
        {
            let delimiter = buffer[pos];
            if delimiter == b'\r' && pos + 1 == buffer.len() && !eof {
                break;
            }
            let line = buffer[..pos].to_vec();
            let drain_to = if delimiter == b'\r' && buffer.get(pos + 1) == Some(&b'\n') {
                pos + 2
            } else {
                pos + 1
            };
            buffer.drain(..drain_to);
            process_line(event, queued, &line);
        }
    }

    futures::stream::unfold(
        ParseState {
            stream: Box::pin(stream),
            buffer: Vec::new(),
            event: SseEvent::default(),
            queued: VecDeque::new(),
            eof: false,
        },
        |mut state| async move {
            loop {
                if let Some(event) = state.queued.pop_front() {
                    return Some((event, state));
                }

                if state.eof {
                    return None;
                }

                match state.stream.next().await {
                    Some(Ok(bytes)) => {
                        state.buffer.extend_from_slice(&bytes);
                        drain_complete_lines(
                            &mut state.buffer,
                            &mut state.event,
                            &mut state.queued,
                            false,
                        );
                    }
                    Some(Err(e)) => {
                        return Some((Err(DeepSeekError::Network(e)), state));
                    }
                    None => {
                        if !state.buffer.is_empty() {
                            let line = std::mem::take(&mut state.buffer);
                            process_line(&mut state.event, &mut state.queued, &line);
                        }
                        dispatch_event(&mut state.event, &mut state.queued);
                        state.eof = true;
                    }
                }
            }
        },
    )
}

#[derive(Debug, Clone)]
pub enum StreamEvent {
    Chunk(StreamChunk),
    Done,
}

/// Accumulates stream deltas into a single `StreamResult`.
#[derive(Debug, Default)]
pub struct StreamAccumulator {
    result: StreamResult,
    pending_tool_calls: Vec<Option<PendingToolCall>>,
}

#[derive(Debug, Default)]
struct PendingToolCall {
    index: u32,
    id: String,
    name: String,
    arguments: String,
}

impl StreamAccumulator {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply_chunk(&mut self, chunk: &StreamChunk) -> Result<(), DeepSeekError> {
        const MAX_CONTENT_LEN: usize = 8 * 1024 * 1024; // 8 MiB

        for choice in &chunk.choices {
            let delta = &choice.delta;

            if let Some(ref content) = delta.content {
                if self.result.content.len() + content.len() > MAX_CONTENT_LEN {
                    return Err(DeepSeekError::Other(
                        "stream content exceeded maximum size (8 MiB)".into(),
                    ));
                }
                self.result.content.push_str(content);
            }
            if let Some(ref reasoning) = delta.reasoning_content {
                if self.result.reasoning_content.len() + reasoning.len() > MAX_CONTENT_LEN {
                    return Err(DeepSeekError::Other(
                        "stream reasoning content exceeded maximum size (8 MiB)".into(),
                    ));
                }
                self.result.reasoning_content.push_str(reasoning);
            }
            if let Some(ref reasoning_details) = delta.reasoning_details {
                let reasoning = reasoning_details_to_text(reasoning_details);
                if !reasoning.is_empty() {
                    if self.result.reasoning_content.len() + reasoning.len() > MAX_CONTENT_LEN {
                        return Err(DeepSeekError::Other(
                            "stream reasoning content exceeded maximum size (8 MiB)".into(),
                        ));
                    }
                    self.result.reasoning_content.push_str(&reasoning);
                }
            }
            if let Some(ref tool_deltas) = delta.tool_calls {
                for td in tool_deltas {
                    self.merge_tool_delta(td)?;
                }
            }
            if let Some(ref reason) = choice.finish_reason {
                self.result.finish_reason = Some(reason.clone());
            }
        }
        if let Some(ref usage) = chunk.usage {
            self.result.usage = Some(usage.clone());
        }
        Ok(())
    }

    fn merge_tool_delta(&mut self, td: &ToolCallDelta) -> Result<(), DeepSeekError> {
        const MAX_CONTENT_LEN: usize = 8 * 1024 * 1024; // 8 MiB

        let idx = td.index as usize;
        while self.pending_tool_calls.len() <= idx {
            self.pending_tool_calls.push(None);
        }
        let pending = self.pending_tool_calls[idx].get_or_insert_with(|| PendingToolCall {
            index: td.index,
            ..PendingToolCall::default()
        });
        pending.index = td.index;
        if let Some(ref id) = td.id {
            pending.id = id.clone();
        }
        if let Some(ref func) = td.function {
            if let Some(ref name) = func.name {
                pending.name = name.clone();
            }
            if let Some(ref args) = func.arguments {
                if pending.arguments.len() + args.len() > MAX_CONTENT_LEN {
                    return Err(DeepSeekError::Other(
                        "stream tool arguments exceeded maximum size (8 MiB)".into(),
                    ));
                }
                pending.arguments.push_str(args);
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn finalize(mut self) -> StreamResult {
        for p in self.pending_tool_calls.into_iter().flatten() {
            if p.id.is_empty() || p.name.is_empty() {
                continue;
            }
            self.result.tool_calls.push(ToolCall {
                id: p.id,
                call_type: "function".into(),
                function: super::models::ToolCallFunction {
                    name: p.name,
                    arguments: p.arguments,
                },
            });
        }
        self.result
    }
}

fn reasoning_details_to_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(items) => items
            .iter()
            .map(reasoning_details_to_text)
            .collect::<Vec<_>>()
            .join(""),
        serde_json::Value::Object(object) => object
            .get("text")
            .or_else(|| object.get("content"))
            .map(reasoning_details_to_text)
            .unwrap_or_default(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use futures::StreamExt;

    use super::{parse_stream, StreamEvent};

    #[tokio::test]
    async fn parse_stream_buffers_split_sse_json_lines() {
        let payload = r#"data: {"id":"1","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"content":"你好"},"finish_reason":null}]}"#;
        let chunks = vec![
            Ok(Bytes::from(payload[..35].to_string())),
            Ok(Bytes::from(payload[35..90].to_string())),
            Ok(Bytes::from(format!("{}\n\n", &payload[90..]))),
            Ok(Bytes::from("data: [DONE]\n\n")),
        ];

        let mut stream = Box::pin(parse_stream(futures::stream::iter(chunks)));
        let first = stream.next().await.expect("first event").expect("chunk");
        match first {
            StreamEvent::Chunk(chunk) => {
                assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("你好"));
            }
            StreamEvent::Done => panic!("expected chunk before done"),
        }
        assert!(matches!(
            stream.next().await.expect("done event").expect("done"),
            StreamEvent::Done
        ));
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn parse_stream_accepts_data_without_space() {
        let payload = r#"data:{"id":"1","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"content":"ok"},"finish_reason":null}]}"#;
        let chunks = vec![
            Ok(Bytes::from(format!("{payload}\n\n"))),
            Ok(Bytes::from("data:[DONE]\n\n")),
        ];

        let mut stream = Box::pin(parse_stream(futures::stream::iter(chunks)));
        let first = stream.next().await.expect("first event").expect("chunk");
        match first {
            StreamEvent::Chunk(chunk) => {
                assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("ok"));
            }
            StreamEvent::Done => panic!("expected chunk before done"),
        }
        assert!(matches!(
            stream.next().await.expect("done event").expect("done"),
            StreamEvent::Done
        ));
    }

    #[tokio::test]
    async fn parse_stream_multiline_data_event() {
        let chunks = vec![
            Ok(Bytes::from(
                "data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\n",
            )),
            Ok(Bytes::from(
                "data: \"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ok\"},\"finish_reason\":null}]}\n\n",
            )),
            Ok(Bytes::from("data: [DONE]\n\n")),
        ];

        let mut stream = Box::pin(parse_stream(futures::stream::iter(chunks)));
        let first = stream.next().await.expect("first event").expect("chunk");
        match first {
            StreamEvent::Chunk(chunk) => {
                assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("ok"));
            }
            StreamEvent::Done => panic!("expected chunk before done"),
        }
        assert!(matches!(
            stream.next().await.expect("done event").expect("done"),
            StreamEvent::Done
        ));
    }

    #[tokio::test]
    async fn parse_stream_cr_only_line_endings() {
        let payload = r#"data: {"id":"1","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"content":"ok"},"finish_reason":null}]}"#;
        let chunks = vec![Ok(Bytes::from(format!("{payload}\r\rdata: [DONE]\r\r")))];

        let mut stream = Box::pin(parse_stream(futures::stream::iter(chunks)));
        assert!(matches!(
            stream.next().await.expect("chunk event").expect("chunk"),
            StreamEvent::Chunk(_)
        ));
        assert!(matches!(
            stream.next().await.expect("done event").expect("done"),
            StreamEvent::Done
        ));
    }
}
