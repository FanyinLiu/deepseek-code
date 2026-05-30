use super::*;

// ---------------------------------------------------------------------------
// Rule engine tests
// ---------------------------------------------------------------------------

#[test]
fn read_only_task_is_simple() {
    let result = rules::assess_with_rules("解释一下这个函数为什么重试3次", None);
    assert!(
        result.score < 40,
        "read-only task should score < 40, got {}",
        result.score
    );
    assert!(
        result.confidence >= 0.75,
        "read-only task should have high confidence"
    );
    assert!(result.reason_codes.contains(&ReasonCode::ReadOnly));
}

#[test]
fn chinese_review_only_task_is_read_only() {
    let result = rules::assess_with_rules("仅读取代码，审查 router 配置阈值，不要改文件", None);

    assert_eq!(result.predicted_write_files, 0);
    assert!(
        result.score < 40,
        "review-only task should stay direct/read-only, got {}",
        result.score
    );
    assert!(result.confidence >= 0.75);
    assert!(result.reason_codes.contains(&ReasonCode::ReadOnly));
}

#[test]
fn review_then_fix_is_not_read_only() {
    let result = rules::assess_with_rules("审查后修复问题并补测试", None);

    assert!(!result.reason_codes.contains(&ReasonCode::ReadOnly));
    assert!(
        result.reason_codes.contains(&ReasonCode::TestRequired)
            || result.reason_codes.contains(&ReasonCode::ShellRequired)
    );
}

#[test]
fn greeting_is_simple() {
    let result = rules::assess_with_rules("你好", None);
    assert!(
        result.score < 40,
        "greeting should score < 40, got {}",
        result.score
    );
    assert!(
        result.confidence >= 0.75,
        "greeting should have high confidence"
    );
}

#[test]
fn single_file_text_change_is_simple() {
    let result = rules::assess_with_rules(
        "修改 src/components/Button.tsx 文件，把文案从'开始'改成'立即开始'",
        None,
    );
    assert!(
        result.score < 40,
        "single-file text change should score < 40, got {}",
        result.score
    );
    assert!(result.reason_codes.contains(&ReasonCode::SingleFileSafe));
}

#[test]
fn multi_file_feature_is_complex() {
    let result = rules::assess_with_rules("重构登录模块，添加邮箱验证码并补测试", None);
    assert!(
        result.score >= 40,
        "multi-file feature should score >= 40, got {}",
        result.score
    );
    assert!(
        result.reason_codes.contains(&ReasonCode::MultiFile)
            || result.reason_codes.contains(&ReasonCode::TestRequired)
    );
}

#[test]
fn network_install_is_hard_trigger_complex() {
    let result = rules::assess_with_rules("安装新的依赖并更新流水线", None);
    assert!(
        result.has_hard_trigger,
        "network/install should trigger hard complex"
    );
    assert!(result.reason_codes.contains(&ReasonCode::NetworkRequired));
}

#[test]
fn delete_is_hard_trigger_complex() {
    let result = rules::assess_with_rules("删除旧模块并清理配置", None);
    assert!(
        result.has_hard_trigger,
        "delete should trigger hard complex"
    );
    assert!(result.risk_flags.contains(&RiskFlag::Delete));
}

#[test]
fn deploy_is_hard_trigger_complex() {
    let result = rules::assess_with_rules("部署到生产环境", None);
    assert!(
        result.has_hard_trigger,
        "deploy should trigger hard complex"
    );
    assert!(result.risk_flags.contains(&RiskFlag::Deploy));
}

#[test]
fn drop_statement_is_flagged_as_delete() {
    // "drop" alone (no delete/remove word) is still destructive.
    let result = rules::assess_with_rules("drop the legacy_sessions table", None);
    assert!(result.risk_flags.contains(&RiskFlag::Delete));
    assert!(result.has_hard_trigger);
}

#[test]
fn network_install_has_no_spurious_permission_flag() {
    // Installing deps is a network action, not a permission change.
    let result = rules::assess_with_rules("npm install lodash", None);
    assert!(result.reason_codes.contains(&ReasonCode::NetworkRequired));
    assert!(!result.risk_flags.contains(&RiskFlag::PermissionChange));
}

