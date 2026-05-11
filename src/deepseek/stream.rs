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
    stream
        .map(|item| match item {
            Ok(bytes) => {
                // Use lossy conversion so that a multi-byte UTF-8 character
                // split across chunk boundaries does not kill the entire stream.
                let text = String::from_utf8_lossy(&bytes).to_string();
                let mut events = Vec::new();

                for line in text.lines() {
                    let line = line.trim();

                    if line.is_empty() || line.starts_with(':') {
                        continue;
                    }

                    if let Some(data) = line.strip_prefix("data: ") {
                        if data == "[DONE]" {
                            events.push(Ok(StreamEvent::Done));
                        } else {
                            match serde_json::from_str::<StreamChunk>(data) {
                                Ok(chunk) => events.push(Ok(StreamEvent::Chunk(chunk))),
                                Err(e) => {
                                    events.push(Err(DeepSeekError::Parse(e)));
                                }
                            }
                        }
                    }
                }

                events
            }
            Err(e) => vec![Err(DeepSeekError::Network(e))],
        })
        .flat_map(futures::stream::iter)
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
