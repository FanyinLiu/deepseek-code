use super::schema::Plan;

/// Review a plan for common issues before presenting to the user.
#[must_use]
pub fn review_plan(plan: &Plan) -> PlanReview {
    let mut warnings = Vec::new();
    let mut suggestions = Vec::new();

    // Check for empty steps
    if plan.steps.is_empty() {
        warnings.push("Plan has no execution steps".into());
    }

    // Check for missing verification
    if plan.verification.is_empty() && plan.requires_write {
        warnings.push("Plan involves file writes but has no verification steps".into());
        suggestions.push("Add at least one verification step (e.g., 'run tests')".into());
    }

    // Check for risky operations without risk documentation
    if plan.requires_command && plan.risks.is_empty() {
        warnings.push("Plan involves command execution but documents no risks".into());
        suggestions.push("Add risk entries for commands that will be executed".into());
    }

    // Check target file count
    if plan.target_files.len() > 10 {
        warnings.push(format!(
            "Plan targets {} files — consider splitting into smaller tasks",
            plan.target_files.len()
        ));
    }

    // Check critical risks
    let has_critical = plan
        .risks
        .iter()
        .any(|r| matches!(r.level, super::schema::RiskLevel::Critical));
    if has_critical {
        warnings.push("Plan contains CRITICAL risks — manual review strongly advised".into());
    }

    PlanReview {
        approved: warnings.is_empty(),
        warnings,
        suggestions,
    }
}

#[derive(Debug, Clone)]
pub struct PlanReview {
    pub approved: bool,
    pub warnings: Vec<String>,
    pub suggestions: Vec<String>,
}

impl std::fmt::Display for PlanReview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.approved {
            writeln!(f, "Plan review: PASSED")?;
        } else {
            writeln!(f, "Plan review: NEEDS ATTENTION")?;
            for w in &self.warnings {
                writeln!(f, "  WARNING: {w}")?;
            }
        }
        for s in &self.suggestions {
            writeln!(f, "  SUGGESTION: {s}")?;
        }
        Ok(())
    }
}