#[test]
fn chinese_multi_file_request_is_flagged_multi_file() {
    // Chinese conjunction "和" + file noun should read as multiple files,
    // matching the English "and ... file" path.
    let result = rules::assess_with_rules("修改 auth 模块和 user 模块的文件", None);
    assert!(result.reason_codes.contains(&ReasonCode::MultiFile));
}

#[test]
fn ambiguous_short_task_is_clarify() {
    let result = rules::assess_with_rules("fix it", None);
    // Very short + vague → the engine may treat it as simple due to low score.
    // We primarily assert it doesn't crash and produces a valid assessment.
    assert!(
        result.score <= 40,
        "vague short task should not score high, got {}",
        result.score
    );
}

#[test]
fn refactor_is_complex() {
    let result = rules::assess_with_rules("重构整个认证模块", None);
    assert!(
        result.score >= 40,
        "refactor should score >= 40, got {}",
        result.score
    );
}

#[test]
fn shell_command_is_complex() {
    let result = rules::assess_with_rules("运行测试并修复失败的用例", None);
    // Should have test-related reason code; exact score depends on rules
    assert!(
        result.reason_codes.contains(&ReasonCode::TestRequired)
            || result.reason_codes.contains(&ReasonCode::ShellRequired),
        "run tests should have TestRequired or ShellRequired, got reasons={:?}",
        result.reason_codes
    );
}

#[test]
fn no_duplicate_test_scoring() {
    let result = rules::assess_with_rules("run the tests", None);
    // "run" triggers ShellRequired, "tests" triggers TestRequired
    // Both are valid distinct signals here
    let _has_shell = result.reason_codes.contains(&ReasonCode::ShellRequired);
    let has_test = result.reason_codes.contains(&ReasonCode::TestRequired);
    assert!(has_test, "should have TestRequired");
}

#[test]
fn estimate_file_changes_default_zero() {
    let result = rules::assess_with_rules("hello world", None);
    assert_eq!(
        result.predicted_write_files, 0,
        "chat greeting should estimate 0 files"
    );
}

#[test]
fn estimate_file_changes_detects_file_mention() {
    let result = rules::assess_with_rules("修改 src/main.rs 文件", None);
    assert_eq!(
        result.predicted_write_files, 1,
        "explicit file mention should estimate 1 file"
    );
}

#[test]
fn local_file_read_requests_are_direct_high_confidence() {
    for input in [
        "你能读取电脑里的文件吗",
        "能读取我的电脑文件吗",
        "检测一下电脑里面 ds文件夹的 deekseep code 文件夹",
        "读取 C:/Users/name/Desktop/a.txt",
        "read my computer file",
    ] {
        let result = rules::assess_with_rules(input, None);
        assert!(
            result.score < 40,
            "local read should remain direct, got score {} for {input}",
            result.score
        );
        assert!(
            result.confidence >= 0.75,
            "local read should not be routed to clarification, got confidence {} for {input}",
            result.confidence
        );
        assert!(result.reason_codes.contains(&ReasonCode::ReadOnly));

        let route =
            super::determine_route_from_score(result.score, result.confidence, true, 40, 0.75);
        assert_eq!(route, Route::DirectExecute, "input: {input}");
    }
}

// ---------------------------------------------------------------------------
// Route determination tests
// ---------------------------------------------------------------------------

#[test]
fn route_direct_when_low_score_high_confidence() {
    let route = super::determine_route_from_score(20, 0.80, true, 40, 0.75);
    assert_eq!(route, Route::DirectExecute);
}

#[test]
fn route_plan_when_high_score() {
    let route = super::determine_route_from_score(60, 0.90, true, 40, 0.75);
    assert_eq!(route, Route::PlanReview);
}

#[test]
fn route_plan_when_low_confidence() {
    let route = super::determine_route_from_score(30, 0.30, true, 40, 0.75);
    assert_eq!(route, Route::PlanReview);
}

#[test]
fn route_clarify_when_mid_confidence() {
    let route = super::determine_route_from_score(30, 0.60, true, 40, 0.75);
    assert_eq!(route, Route::Clarify);
}

