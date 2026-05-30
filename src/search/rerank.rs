use crate::deepseek::client::DeepSeekClient;
use crate::deepseek::{
    ChatMessage, ChatMessageContent, ChatRequest, DeepSeekModel, ThinkingConfig,
};

/// Use `DeepSeek` to rerank or summarize search results.
/// This helps the agent prioritize which files to read.
pub async fn deepseek_rerank(
    client: &DeepSeekClient,
    model: &DeepSeekModel,
    query: &str,
    search_context: &super::packer::SearchContext,
) -> Result<String, anyhow::Error> {
    let prompt = format!(
        "A user searched a codebase with query: \"{query}\"\n\n\
         Search results:\n{search_context}\n\n\
         Based on these results, which files are most likely to contain \
         the answer? Rank them by relevance and explain why. \
         If the search missed something, suggest a better search query."
    );

    let request = ChatRequest {
        model: model.to_string(),
        messages: vec![
            ChatMessage {
                role: "system".into(),
                content: Some(ChatMessageContent::Text(
                    "You are a code search assistant. \
                     Analyze search results and help the user find the right files. \
                     Be concise."
                        .into(),
                )),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            ChatMessage {
                role: "user".into(),
                content: Some(ChatMessageContent::Text(prompt)),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
        ],
        tools: None,
        thinking: Some(ThinkingConfig::disabled()),
        response_format: None,
        stream: false,
        max_tokens: Some(2000),
        temperature: Some(0.0),
    };

    let response = client.chat_with_retry(&request).await?;

    let text = response
        .choices
        .first()
        .and_then(|c| c.message.content.as_ref().and_then(|cc| cc.as_text()))
        .unwrap_or("No rerank analysis available");

    Ok(text.to_string())
}
