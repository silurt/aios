//! Normalizer tests, against event shapes captured from a real `claude -p
//! --output-format stream-json` run rather than invented ones.

use aios_caps::ports::Harness;
use aios_claude::Claude;
use aios_types::{MessageRole, RunEvent};

fn ev(line: &str) -> Vec<RunEvent> {
    Claude::new().translate(line)
}

#[test]
fn init_becomes_started_with_session_and_tools() {
    let events = ev(r#"{"type":"system","subtype":"init","session_id":"abc-123",
        "model":"claude-opus-5","cwd":"/tmp","tools":["Bash","Edit"],
        "permissionMode":"manual","uuid":"u"}"#);
    assert_eq!(
        events,
        vec![RunEvent::Started {
            session_ref: Some("abc-123".into()),
            model: Some("claude-opus-5".into()),
            tools: vec!["Bash".into(), "Edit".into()],
        }]
    );
}

#[test]
fn hook_chatter_is_dropped() {
    // A real run emitted a dozen of these before doing any work. Transcripts
    // should be about the task, not the plumbing.
    for line in [
        r#"{"type":"system","subtype":"hook_started","hook_name":"bd","session_id":"s","uuid":"u"}"#,
        r#"{"type":"system","subtype":"hook_response","hook_name":"bd","exit_code":0,"session_id":"s","uuid":"u"}"#,
        r#"{"type":"system","subtype":"hook_progress","hook_name":"bd","session_id":"s","uuid":"u"}"#,
    ] {
        assert!(ev(line).is_empty(), "should have been dropped: {line}");
    }
}

#[test]
fn api_retry_becomes_a_notice_rather_than_vanishing() {
    let events = ev(
        r#"{"type":"system","subtype":"api_retry","attempt":2,"max_retries":5,
        "error":"overloaded","session_id":"s","uuid":"u"}"#,
    );
    match &events[..] {
        [RunEvent::Notice { detail }] => {
            assert!(
                detail.contains("2") && detail.contains("overloaded"),
                "{detail}"
            );
        }
        other => panic!("expected one Notice, got {other:?}"),
    }
}

#[test]
fn one_assistant_message_can_yield_several_events() {
    // Text plus two tool calls is three normalized events, which is why
    // translate returns a Vec.
    let events = ev(
        r#"{"type":"assistant","session_id":"s","uuid":"u","message":{"content":[
        {"type":"text","text":"Looking now."},
        {"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls -la"}},
        {"type":"tool_use","id":"t2","name":"Read","input":{"file_path":"/etc/hosts"}}
    ]}}"#,
    );
    assert_eq!(events.len(), 3);
    assert_eq!(
        events[0],
        RunEvent::Message {
            role: MessageRole::Assistant,
            text: "Looking now.".into()
        }
    );
    assert_eq!(
        events[1],
        RunEvent::ToolUse {
            id: Some("t1".into()),
            name: "Bash".into(),
            summary: "ls -la".into()
        }
    );
}

#[test]
fn thinking_is_kept_separate_from_prose() {
    let events = ev(
        r#"{"type":"assistant","session_id":"s","uuid":"u","message":{"content":[
        {"type":"thinking","thinking":"weighing options"}]}}"#,
    );
    assert_eq!(
        events,
        vec![RunEvent::Thinking {
            text: "weighing options".into()
        }]
    );
}

#[test]
fn empty_text_blocks_produce_nothing() {
    let events = ev(
        r#"{"type":"assistant","session_id":"s","uuid":"u","message":{"content":[
        {"type":"text","text":"   \n  "}]}}"#,
    );
    assert!(events.is_empty());
}

#[test]
fn tool_results_carry_their_outcome() {
    let events = ev(
        r#"{"type":"user","session_id":"s","uuid":"u","message":{"content":[
        {"type":"tool_result","tool_use_id":"t1","is_error":true,"content":"no such file"}]}}"#,
    );
    assert_eq!(
        events,
        vec![RunEvent::ToolResult {
            id: Some("t1".into()),
            ok: false,
            summary: "no such file".into()
        }]
    );
}