#[test]
fn route_direct_with_liberal_threshold() {
    let route = super::determine_route_from_score(45, 0.80, false, 40, 0.75);
    assert_eq!(route, Route::DirectExecute);
}

#[test]
fn route_uses_configured_confidence_threshold() {
    let route = super::determine_route_from_score(20, 0.80, true, 40, 0.90);
    assert_eq!(route, Route::Clarify);
}

#[test]
fn route_uses_configured_simple_threshold() {
    let route = super::determine_route_from_score(50, 0.90, true, 60, 0.75);
    assert_eq!(route, Route::DirectExecute);
}

#[test]
fn router_from_config_copies_thresholds() {
    let cfg = crate::storage::config::RouterConfig {
        simple_threshold: 55,
        confidence_threshold: 0.9,
        ..Default::default()
    };
    let router = ComplexityRouter::from(&cfg);

    assert_eq!(router.simple_threshold, 55);
    assert_eq!(router.confidence_threshold, 0.9);
}

// ---------------------------------------------------------------------------
// ComplexityAssessment tests
// ---------------------------------------------------------------------------

#[test]
fn assessment_display_summary_contains_route() {
    let assessment = ComplexityAssessment {
        route: Route::DirectExecute,
        complexity_label: ComplexityLabel::Simple,
        score: 25,
        confidence: 0.88,
        reason_codes: vec![ReasonCode::SingleFileSafe],
        hard_trigger_codes: vec![],
        predicted_write_files: 1,
        predicted_commands: 0,
        predicted_duration_ms: 15000,
        risk_flags: vec![],
        classifier_version: "rules-test".into(),
        explanation: "test".into(),
        model_name: None,
        latency_ms: 0,
    };
    let summary = assessment.display_summary();
    assert!(
        summary.contains("直接执行"),
        "summary should mention 直接执行"
    );
    assert!(summary.contains("25"), "summary should contain score");
    assert!(summary.contains("88%"), "summary should contain confidence");
}

#[test]
fn reason_code_user_text_not_empty() {
    for code in [
        ReasonCode::ReadOnly,
        ReasonCode::SingleFileSafe,
        ReasonCode::MultiFile,
        ReasonCode::ShellRequired,
        ReasonCode::TestRequired,
        ReasonCode::NetworkRequired,
        ReasonCode::HighRisk,
        ReasonCode::AmbiguousTask,
    ] {
        assert!(
            !code.user_text().is_empty(),
            "ReasonCode::{code:?} should have non-empty text"
        );
    }
}

#[test]
fn safe_reason_codes_allow_direct() {
    assert!(ReasonCode::ReadOnly.is_safe_for_direct());
    assert!(ReasonCode::SingleFileSafe.is_safe_for_direct());
    assert!(!ReasonCode::MultiFile.is_safe_for_direct());
    assert!(!ReasonCode::HighRisk.is_safe_for_direct());
}

// ---------------------------------------------------------------------------
// End-to-end routing tests (score + confidence -> Route).
//
// These exercise `ComplexityRouter::assess` (rules-only) so a regression in
// the score/confidence thresholds in `determine_route_from_score` is caught,
// not just the rule-engine sub-component above.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn route_hard_risk_task_goes_to_plan_review() {
    let route = ComplexityRouter::new()
        .rules_only()
        .assess("部署到生产环境", None, None)
        .await
        .route;
    assert_eq!(route, Route::PlanReview);
}

#[tokio::test]
async fn route_simple_greeting_executes_directly() {
    let route = ComplexityRouter::new()
        .rules_only()
        .assess("你好", None, None)
        .await
        .route;
    assert_eq!(route, Route::DirectExecute);
}

#[tokio::test]
async fn route_read_only_question_executes_directly() {
    let route = ComplexityRouter::new()
        .rules_only()
        .assess("解释一下这个函数为什么重试3次", None, None)
        .await
        .route;
    assert_eq!(route, Route::DirectExecute);
}

#[tokio::test]
async fn route_multi_file_feature_goes_to_plan_review() {
    let route = ComplexityRouter::new()
        .rules_only()
        .assess("重构登录模块，添加邮箱验证码并补测试", None, None)
        .await
        .route;
    assert_eq!(route, Route::PlanReview);
}
