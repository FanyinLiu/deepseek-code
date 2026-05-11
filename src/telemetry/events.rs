/// Telemetry event types. All telemetry is off by default.
/// Users must explicitly opt in on first launch.

#[derive(Debug, Clone, serde::Serialize)]
pub struct TelemetryEvent {
    pub event_type: EventType,
    pub version: String,
    pub os: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    CommandInvoked,
    ApiCall,
    ToolExecuted,
    Panic,
    SessionCreated,
    SessionResumed,
    ComplexityAssessed,
    RouteOverridden,
    PlanGenerated,
    PlanApproved,
    ExecutionStarted,
    ExecutionFinished,
    ResultReverted,
}

/// Check if telemetry should be sent.
/// Respects the config setting and first-launch opt-in.
#[must_use]
pub fn should_send_telemetry(config: &crate::storage::Config) -> bool {
    config.telemetry.enabled
}

/// Build a telemetry event with common fields.
/// Never includes prompt content, file content, or code diffs.
#[must_use]
pub fn build_event(event_type: EventType, data: serde_json::Value) -> TelemetryEvent {
    TelemetryEvent {
        event_type,
        version: env!("CARGO_PKG_VERSION").to_string(),
        os: std::env::consts::OS.to_string(),
        timestamp: chrono::Utc::now(),
        data,
    }
}

/// Log a complexity assessment event to the local telemetry log.
/// This is a no-op if telemetry is not enabled in the config.
pub fn log_complexity_assessed(
    assessment: &crate::agent::router::ComplexityAssessment,
    session_id: &crate::deepseek::SessionId,
) {
    let config = match crate::storage::Config::load(None) {
        Ok(c) => c,
        Err(_) => return,
    };
    if !config.telemetry.enabled {
        return;
    }

    let data = serde_json::json!({
        "session_id": session_id.to_string(),
        "route": assessment.route,
        "score": assessment.score,
        "confidence": assessment.confidence,
        "reason_codes": assessment.reason_codes,
        "model_name": assessment.model_name,
        "latency_ms": assessment.latency_ms,
        "version": assessment.classifier_version,
    });

    let event = build_event(EventType::ComplexityAssessed, data);
    if let Ok(json) = serde_json::to_string(&event) {
        tracing::info!(target: "telemetry", "{}", json);
    }
}