#[test]
fn result_closes_the_run_with_cost_and_turns() {
    let events = ev(r#"{"type":"result","subtype":"success","is_error":false,
        "result":"done","total_cost_usd":0.0123,"num_turns":3,"duration_ms":4200,
        "permission_denials":[],"session_id":"s","uuid":"u"}"#);
    match &events[..] {
        [
            RunEvent::Finished {
                ok,
                summary,
                cost_usd,
                turns,
                duration_ms,
            },
        ] => {
            assert!(ok);
            assert_eq!(summary.as_deref(), Some("done"));
            assert_eq!(*cost_usd, Some(0.0123));
            assert_eq!(*turns, Some(3));
            assert_eq!(*duration_ms, Some(4200));
        }
        other => panic!("expected Finished, got {other:?}"),
    }
}

#[test]
fn permission_denials_are_surfaced_in_the_summary() {
    // A run that "succeeded" while blocked from everything it tried is not a
    // success anyone wants reported silently.
    let events = ev(
        r#"{"type":"result","subtype":"success","is_error":false,"result":"tried",
        "permission_denials":[{"tool_name":"Bash"},{"tool_name":"Edit"}],
        "session_id":"s","uuid":"u"}"#,
    );
    match &events[..] {
        [RunEvent::Finished { summary, .. }] => {
            let s = summary.as_deref().unwrap_or_default();
            assert!(s.contains("2 permission denial"), "{s}");
        }
        other => panic!("expected Finished, got {other:?}"),
    }
}

#[test]
fn an_error_result_is_not_ok() {
    let events = ev(
        r#"{"type":"result","subtype":"error_max_turns","is_error":true,
        "session_id":"s","uuid":"u"}"#,
    );
    assert!(matches!(events[..], [RunEvent::Finished { ok: false, .. }]));
}

#[test]
fn garbage_lines_are_skipped_not_fatal() {
    // Harnesses print things that are not protocol; a normalizer that panicked
    // on one would take the whole run down.
    for line in ["", "not json", "{", "null", "[]"] {
        assert!(ev(line).is_empty(), "should have been skipped: {line:?}");
    }
}

#[test]
fn long_tool_input_is_truncated_for_the_transcript() {
    // An Edit carries whole file contents; keeping that per event would make
    // transcripts enormous. The full input goes on the approval instead.
    let long = "x".repeat(5_000);
    let line = format!(
        r#"{{"type":"assistant","session_id":"s","uuid":"u","message":{{"content":[
            {{"type":"tool_use","id":"t","name":"Bash","input":{{"command":"{long}"}}}}]}}}}"#
    );
    match &ev(&line)[..] {
        [RunEvent::ToolUse { summary, .. }] => {
            assert!(
                summary.chars().count() <= 161,
                "len {}",
                summary.chars().count()
            );
            assert!(summary.ends_with('…'));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn command_arguments_include_the_flags_the_cli_actually_needs() {
    use aios_caps::ports::RunSpec;
    let spec = RunSpec {
        prompt: "do the thing".into(),
        cwd: "/tmp".into(),
        model: Some("claude-opus-5".into()),
        allowed_tools: vec!["Read".into()],
        disallowed_tools: vec!["Bash".into()],
    };
    let args = Claude::new().command(&spec);

    // --verbose is not optional: stream-json will not emit the full event
    // stream without it, which would silently produce empty transcripts.
    assert!(args.contains(&"--verbose".to_string()));
    assert!(args.contains(&"stream-json".to_string()));
    assert!(args.contains(&"do the thing".to_string()));
    assert!(args.contains(&"--allowed-tools".to_string()));

    let resumed = Claude::new().resume_command(&spec, "sess-1").unwrap();
    assert!(resumed.windows(2).any(|w| w == ["--resume", "sess-1"]));
}
