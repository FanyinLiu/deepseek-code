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
        queued: VecDeque<Result<StreamEvent, DeepSeekError>>,
        eof: bool,
    }

    fn queue_line(queued: &mut VecDeque<Result<StreamEvent, DeepSeekError>>, line: &[u8]) {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let line = String::from_utf8_lossy(line);
        let line = line.trim();

        if line.is_empty() || line.starts_with(':') {
            return;
        }

        if let Some(data) = line.strip_prefix("data: ") {
            if data == "[DONE]" {
                queued.push_back(Ok(StreamEvent::Done));
            } else {
                match serde_json::from_str::<StreamChunk>(data) {
                    Ok(chunk) => queued.push_back(Ok(StreamEvent::Chunk(chunk))),
                    Err(e) => queued.push_back(Err(DeepSeekError::Parse(e))),
                }
            }
        }
    }

    fn drain_complete_lines(
        buffer: &mut Vec<u8>,
        queued: &mut VecDeque<Result<StreamEvent, DeepSeekError>>,
    ) {
        while let Some(pos) = buffer.iter().position(|byte| *byte == b'\n') {
            let mut line: Vec<u8> = buffer.drain(..=pos).collect();
            if line.ends_with(b"\n") {
                line.pop();
            }
            queue_line(queued, &line);
        }
    }

    futures::stream::unfold(
        ParseState {
            stream: Box::pin(stream),
            buffer: Vec::new(),
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
                        drain_complete_lines(&mut state.buffer, &mut state.queued);
                    }
                    Some(Err(e)) => {
                        return Some((Err(DeepSeekError::Network(e)), state));
                    }
                    None => {
                        if !state.buffer.is_empty() {
                            let line = std::mem::take(&mut state.buffer);
                            queue_line(&mut state.queued, &line);
                        }
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
    pending_tool_calls: Vec<PendingToolCall>,
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
            self.pending_tool_calls.push(PendingToolCall::default());
        }
        let pending = &mut self.pending_tool_calls[idx];
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
        for p in self.pending_tool_calls {
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
}
