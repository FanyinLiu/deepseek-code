use serde::{Deserialize, Serialize};

/// Structured plan output from Plan Mode.
/// The agent generates this via JSON Output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub summary: String,
    pub target_files: Vec<TargetFile>,
    pub steps: Vec<String>,
    pub risks: Vec<Risk>,
    pub verification: Vec<String>,
    pub requires_write: bool,
    pub requires_command: bool,
    pub recommended_model: Option<String>,
    pub thinking: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetFile {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Risk {
    pub level: RiskLevel,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

impl std::fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Low => write!(f, "LOW"),
            Self::Medium => write!(f, "MEDIUM"),
            Self::High => write!(f, "HIGH"),
            Self::Critical => write!(f, "CRITICAL"),
        }
    }
}

impl Plan {
    /// Validate that a plan is reasonable before presenting to user.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if self.summary.trim().is_empty() {
            errors.push("plan summary is empty".into());
        }
        if self.steps.is_empty() {
            errors.push("plan has no execution steps".into());
        }
        if self.target_files.is_empty() {
            errors.push("plan has no target files".into());
        }
        for (i, step) in self.steps.iter().enumerate() {
            if step.trim().is_empty() {
                errors.push(format!("step {} is empty", i + 1));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Format plan for display to user.
    #[must_use]
    pub fn display(&self) -> String {
        let mut out = String::new();

        out.push_str(&format!("# Plan: {}\n\n", self.summary));

        out.push_str("## Target Files\n\n");
        for f in &self.target_files {
            out.push_str(&format!("- `{}` — {}\n", f.path, f.reason));
        }

        out.push_str("\n## Steps\n\n");
        for (i, step) in self.steps.iter().enumerate() {
            out.push_str(&format!("{}. {}\n", i + 1, step));
        }

        if !self.risks.is_empty() {
            out.push_str("\n## Risks\n\n");
            for risk in &self.risks {
                out.push_str(&format!("- **[{}]** {}\n", risk.level, risk.description));
            }
        }

        if !self.verification.is_empty() {
            out.push_str("\n## Verification\n\n");
            for v in &self.verification {
                out.push_str(&format!("- {v}\n"));
            }
        }

        out.push_str("\n## Execution\n\n");
        out.push_str(&format!("- Write: {}\n", self.requires_write));
        out.push_str(&format!("- Command: {}\n", self.requires_command));
        if let Some(ref model) = self.recommended_model {
            out.push_str(&format!("- Recommended model: {model}\n"));
        }
        if let Some(thinking) = self.thinking {
            out.push_str(&format!("- Thinking: {thinking}\n"));
        }

        out
    }
}
