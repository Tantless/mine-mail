use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use mine_mail::{
    AccountConfig, ComposeFormat, ComposeRequest, MailBackend, ServerConfig, SmtpSecurity,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tempfile::tempdir;

use super::{
    AI_TRANSLATION_SUBJECT_PART_ID, AiContact, AiDraftSnapshot, AiExecutionContext, AiMode,
    AiRuntime, AiTranslationFormat, AiTranslationPartRequest, AiTranslationRequest,
};

const RUN_GUARD: &str = "MINE_MAIL_RUN_AI_CHAIN";
const MAIL_DATA_ROOT_ENV: &str = "MINE_MAIL_AI_TEST_DATA_ROOT";
const MODEL_ENV: &str = "MINE_MAIL_AI_TEST_MODEL";
const BASE_URL_ENV: &str = "MINE_MAIL_AI_TEST_BASE_URL";
const DEFAULT_MODEL: &str = "deepseek-v4-pro";
const DEFAULT_BASE_URL: &str = "https://api.deepseek.com";

#[derive(Serialize)]
struct CaseReport {
    case: &'static str,
    status: &'static str,
    duration_ms: u128,
    changed_fields: Vec<String>,
    decision: Option<String>,
    tool_activity_count: usize,
    output_bytes: usize,
    output_digest: String,
    note: &'static str,
}

#[derive(Serialize)]
struct ChainReport {
    provider: &'static str,
    protocol: &'static str,
    model: String,
    real_mail_digest: Option<String>,
    cases: Vec<CaseReport>,
}

struct Harness {
    runtime: AiRuntime,
    backend: Arc<MailBackend>,
    provider_instance_id: String,
    model: String,
}

#[derive(Clone)]
struct SelectedMail {
    subject: String,
    body: String,
    digest: String,
}

fn require_manual_guard() {
    assert_eq!(
        env::var(RUN_GUARD).ok().as_deref(),
        Some("1"),
        "refusing a billable AI test without the explicit manual guard"
    );
    assert!(
        env::var("DEEPSEEK_API_KEY").is_ok_and(|value| !value.trim().is_empty()),
        "DEEPSEEK_API_KEY is required"
    );
}

fn required_env(name: &str) -> String {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| panic!("{name} is required"))
}

fn digest(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hasher.finalize()[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn mock_backend(database_path: impl AsRef<Path>) -> Arc<MailBackend> {
    let config = AccountConfig::new(
        "ai-chain-mock-account",
        "ai-chain@example.invalid",
        "not-a-real-secret",
        ServerConfig {
            host: "imap.example.invalid".to_owned(),
            port: 993,
        },
        ServerConfig {
            host: "smtp.example.invalid".to_owned(),
            port: 465,
        },
        SmtpSecurity::ImplicitTls,
    )
    .expect("mock account config");
    Arc::new(MailBackend::open(config, database_path).expect("mock mail backend"))
}

fn setup_harness() -> (tempfile::TempDir, Harness) {
    let directory = tempdir().expect("temporary AI chain directory");
    let runtime = AiRuntime::open(directory.path());
    let model = env::var(MODEL_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_owned());
    let base_url = env::var(BASE_URL_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned());
    let provider_instance_id = "01777777-7777-4777-8777-777777777777".to_owned();
    let connection = Connection::open(directory.path().join("desktop-ai.sqlite3"))
        .expect("temporary AI database");
    connection
        .execute(
            "INSERT INTO ai_provider_instances (
                 id, provider_id, name, protocol_id, base_url, model_name,
                 use_environment_key, sort_order, is_default, status,
                 latency_ms, checked_at_ms, legacy_credential_provider_id, updated_at_ms
             ) VALUES (?1, 'deepseek', '手动链路测试', 'openai_chat_completions', ?2, ?3,
                       1, 0, 1, 'untested', NULL, NULL, NULL, 1)",
            (&provider_instance_id, &base_url, &model),
        )
        .expect("temporary DeepSeek provider instance");
    connection
        .execute(
            "INSERT INTO ai_config (
                 singleton, provider_id, protocol_id, base_url, model_name,
                 use_environment_key, translation_language, updated_at_ms
             ) VALUES (1, 'deepseek', 'openai_chat_completions', ?1, ?2, 1, 'zh-Hans', 1)",
            (&base_url, &model),
        )
        .expect("temporary DeepSeek translation config");
    drop(connection);
    let backend = mock_backend(directory.path().join("mock-mail.sqlite3"));
    (
        directory,
        Harness {
            runtime,
            backend,
            provider_instance_id,
            model,
        },
    )
}

