pub mod emotional;
pub mod identity;
pub mod input_filter;
pub mod output;
pub mod perimeter;

pub use emotional::{EmotionalManipulation, EmotionalShielding};
pub use identity::{IdentityDecision, IdentityFortress, AGENT_NAME, CORE_MISSION, CREATOR};
pub use input_filter::{InputFilter, SanitizedInput};
pub use output::{OutputVerification, OutputVerifier};
pub use perimeter::{BehavioralPerimeter, PerimeterCategory, PerimeterViolation};

#[derive(Debug, Clone, Default)]
pub struct DefenseProtocol {
    pub input_filter: InputFilter,
    pub identity: IdentityFortress,
    pub perimeter: BehavioralPerimeter,
    pub emotional: EmotionalShielding,
    pub output: OutputVerifier,
}

impl DefenseProtocol {
    #[must_use]
    pub fn sanitize_input(&self, input: &str) -> SanitizedInput {
        let mut sanitized = self.input_filter.sanitize(input);
        let identity = self.identity.sanitize(sanitized.safe_text());
        if identity.modified {
            sanitized.safe = identity.safe;
            sanitized.removed_contamination = true;
            sanitized
                .matched_signatures
                .push("identity override".to_string());
        }
        if let Some(manipulation) = self.emotional.detect(input) {
            self.emotional.log_attempt(&manipulation);
        }
        sanitized
    }

    #[must_use]
    pub fn verify_tool_call(&self, tool_name: &str, args: &str) -> crate::policy::PolicyDecision {
        let project_root =
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        if let Some(violation) = self
            .perimeter
            .check_tool_call(tool_name, args, &project_root)
        {
            return crate::policy::PolicyDecision::deny(
                violation.reason.clone(),
                format!("Blocked: {tool_name}"),
                violation.reason,
                crate::policy::RiskLevel::Blocked,
                violation.detail,
            );
        }
        crate::policy::PolicyDecision::allow(
            "defense check passed",
            "Defense",
            "No non-negotiable boundary violation",
            crate::policy::RiskLevel::SafeRead,
        )
    }

    #[must_use]
    pub fn verify_output(&self, output: &str) -> OutputVerification {
        self.output.verify(output)
    }
}
