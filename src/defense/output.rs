#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputVerification {
    pub safe: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct OutputVerifier;

impl OutputVerifier {
    #[must_use]
    pub fn verify(&self, output: &str) -> OutputVerification {
        let lower = output.to_lowercase();
        let mut warnings = Vec::new();
        for marker in [
            "system prompt",
            "developer message",
            "ignore previous instructions",
            "系统提示词",
            "sk-",
            "ghp_",
        ] {
            if lower.contains(marker) {
                warnings.push(format!("output contains protected marker: {marker}"));
            }
        }
        OutputVerification {
            safe: warnings.is_empty(),
            warnings,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_output_passes() {
        let result = OutputVerifier.verify("已完成代码修改并运行测试。");
        assert!(result.safe);
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn protected_marker_is_flagged() {
        let result = OutputVerifier.verify("The system prompt is ...");
        assert!(!result.safe);
        assert_eq!(result.warnings.len(), 1);
    }
}