fn draft(subject: &str, body: &str) -> AiDraftSnapshot {
    AiDraftSnapshot {
        account_id: "ai-chain-mock-account".to_owned(),
        compose_instance_id: "ai-chain-mock-compose".to_owned(),
        draft_id: None,
        local_version: None,
        compose: ComposeRequest {
            to: vec!["recipient@example.invalid".to_owned()],
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: subject.to_owned(),
            body_text: body.to_owned(),
            format: ComposeFormat::default(),
            reply_context: None,
        },
        attachments: Vec::new(),
        forward_context: None,
    }
}

fn context(harness: &Harness) -> AiExecutionContext {
    AiExecutionContext {
        backend: harness.backend.clone(),
        sender_email: "ai-chain@example.invalid".to_owned(),
        sender_remark: Some("测试用户".to_owned()),
        contacts: vec![AiContact {
            email: "teammate@example.invalid".to_owned(),
            display_name: "测试同事".to_owned(),
            is_favorite: true,
        }],
        attachments: Vec::new(),
        reply_context: None,
        forward_context: None,
    }
}

async fn run_turn_case(
    harness: &Harness,
    case: &'static str,
    mode: AiMode,
    instruction: &str,
    snapshot: AiDraftSnapshot,
    expect_change: bool,
    minimum_tool_activities: usize,
    required_changed_fields: &[&str],
) -> CaseReport {
    let original = snapshot.compose.clone();
    let started = Instant::now();
    let result = harness
        .runtime
        .run_turn(
            super::AiTurnRequest {
                mode,
                instruction: instruction.to_owned(),
                session_id: None,
                provider_instance_id: Some(harness.provider_instance_id.clone()),
                model_name: Some(harness.model.clone()),
                draft_revision: format!("manual-{case}"),
                draft: snapshot,
            },
            context(harness),
            None,
        )
        .await
        .unwrap_or_else(|error| panic!("{case} failed: {error}"));
    assert_eq!(result.status, "completed", "{case} did not complete");
    if expect_change {
        assert!(result.draft.is_some(), "{case} returned no proposal");
        assert!(
            !result.changed_fields.is_empty(),
            "{case} returned an empty change set"
        );
    } else {
        assert!(
            result.draft.is_none(),
            "{case} unexpectedly changed the draft"
        );
    }
    let output = result
        .draft
        .as_ref()
        .map(|value| format!("{}\n{}", value.subject, value.body_text))
        .unwrap_or_else(|| result.assistant_message.clone());
    if let Some(proposal) = result.draft.as_ref() {
        assert_ne!(&original, proposal, "{case} proposal is identical to input");
        assert!(
            !proposal.body_text.contains("\n\n"),
            "{case} produced blank lines in plain-text paragraphs"
        );
    }
    for field in required_changed_fields {
        assert!(
            result.changed_fields.iter().any(|changed| changed == field),
            "{case} did not change required field {field}"
        );
    }
    let tool_activity_count = result
        .session
        .iter()
        .flat_map(|session| session.messages.iter())
        .flat_map(|message| message.activities.iter())
        .filter(|activity| activity.kind == "tool" && activity.status == "completed")
        .count();
    assert!(
        tool_activity_count >= minimum_tool_activities,
        "{case} completed only {tool_activity_count} tool activities; expected at least {minimum_tool_activities}"
    );
    CaseReport {
        case,
        status: "passed",
        duration_ms: started.elapsed().as_millis(),
        changed_fields: result.changed_fields,
        decision: result.optimization_decision,
        tool_activity_count,
        output_bytes: output.len(),
        output_digest: digest(&output),
        note: if expect_change {
            "proposal validated in memory; not applied"
        } else {
            "read-only answer validated; no proposal"
        },
    }
}

fn selected_real_mail() -> SelectedMail {
    let data_root = PathBuf::from(required_env(MAIL_DATA_ROOT_ENV));
    assert!(
        data_root.is_dir(),
        "Mine Mail product-data directory does not exist"
    );
    let mut matches = Vec::new();
    for entry in fs::read_dir(&data_root).expect("read Mine Mail product-data directory") {
        let path = entry.expect("read product-data entry").path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !path.is_file()
            || !name.starts_with("mine-mail-")
            || !name.ends_with(".sqlite3")
            || name.ends_with("-oauth.sqlite3")
        {
            continue;
        }
        let connection = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .expect("open account mail database read-only");
        let has_messages = connection
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'messages'",
                [],
                |_| Ok(()),
            )
            .optional()
            .expect("inspect account mail database")
            .is_some();
        if !has_messages {
            continue;
        }
        let mut statement = connection
            .prepare(
                "SELECT public_id, subject, body_text, body_last_accessed_at
                 FROM messages
                 WHERE body_fetched = 1
                   AND length(trim(COALESCE(body_text, ''))) > 0
                   AND body_last_accessed_at IS NOT NULL
                 ORDER BY body_last_accessed_at DESC, id DESC
                 LIMIT 2",
            )
            .expect("prepare selected mail query");
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .expect("query selected mail");
        matches.extend(rows.map(|row| row.expect("read selected mail candidate")));
    }
    assert!(
        !matches.is_empty(),
        "no recently opened cached mail was found; open the target mail in Mine Mail first"
    );
    matches.sort_by(|left, right| right.3.cmp(&left.3));
    if matches.len() > 1 {
        assert_ne!(
            matches[0].3, matches[1].3,
            "multiple cached mails share the most-recent access time; reopen the target mail and retry"
        );
    }
    let (public_id, subject, body, _) = matches.remove(0);
    SelectedMail {
        subject,
        digest: digest(&format!("{}\0{}", public_id, body)),
        body,
    }
}

