use deepseek_code::deepseek::models::StreamChunk;
/// Tests for `DeepSeek` SSE stream parsing.
use deepseek_code::deepseek::stream::StreamAccumulator;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Content delta accumulation
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[test]
fn test_accumulate_content_deltas() {
    let mut acc = StreamAccumulator::new();

    let chunk1 = serde_json::from_str::<StreamChunk>(
        r#"{"choices":[{"index":0,"delta":{"content":"Hello "},"finish_reason":null}],"usage":null}"#
    ).expect("valid stream chunk");
    let chunk2 = serde_json::from_str::<StreamChunk>(
        r#"{"choices":[{"index":0,"delta":{"content":"world"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#
    ).expect("valid stream chunk");

    acc.apply_chunk(&chunk1).unwrap();
    acc.apply_chunk(&chunk2).unwrap();

    let result = acc.finalize();
    assert_eq!(result.content, "Hello world");
    assert_eq!(result.finish_reason, Some("stop".into()));
    assert!(result.usage.is_some());
    assert_eq!(
        result.usage.as_ref().expect("usage present").total_tokens,
        15
    );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Reasoning content accumulation
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[test]
fn test_accumulate_reasoning_deltas() {
    let mut acc = StreamAccumulator::new();

    let chunk = serde_json::from_str::<StreamChunk>(
        r#"{"choices":[{"index":0,"delta":{"reasoning_content":"Let me think about this..."},"finish_reason":null}],"usage":null}"#
    ).expect("valid stream chunk");

    acc.apply_chunk(&chunk).unwrap();

    let result = acc.finalize();
    assert_eq!(result.reasoning_content, "Let me think about this...");
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tool call delta merging
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[test]
fn test_accumulate_tool_call_deltas() {
    let mut acc = StreamAccumulator::new();

    // First delta: tool call id and function name
    let chunk1 = serde_json::from_str::<StreamChunk>(
        r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_123","function":{"name":"read_file","arguments":""}}]},"finish_reason":null}],"usage":null}"#
    ).expect("valid stream chunk");

    // Second delta: function arguments
    let chunk2 = serde_json::from_str::<StreamChunk>(
        r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"path\":\"src/main.rs\"}"}}]},"finish_reason":"tool_calls"}],"usage":null}"#
    ).expect("valid stream chunk");

    acc.apply_chunk(&chunk1).unwrap();
    acc.apply_chunk(&chunk2).unwrap();

    let result = acc.finalize();
    assert_eq!(result.finish_reason, Some("tool_calls".into()));
    assert_eq!(result.tool_calls.len(), 1);
    assert_eq!(result.tool_calls[0].id, "call_123");
    assert_eq!(result.tool_calls[0].function.name, "read_file");
    assert_eq!(
        result.tool_calls[0].function.arguments,
        r#"{"path":"src/main.rs"}"#
    );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Multiple tool calls in one response
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[test]
fn test_multiple_tool_calls() {
    let mut acc = StreamAccumulator::new();

    let chunk1 = serde_json::from_str::<StreamChunk>(
        r#"{"choices":[{"index":0,"delta":{"tool_calls":[
            {"index":0,"id":"call_a","function":{"name":"read_file","arguments":"{\"path\":\"a.rs\"}"}},
            {"index":1,"id":"call_b","function":{"name":"read_file","arguments":"{\"path\":\"b.rs\"}"}}
        ]},"finish_reason":"tool_calls"}],"usage":null}"#
    ).expect("valid stream chunk");

    acc.apply_chunk(&chunk1).unwrap();

    let result = acc.finalize();
    assert_eq!(result.tool_calls.len(), 2);
    assert_eq!(result.tool_calls[0].id, "call_a");
    assert_eq!(result.tool_calls[1].id, "call_b");
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tool call arguments accumulate across deltas
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[test]
fn test_tool_call_arguments_accumulate() {
    let mut acc = StreamAccumulator::new();

    let chunks: Vec<StreamChunk> = [
        r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"tc_1","function":{"name":"edit_file","arguments":"{\"path\":"}}]},"finish_reason":null}],"usage":null}"#,
        r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"src/main.rs\",\"old\":"}}]},"finish_reason":null}],"usage":null}"#,
        r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"a\",\"new\":\"b\"}"}}]},"finish_reason":"tool_calls"}],"usage":null}"#,
    ]
    .iter()
    .map(|s| serde_json::from_str::<StreamChunk>(s).expect("valid stream chunk json"))
    .collect();

    for chunk in &chunks {
        acc.apply_chunk(chunk).unwrap();
    }

    let result = acc.finalize();
    assert_eq!(result.tool_calls.len(), 1);
    assert_eq!(
        result.tool_calls[0].function.arguments,
        r#"{"path":"src/main.rs","old":"a","new":"b"}"#
    );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Cache usage parsing from usage
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[test]
fn test_cache_usage_from_usage() {
    use deepseek_code::deepseek::CacheUsage;
    use deepseek_code::deepseek::Usage;

    let usage = Usage {
        prompt_tokens: 1000,
        completion_tokens: 200,
        total_tokens: 1200,
        prompt_cache_hit_tokens: Some(800),
        prompt_cache_miss_tokens: Some(200),
    };

    let cache = CacheUsage::from_usage(&usage);
    assert_eq!(cache.prompt_cache_hit_tokens, 800);
    assert_eq!(cache.prompt_cache_miss_tokens, 200);
    assert!((cache.hit_rate() - 0.8).abs() < 0.001);
}

#[test]
fn test_cache_hit_rate_zero_when_no_tokens() {
    use deepseek_code::deepseek::CacheUsage;
    let cache = CacheUsage {
        prompt_cache_hit_tokens: 0,
        prompt_cache_miss_tokens: 0,
    };
    assert_eq!(cache.hit_rate(), 0.0);
}