async fn run_translation_case(
    harness: &Harness,
    mail: &SelectedMail,
    case: &'static str,
    language_id: &str,
) -> CaseReport {
    let started = Instant::now();
    let result = harness
        .runtime
        .translate(AiTranslationRequest {
            language_id: Some(language_id.to_owned()),
            parts: vec![
                AiTranslationPartRequest {
                    id: AI_TRANSLATION_SUBJECT_PART_ID.to_owned(),
                    format: AiTranslationFormat::Plain,
                    content: mail.subject.clone(),
                },
                AiTranslationPartRequest {
                    id: "body-text".to_owned(),
                    format: AiTranslationFormat::Plain,
                    content: mail.body.clone(),
                },
            ],
        })
        .await
        .unwrap_or_else(|error| panic!("{case} failed: {error}"));
    assert_eq!(result.language, language_id);
    assert_eq!(
        result.translated_count, result.total_count,
        "{case} was partial"
    );
    assert!(result.total_count > 0, "{case} had no translation units");
    let output = result
        .parts
        .iter()
        .map(|part| part.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!output.trim().is_empty(), "{case} returned blank output");
    CaseReport {
        case,
        status: "passed",
        duration_ms: started.elapsed().as_millis(),
        changed_fields: Vec::new(),
        decision: None,
        tool_activity_count: 0,
        output_bytes: output.len(),
        output_digest: digest(&output),
        note: "real cached mail translated in memory; source and output not printed",
    }
}

#[tokio::test]
#[ignore = "manual, billable DeepSeek chain test; invoke only through $test-ai-chain"]
async fn manual_deepseek_ai_chain() {
    require_manual_guard();
    let (_directory, harness) = setup_harness();
    let mock = draft(
        "项目进度沟通",
        "你好，项目现在基本做完了，但是还有几个小问题，然后我们觉得应该很快完成。麻烦你看一下，有问题告诉我。谢谢。",
    );
    let mut cases = Vec::new();
    cases.push(
        run_turn_case(
            &harness,
            "optimize_mock_draft",
            AiMode::Optimize,
            "用户提供了以下优化要求：\n<user_instruction>\n请将邮件调整为自然、简洁、专业的中文，并整理成清晰段落。\n</user_instruction>",
            mock.clone(),
            true,
            0,
            &["body_text"],
        )
        .await,
    );
    cases.push(
        run_turn_case(
            &harness,
            "agent_read_analysis",
            AiMode::Auto,
            "请全面分析当前草稿的目的、事实、待办、语气和潜在歧义，只给建议，不要修改草稿。",
            mock.clone(),
            false,
            6,
            &[],
        )
        .await,
    );
    cases.push(
        run_turn_case(
            &harness,
            "agent_rewrite_proposal",
            AiMode::Chat,
            "请把主题改为“项目进度更新”，并重写当前邮件，使表达更专业、明确并保留全部事实，直接形成可应用的草稿提案。",
            mock.clone(),
            true,
            9,
            &["subject", "body_text"],
        )
        .await,
    );
    cases.push(
        run_turn_case(
            &harness,
            "agent_multi_function_edit",
            AiMode::Generate,
            "请把收件人改为测试同事，把主题改为“项目进度同步”，将正文重写得专业简洁并分成自然段，同时改用横线信纸但不要随邮件发送；保持中文、原意和全部事实。",
            mock,
            true,
            11,
            &["to", "subject", "body_text", "stationery"],
        )
        .await,
    );

    let mail = selected_real_mail();
    cases.push(run_translation_case(&harness, &mail, "translate_real_mail_to_zh", "zh-Hans").await);
    cases.push(run_translation_case(&harness, &mail, "translate_real_mail_to_en", "en").await);

    let report = ChainReport {
        provider: "deepseek",
        protocol: "openai_chat_completions",
        model: harness.model.clone(),
        real_mail_digest: Some(mail.digest),
        cases,
    };
    println!(
        "AI_CHAIN_REPORT {}",
        serde_json::to_string(&report).expect("serialize report summary")
    );
}
