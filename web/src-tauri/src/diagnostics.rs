use std::{
    collections::HashMap,
    fs,
    future::Future,
    io, panic,
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use log::{Level, LevelFilter};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager, Runtime, plugin::TauriPlugin};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};
use uuid::Uuid;

pub(crate) const LOG_FILE_NAME: &str = "mine-mail";
pub(crate) const LOG_FILE_MAX_BYTES: u128 = 5 * 1024 * 1024;
pub(crate) const LOG_ARCHIVE_COUNT: usize = 3;
pub(crate) const LOG_TOTAL_MAX_BYTES: u64 = 20 * 1024 * 1024;
pub(crate) const LOG_RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);

const DIAGNOSTIC_TARGET: &str = "mine_mail::diagnostics";
const DIAGNOSTIC_SCHEMA_VERSION: u8 = 3;
const LOG_ARCHIVE_MAX_BYTES: u64 = LOG_TOTAL_MAX_BYTES - LOG_FILE_MAX_BYTES as u64;
const FAILURE_EMIT_INTERVAL: Duration = Duration::from_secs(60);
const FAILURE_KEY_LIMIT: usize = 128;
const SLOW_COMMAND_THRESHOLD: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ErrorKind {
    Config,
    Validation,
    Database,
    Io,
    Serialization,
    Imap,
    Smtp,
    Mime,
    Timeout,
    NotFound,
    Runtime,
}

pub(crate) fn mail_error_kind(error: &mine_mail::MailError) -> ErrorKind {
    use mine_mail::MailError;

    match error {
        MailError::Config(_) => ErrorKind::Config,
        MailError::Validation(_) => ErrorKind::Validation,
        MailError::StaleCursor => ErrorKind::Validation,
        MailError::Database(_) => ErrorKind::Database,
        MailError::Io(_) => ErrorKind::Io,
        MailError::Serialization(_) => ErrorKind::Serialization,
        MailError::Imap(_) => ErrorKind::Imap,
        MailError::Smtp(_) => ErrorKind::Smtp,
        MailError::Connection(failure) => match failure.protocol {
            mine_mail::ConnectionProtocol::Imap => ErrorKind::Imap,
            mine_mail::ConnectionProtocol::Smtp => ErrorKind::Smtp,
        },
        MailError::Mime(_) => ErrorKind::Mime,
        MailError::Timeout { .. } => ErrorKind::Timeout,
        MailError::NotFound { .. } => ErrorKind::NotFound,
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct Fields {
    #[serde(skip_serializing_if = "Option::is_none")]
    operation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    account_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    item_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    operation: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    trigger: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    protocol: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    audit_reason_codes: Option<Vec<&'static str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    finish_reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    outcome: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_kind: Option<ErrorKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    force: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    degraded: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    incident_duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    account_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    success_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fetched_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    changed_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    changed_fields: Option<Vec<&'static str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    removed_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    conflict_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    attempt_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    batch_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    batch_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    suppressed_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    draft_version: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cleanup_removed_files: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cleanup_removed_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    moved_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    input_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_chunk_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_event_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    optimization_decision: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_shape: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    terminal_event_seen: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    json_depth: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    json_complete: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    json_error_kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    json_error_line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    json_error_column: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    app_version: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    platform: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    architecture: Option<&'static str>,
}

impl Fields {
    pub(crate) fn operation_id(mut self, value: OperationId) -> Self {
        self.operation_id = Some(value.0);
        self
    }

    pub(crate) fn operation_id_value(mut self, value: &str) -> Self {
        self.operation_id = Some(value.to_owned());
        self
    }

    pub(crate) fn account(mut self, account_id: &str) -> Self {
        self.account_ref = Some(private_ref("account", account_id));
        self
    }

    pub(crate) fn item(mut self, kind: &'static str, item_id: &str) -> Self {
        self.item_ref = Some(private_ref(kind, item_id));
        self
    }

    pub(crate) fn operation(mut self, value: &'static str) -> Self {
        self.operation = Some(value);
        self
    }

    pub(crate) fn trigger(mut self, value: &'static str) -> Self {
        self.trigger = Some(value);
        self
    }

    pub(crate) fn mode(mut self, value: &'static str) -> Self {
        self.mode = Some(value);
        self
    }

    pub(crate) fn provider(mut self, value: &'static str) -> Self {
        self.provider = Some(value);
        self
    }

    pub(crate) fn protocol(mut self, value: &'static str) -> Self {
        self.protocol = Some(value);
        self
    }

    pub(crate) fn model(mut self, value: &str) -> Self {
        self.model_ref = Some(private_ref("ai_model", value));
        self
    }

    pub(crate) fn tool(mut self, value: &'static str) -> Self {
        self.tool = Some(value);
        self
    }

    pub(crate) fn tool_calls(mut self, value: usize) -> Self {
        self.tool_call_count = Some(value);
        self
    }

    pub(crate) fn audit_reasons(mut self, value: Vec<&'static str>) -> Self {
        self.audit_reason_codes = Some(value);
        self
    }

    pub(crate) fn finish_reason(mut self, value: &'static str) -> Self {
        self.finish_reason = Some(value);
        self
    }

    pub(crate) fn outcome(mut self, value: &'static str) -> Self {
        self.outcome = Some(value);
        self
    }

    pub(crate) fn error(mut self, value: ErrorKind) -> Self {
        self.error_kind = Some(value);
        self
    }

    pub(crate) fn force(mut self, value: bool) -> Self {
        self.force = Some(value);
        self
    }

    pub(crate) fn degraded(mut self, value: bool) -> Self {
        self.degraded = Some(value);
        self
    }

    pub(crate) fn duration(mut self, value: Duration) -> Self {
        self.duration_ms = Some(value.as_millis().min(u128::from(u64::MAX)) as u64);
        self
    }

    pub(crate) fn runtime_metadata(mut self) -> Self {
        self.app_version = Some(env!("CARGO_PKG_VERSION"));
        self.platform = Some(std::env::consts::OS);
        self.architecture = Some(std::env::consts::ARCH);
        self
    }

    pub(crate) fn accounts(mut self, value: usize) -> Self {
        self.account_count = Some(value);
        self
    }

    pub(crate) fn successes(mut self, value: usize) -> Self {
        self.success_count = Some(value);
        self
    }

    pub(crate) fn failures(mut self, value: usize) -> Self {
        self.failure_count = Some(value);
        self
    }

    pub(crate) fn inbox_counts(mut self, fetched: usize, changed: usize, removed: usize) -> Self {
        self.fetched_count = Some(fetched);
        self.changed_count = Some(changed);
        self.removed_count = Some(removed);
        self
    }

    pub(crate) fn conflicts(mut self, value: usize) -> Self {
        self.conflict_count = Some(value);
        self
    }

    pub(crate) fn attempt(mut self, value: u64) -> Self {
        self.attempt_count = Some(value);
        self
    }

    pub(crate) fn batches(mut self, value: usize) -> Self {
        self.batch_count = Some(value);
        self
    }

    pub(crate) fn batch(mut self, index: usize, count: usize) -> Self {
        self.batch_index = Some(index);
        self.batch_count = Some(count);
        self
    }

    pub(crate) fn changes(mut self, value: usize) -> Self {
        self.changed_count = Some(value);
        self
    }

    pub(crate) fn change_set(mut self, value: Vec<&'static str>) -> Self {
        self.changed_fields = Some(value);
        self
    }

    pub(crate) fn draft_version(mut self, value: u64) -> Self {
        self.draft_version = Some(value);
        self
    }

    pub(crate) fn moved_bytes(mut self, value: u64) -> Self {
        self.moved_bytes = Some(value);
        self
    }

    pub(crate) fn payload_bytes(mut self, input: u64, output: u64) -> Self {
        self.input_bytes = Some(input);
        self.output_bytes = Some(output);
        self
    }

    pub(crate) fn tokens(mut self, input: u64, output: u64) -> Self {
        self.input_tokens = Some(input);
        self.output_tokens = Some(output);
        self
    }

    pub(crate) fn reasoning(mut self, bytes: u64, tokens: u64) -> Self {
        self.reasoning_bytes = Some(bytes);
        self.reasoning_tokens = Some(tokens);
        self
    }

    pub(crate) fn optimization_decision(mut self, value: &'static str) -> Self {
        self.optimization_decision = Some(value);
        self
    }

    pub(crate) fn output_shape(mut self, value: &'static str) -> Self {
        self.output_shape = Some(value);
        self
    }

    pub(crate) fn stream_state(
        mut self,
        chunks: usize,
        events: usize,
        content_bytes: u64,
        reasoning_bytes: u64,
        terminal_event_seen: bool,
        json_depth: i64,
        json_complete: bool,
    ) -> Self {
        self.stream_chunk_count = Some(chunks);
        self.stream_event_count = Some(events);
        self.content_bytes = Some(content_bytes);
        self.reasoning_bytes = Some(reasoning_bytes);
        self.terminal_event_seen = Some(terminal_event_seen);
        self.json_depth = Some(json_depth);
        self.json_complete = Some(json_complete);
        self
    }

    pub(crate) fn json_error(mut self, kind: &'static str, line: usize, column: usize) -> Self {
        self.json_error_kind = Some(kind);
        self.json_error_line = Some(line);
        self.json_error_column = Some(column);
        self
    }

    fn failure_summary(mut self, attempts: u64, suppressed: u64, duration: Duration) -> Self {
        self.attempt_count = Some(attempts);
        self.suppressed_count = Some(suppressed);
        self.incident_duration_ms = Some(duration.as_millis().min(u128::from(u64::MAX)) as u64);
        self
    }

    fn cleanup(mut self, report: CleanupReport) -> Self {
        self.cleanup_removed_files = Some(report.removed_files);
        self.cleanup_removed_bytes = Some(report.removed_bytes);
        self
    }
}

#[derive(Serialize)]
struct DiagnosticEvent<'a> {
    schema_version: u8,
    timestamp_utc_ms: u64,
    sequence: u64,
    uptime_ms: u64,
    session_id: &'a str,
    level: &'static str,
    event: &'static str,
    #[serde(flatten)]
    fields: Fields,
}

#[derive(Clone, Debug)]
pub(crate) struct OperationId(String);

impl OperationId {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

pub(crate) fn operation_id() -> OperationId {
    OperationId(Uuid::new_v4().to_string())
}

pub(crate) fn info(event: &'static str, fields: Fields) {
    emit(Level::Info, event, fields);
}

pub(crate) fn warn(event: &'static str, fields: Fields) {
    emit(Level::Warn, event, fields);
}

pub(crate) fn error(event: &'static str, fields: Fields) {
    emit(Level::Error, event, fields);
}

pub(crate) fn command<T, E>(
    operation: &'static str,
    fields: Fields,
    action: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    let fields = prepare_command_fields(operation, fields);
    let started = Instant::now();
    observe_command(operation, fields, started, false, action())
}

pub(crate) async fn command_async<T, E>(
    operation: &'static str,
    fields: Fields,
    action: impl Future<Output = Result<T, E>>,
) -> Result<T, E> {
    let fields = prepare_command_fields(operation, fields);
    let started = Instant::now();
    observe_command(operation, fields, started, false, action.await)
}

pub(crate) fn command_lifecycle<T, E>(
    operation: &'static str,
    fields: Fields,
    action: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    let fields = prepare_command_fields(operation, fields);
    let started = Instant::now();
    info("command_started", fields.clone());
    observe_command(operation, fields, started, true, action())
}

pub(crate) async fn command_lifecycle_async<T, E>(
    operation: &'static str,
    fields: Fields,
    action: impl Future<Output = Result<T, E>>,
) -> Result<T, E> {
    let fields = prepare_command_fields(operation, fields);
    let started = Instant::now();
    info("command_started", fields.clone());
    observe_command(operation, fields, started, true, action.await)
}

fn observe_command<T, E>(
    operation: &'static str,
    fields: Fields,
    started: Instant,
    lifecycle: bool,
    result: Result<T, E>,
) -> Result<T, E> {
    let elapsed = started.elapsed();
    if result.is_err() {
        limited_failure_with_fields(
            "command_failed",
            operation,
            ErrorKind::Runtime,
            fields.duration(elapsed),
        );
        return result;
    }

    limited_recovery_with_fields(
        "command_failed",
        "command_recovered",
        operation,
        fields.clone().duration(elapsed),
    );
    if lifecycle {
        info(
            "command_completed",
            fields.clone().outcome("completed").duration(elapsed),
        );
    }
    if elapsed >= SLOW_COMMAND_THRESHOLD {
        warn(
            "command_slow",
            fields.outcome("completed").duration(elapsed),
        );
    }
    result
}

fn prepare_command_fields(operation: &'static str, mut fields: Fields) -> Fields {
    fields.operation = Some(operation);
    if fields.operation_id.is_none() {
        fields.operation_id = Some(Uuid::new_v4().to_string());
    }
    fields
}

pub(crate) fn emit_event<R, S>(app: &AppHandle<R>, event: &'static str, payload: S)
where
    R: Runtime,
    S: Serialize + Clone,
{
    match app.emit(event, payload) {
        Ok(()) => limited_recovery(
            "frontend_event_emit_failed",
            "frontend_event_emit_recovered",
            event,
            None,
        ),
        Err(_) => limited_failure(
            "frontend_event_emit_failed",
            event,
            None,
            ErrorKind::Serialization,
        ),
    }
}

pub(crate) fn emit_to_event<R, S>(
    app: &AppHandle<R>,
    target: &'static str,
    event: &'static str,
    payload: S,
) where
    R: Runtime,
    S: Serialize + Clone,
{
    match app.emit_to(target, event, payload) {
        Ok(()) => limited_recovery_with_fields(
            "frontend_event_emit_failed",
            "frontend_event_emit_recovered",
            event,
            Fields::default().mode(target),
        ),
        Err(_) => limited_failure_with_fields(
            "frontend_event_emit_failed",
            event,
            ErrorKind::Serialization,
            Fields::default().mode(target),
        ),
    }
}

pub(crate) fn install_panic_hook() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let _ = panic::take_hook();
        panic::set_hook(Box::new(|_| {
            error(
                "runtime_panic",
                Fields::default()
                    .operation("panic_hook")
                    .error(ErrorKind::Runtime),
            );
        }));
    });
}

fn emit(level: Level, event: &'static str, fields: Fields) {
    let line = serialize_event(level, event, fields);
    log::log!(target: DIAGNOSTIC_TARGET, level, "{line}");
}

fn serialize_event(level: Level, event: &'static str, fields: Fields) -> String {
    let record = DiagnosticEvent {
        schema_version: DIAGNOSTIC_SCHEMA_VERSION,
        timestamp_utc_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64,
        sequence: next_sequence(),
        uptime_ms: session_started_at()
            .elapsed()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64,
        session_id: session_id(),
        level: match level {
            Level::Error => "error",
            Level::Warn => "warn",
            Level::Info => "info",
            Level::Debug => "debug",
            Level::Trace => "trace",
        },
        event,
        fields,
    };
    serde_json::to_string(&record).unwrap_or_else(|_| {
        format!(
            "{{\"schema_version\":{DIAGNOSTIC_SCHEMA_VERSION},\"timestamp_utc_ms\":0,\"sequence\":{},\"uptime_ms\":0,\"session_id\":\"{}\",\"level\":\"error\",\"event\":\"diagnostic_serialization_failed\"}}",
            next_sequence(), session_id()
        )
    })
}

fn session_id() -> &'static str {
    static SESSION_ID: OnceLock<String> = OnceLock::new();
    SESSION_ID.get_or_init(|| Uuid::new_v4().to_string())
}

fn session_started_at() -> &'static Instant {
    static SESSION_STARTED_AT: OnceLock<Instant> = OnceLock::new();
    SESSION_STARTED_AT.get_or_init(Instant::now)
}

fn next_sequence() -> u64 {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    SEQUENCE.fetch_add(1, Ordering::Relaxed).saturating_add(1)
}

fn private_ref(kind: &'static str, value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(kind.as_bytes());
    hasher.update([0]);
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    digest[..6]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) fn plugin<R: Runtime>() -> TauriPlugin<R> {
    let file_target = Target::new(TargetKind::LogDir {
        file_name: Some(LOG_FILE_NAME.to_owned()),
    })
    .filter(|metadata| metadata.target() == DIAGNOSTIC_TARGET)
    .format(|out, message, _record| out.finish(format_args!("{message}")));

    tauri_plugin_log::Builder::new()
        .clear_targets()
        .target(file_target)
        .filter(|metadata| metadata.target() == DIAGNOSTIC_TARGET)
        .level(LevelFilter::Info)
        .max_file_size(LOG_FILE_MAX_BYTES)
        .rotation_strategy(RotationStrategy::KeepSome(LOG_ARCHIVE_COUNT))
        .build()
}

pub(crate) fn cleanup_on_startup<R: Runtime>(app: &AppHandle<R>) {
    let result = app
        .path()
        .app_log_dir()
        .map_err(io::Error::other)
        .and_then(|path| cleanup_log_dir(&path, LOG_RETENTION, LOG_ARCHIVE_MAX_BYTES));
    match result {
        Ok(report) if report.removed_files > 0 => {
            info("log_cleanup_completed", Fields::default().cleanup(report));
        }
        Ok(_) => {}
        Err(_) => warn("log_cleanup_failed", Fields::default().error(ErrorKind::Io)),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct CleanupReport {
    removed_files: usize,
    removed_bytes: u64,
}

#[derive(Debug)]
struct LogFile {
    path: PathBuf,
    modified: SystemTime,
    size: u64,
}

fn cleanup_log_dir(
    directory: &Path,
    max_age: Duration,
    max_archived_bytes: u64,
) -> io::Result<CleanupReport> {
    let mut report = CleanupReport::default();
    let now = SystemTime::now();
    let mut archives = Vec::new();

    for entry in fs::read_dir(directory)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if !is_owned_archive(&file_name) {
            continue;
        }
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue,
        };
        if !file_type.is_file() {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
        let size = metadata.len();
        let expired = now.duration_since(modified).is_ok_and(|age| age > max_age);
        if expired && fs::remove_file(entry.path()).is_ok() {
            report.removed_files += 1;
            report.removed_bytes = report.removed_bytes.saturating_add(size);
            continue;
        }
        archives.push(LogFile {
            path: entry.path(),
            modified,
            size,
        });
    }

    archives.sort_by_key(|file| file.modified);
    let mut archived_bytes = archives
        .iter()
        .fold(0u64, |total, file| total.saturating_add(file.size));
    for file in archives {
        if archived_bytes <= max_archived_bytes {
            break;
        }
        if fs::remove_file(&file.path).is_ok() {
            archived_bytes = archived_bytes.saturating_sub(file.size);
            report.removed_files += 1;
            report.removed_bytes = report.removed_bytes.saturating_add(file.size);
        }
    }

    Ok(report)
}

fn is_owned_archive(file_name: &str) -> bool {
    let prefix = format!("{LOG_FILE_NAME}_");
    file_name.starts_with(&prefix)
        && (file_name.ends_with(".log") || file_name.ends_with(".log.bak"))
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct FailureKey {
    event: &'static str,
    operation: &'static str,
    account_ref: Option<String>,
    error_kind: ErrorKind,
}

#[derive(Clone, Copy, Debug)]
struct FailureState {
    first_seen: Instant,
    last_emitted: Instant,
    attempts: u64,
    suppressed: u64,
}

#[derive(Default)]
struct FailureLimiter {
    entries: HashMap<FailureKey, FailureState>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FailureSummary {
    attempts: u64,
    suppressed: u64,
    duration: Duration,
}

impl FailureLimiter {
    fn failure(&mut self, key: FailureKey, now: Instant) -> Option<FailureSummary> {
        if let Some(state) = self.entries.get_mut(&key) {
            state.attempts = state.attempts.saturating_add(1);
            if now.duration_since(state.last_emitted) < FAILURE_EMIT_INTERVAL {
                state.suppressed = state.suppressed.saturating_add(1);
                return None;
            }
            let summary = FailureSummary {
                attempts: state.attempts,
                suppressed: state.suppressed,
                duration: now.duration_since(state.first_seen),
            };
            state.last_emitted = now;
            state.suppressed = 0;
            return Some(summary);
        }

        if self.entries.len() >= FAILURE_KEY_LIMIT
            && let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, state)| state.first_seen)
                .map(|(key, _)| key.clone())
        {
            self.entries.remove(&oldest);
        }
        self.entries.insert(
            key,
            FailureState {
                first_seen: now,
                last_emitted: now,
                attempts: 1,
                suppressed: 0,
            },
        );
        Some(FailureSummary {
            attempts: 1,
            suppressed: 0,
            duration: Duration::ZERO,
        })
    }

    fn recoveries(
        &mut self,
        event: &'static str,
        operation: &'static str,
        account_ref: Option<&str>,
        now: Instant,
    ) -> Vec<FailureSummary> {
        let keys = self
            .entries
            .keys()
            .filter(|key| {
                key.event == event
                    && key.operation == operation
                    && key.account_ref.as_deref() == account_ref
            })
            .cloned()
            .collect::<Vec<_>>();
        keys.into_iter()
            .filter_map(|key| self.entries.remove(&key))
            .map(|state| FailureSummary {
                attempts: state.attempts,
                suppressed: state.suppressed,
                duration: now.duration_since(state.first_seen),
            })
            .collect()
    }
}

fn failure_limiter() -> &'static Mutex<FailureLimiter> {
    static LIMITER: OnceLock<Mutex<FailureLimiter>> = OnceLock::new();
    LIMITER.get_or_init(|| Mutex::new(FailureLimiter::default()))
}

pub(crate) fn limited_failure(
    event: &'static str,
    operation: &'static str,
    account_id: Option<&str>,
    error_kind: ErrorKind,
) {
    let mut fields = Fields::default();
    fields.account_ref = account_id.map(|value| private_ref("account", value));
    limited_failure_with_fields(event, operation, error_kind, fields);
}

pub(crate) fn limited_failure_with_fields(
    event: &'static str,
    operation: &'static str,
    error_kind: ErrorKind,
    mut fields: Fields,
) {
    let account_ref = fields.account_ref.clone();
    let key = FailureKey {
        event,
        operation,
        account_ref: account_ref.clone(),
        error_kind,
    };
    let summary = failure_limiter()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .failure(key, Instant::now());
    if let Some(summary) = summary {
        fields.operation = Some(operation);
        fields.error_kind = Some(error_kind);
        fields = fields.failure_summary(summary.attempts, summary.suppressed, summary.duration);
        error(event, fields);
    }
}

pub(crate) fn limited_recovery(
    failure_event: &'static str,
    recovery_event: &'static str,
    operation: &'static str,
    account_id: Option<&str>,
) {
    let mut fields = Fields::default();
    fields.account_ref = account_id.map(|value| private_ref("account", value));
    limited_recovery_with_fields(failure_event, recovery_event, operation, fields);
}

pub(crate) fn limited_recovery_with_fields(
    failure_event: &'static str,
    recovery_event: &'static str,
    operation: &'static str,
    mut fields: Fields,
) {
    let account_ref = fields.account_ref.clone();
    let summaries = failure_limiter()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .recoveries(
            failure_event,
            operation,
            account_ref.as_deref(),
            Instant::now(),
        );
    if !summaries.is_empty() {
        let summary = summaries.into_iter().fold(
            FailureSummary {
                attempts: 0,
                suppressed: 0,
                duration: Duration::ZERO,
            },
            |combined, current| FailureSummary {
                attempts: combined.attempts.saturating_add(current.attempts),
                suppressed: combined.suppressed.saturating_add(current.suppressed),
                duration: combined.duration.max(current.duration),
            },
        );
        fields.operation = Some(operation);
        fields.outcome = Some("recovered");
        fields = fields.failure_summary(summary.attempts, summary.suppressed, summary.duration);
        info(recovery_event, fields);
    }
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::Write};

    use super::*;

    #[test]
    fn private_references_are_stable_and_do_not_reveal_input() {
        let email = "person@example.com";
        let first = private_ref("account", email);
        assert_eq!(first, private_ref("account", email));
        assert_eq!(first.len(), 12);
        assert!(!first.contains("person"));
        assert!(!first.contains('@'));
    }

    #[test]
    fn serialized_event_contains_only_safe_typed_fields() {
        let line = serialize_event(
            Level::Info,
            "send_completed",
            Fields::default()
                .account("person@example.com")
                .item("draft", "raw-draft-id")
                .operation("send")
                .outcome("sent")
                .runtime_metadata()
                .changes(2)
                .moved_bytes(128)
                .draft_version(7),
        );
        let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed["schema_version"], DIAGNOSTIC_SCHEMA_VERSION);
        assert_eq!(parsed["event"], "send_completed");
        assert_eq!(parsed["draft_version"], 7);
        assert_eq!(parsed["changed_count"], 2);
        assert_eq!(parsed["moved_bytes"], 128);
        assert_eq!(parsed["app_version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(parsed["platform"], std::env::consts::OS);
        assert_eq!(parsed["architecture"], std::env::consts::ARCH);
        assert!(parsed["sequence"].as_u64().is_some_and(|value| value > 0));
        assert!(parsed["uptime_ms"].is_u64());
        assert!(!line.contains("person@example.com"));
        assert!(!line.contains("raw-draft-id"));
        assert!(!line.contains("password"));
        assert!(!line.contains("RFC822"));
    }

    #[test]
    fn serialized_event_schema_has_an_explicit_privacy_safe_allowlist() {
        let line = serialize_event(
            Level::Warn,
            "schema_contract",
            Fields::default()
                .operation_id(operation_id())
                .account("private-account")
                .item("message", "private-message")
                .operation("schema_test")
                .trigger("test")
                .mode("offline")
                .provider("mimo")
                .protocol("openai_responses")
                .model("mimo-v2.5")
                .tool("replace_draft_body")
                .finish_reason("tool_calls")
                .outcome("degraded")
                .error(ErrorKind::Runtime)
                .force(true)
                .degraded(true)
                .duration(Duration::from_millis(10))
                .accounts(3)
                .successes(2)
                .failures(1)
                .inbox_counts(4, 5, 6)
                .conflicts(1)
                .draft_version(9)
                .batch(2, 4)
                .stream_state(10, 20, 30, 40, false, 2, false)
                .reasoning(40, 6)
                .optimization_decision("changed")
                .output_shape("markdown_fence")
                .json_error("syntax", 2, 17),
        );
        let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed["json_error_kind"], "syntax");
        assert_eq!(parsed["json_error_line"], 2);
        assert_eq!(parsed["json_error_column"], 17);
        assert_eq!(parsed["batch_index"], 2);
        assert_eq!(parsed["batch_count"], 4);
        assert_eq!(parsed["stream_chunk_count"], 10);
        assert_eq!(parsed["stream_event_count"], 20);
        assert_eq!(parsed["content_bytes"], 30);
        assert_eq!(parsed["reasoning_bytes"], 40);
        assert_eq!(parsed["reasoning_tokens"], 6);
        assert_eq!(parsed["optimization_decision"], "changed");
        assert_eq!(parsed["output_shape"], "markdown_fence");
        assert_eq!(parsed["terminal_event_seen"], false);
        assert_eq!(parsed["json_depth"], 2);
        assert_eq!(parsed["json_complete"], false);
        let allowed = [
            "schema_version",
            "timestamp_utc_ms",
            "sequence",
            "uptime_ms",
            "session_id",
            "level",
            "event",
            "operation_id",
            "account_ref",
            "item_ref",
            "operation",
            "trigger",
            "mode",
            "provider",
            "protocol",
            "model_ref",
            "tool",
            "finish_reason",
            "outcome",
            "error_kind",
            "force",
            "degraded",
            "duration_ms",
            "incident_duration_ms",
            "account_count",
            "success_count",
            "failure_count",
            "fetched_count",
            "changed_count",
            "removed_count",
            "conflict_count",
            "attempt_count",
            "batch_index",
            "batch_count",
            "suppressed_count",
            "draft_version",
            "cleanup_removed_files",
            "cleanup_removed_bytes",
            "moved_bytes",
            "stream_chunk_count",
            "stream_event_count",
            "content_bytes",
            "reasoning_bytes",
            "reasoning_tokens",
            "optimization_decision",
            "output_shape",
            "terminal_event_seen",
            "json_depth",
            "json_complete",
            "json_error_kind",
            "json_error_line",
            "json_error_column",
            "app_version",
            "platform",
            "architecture",
        ];
        for key in parsed.as_object().unwrap().keys() {
            assert!(
                allowed.contains(&key.as_str()),
                "unexpected diagnostic field: {key}"
            );
        }
        assert!(!line.contains("private-account"));
        assert!(!line.contains("private-message"));
    }

    #[test]
    fn raw_mail_error_text_never_enters_the_event() {
        let raw_secret = "authorization-secret-should-never-appear";
        let error = mine_mail::MailError::Imap(raw_secret.to_owned());
        let line = serialize_event(
            Level::Error,
            "account_sync_failed",
            Fields::default().error(mail_error_kind(&error)),
        );

        assert!(line.contains("\"error_kind\":\"imap\""));
        assert!(!line.contains(raw_secret));
    }

    #[test]
    fn every_mail_error_variant_maps_to_a_bounded_category() {
        use mine_mail::{ConnectionFailure, ConnectionFailureKind, ConnectionProtocol, MailError};

        let cases = [
            (MailError::Config("secret".to_owned()), ErrorKind::Config),
            (
                MailError::Validation("secret".to_owned()),
                ErrorKind::Validation,
            ),
            (
                MailError::Database(rusqlite::Error::InvalidQuery),
                ErrorKind::Database,
            ),
            (
                MailError::Io(std::io::Error::other("secret")),
                ErrorKind::Io,
            ),
            (
                MailError::Serialization(
                    serde_json::from_str::<serde_json::Value>("{").unwrap_err(),
                ),
                ErrorKind::Serialization,
            ),
            (MailError::Imap("secret".to_owned()), ErrorKind::Imap),
            (MailError::Smtp("secret".to_owned()), ErrorKind::Smtp),
            (
                MailError::Connection(ConnectionFailure::new(
                    ConnectionProtocol::Imap,
                    ConnectionFailureKind::Network,
                )),
                ErrorKind::Imap,
            ),
            (
                MailError::Connection(ConnectionFailure::new(
                    ConnectionProtocol::Smtp,
                    ConnectionFailureKind::Authentication,
                )),
                ErrorKind::Smtp,
            ),
            (MailError::Mime("secret".to_owned()), ErrorKind::Mime),
            (
                MailError::Timeout {
                    operation: "test_operation",
                },
                ErrorKind::Timeout,
            ),
            (
                MailError::NotFound {
                    entity: "message",
                    id: "secret".to_owned(),
                },
                ErrorKind::NotFound,
            ),
        ];
        for (error, expected) in cases {
            assert_eq!(mail_error_kind(&error), expected);
        }
    }

    #[test]
    fn limiter_suppresses_repeats_and_summarizes_recovery() {
        let start = Instant::now();
        let key = FailureKey {
            event: "monitor_failed",
            operation: "monitor_connect",
            account_ref: Some("safe-ref".to_owned()),
            error_kind: ErrorKind::Imap,
        };
        let mut limiter = FailureLimiter::default();
        assert_eq!(limiter.failure(key.clone(), start).unwrap().attempts, 1);
        assert!(
            limiter
                .failure(key.clone(), start + Duration::from_secs(1))
                .is_none()
        );
        let recovery = limiter
            .recoveries(
                "monitor_failed",
                "monitor_connect",
                Some("safe-ref"),
                start + Duration::from_secs(2),
            )
            .pop()
            .unwrap();
        assert_eq!(recovery.attempts, 2);
        assert_eq!(recovery.suppressed, 1);
    }

    #[test]
    fn limiter_reemits_after_interval_and_bounds_distinct_failures() {
        let start = Instant::now();
        let mut limiter = FailureLimiter::default();
        let first_key = FailureKey {
            event: "bounded_failure",
            operation: "operation_0",
            account_ref: None,
            error_kind: ErrorKind::Runtime,
        };
        assert!(limiter.failure(first_key.clone(), start).is_some());
        assert!(
            limiter
                .failure(first_key, start + FAILURE_EMIT_INTERVAL)
                .is_some()
        );

        for index in 1..=FAILURE_KEY_LIMIT {
            let operation = Box::leak(format!("operation_{index}").into_boxed_str());
            let key = FailureKey {
                event: "bounded_failure",
                operation,
                account_ref: None,
                error_kind: ErrorKind::Runtime,
            };
            assert!(
                limiter
                    .failure(key, start + Duration::from_millis(index as u64))
                    .is_some()
            );
        }
        assert_eq!(limiter.entries.len(), FAILURE_KEY_LIMIT);
        assert!(
            !limiter
                .entries
                .keys()
                .any(|key| key.operation == "operation_0")
        );
    }

    fn command_name(block: &str) -> &str {
        let function = block.find("fn ").expect("Tauri command has a function");
        block[function + 3..]
            .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
            .next()
            .expect("Tauri command has a function name")
    }

    #[test]
    fn fallible_tauri_command_diagnostic_coverage_exceeds_ninety_five_percent() {
        let sources = [include_str!("lib.rs"), include_str!("mailbox_api.rs")];
        let blocks = sources
            .iter()
            .flat_map(|source| source.split("#[tauri::command]").skip(1))
            .collect::<Vec<_>>();
        let mut instrumented = 0usize;
        for block in &blocks {
            let signature = block
                .split_once('{')
                .map(|(signature, _)| signature)
                .expect("Tauri command has a body");
            let name = command_name(block);
            let has_diagnostics = block.contains("diagnostics::command");
            if signature.contains("CommandResult<") {
                assert!(
                    has_diagnostics,
                    "fallible Tauri command is not observed: {name}"
                );
                assert!(
                    block.contains(&format!("\"{name}\"")),
                    "Tauri command uses a mismatched diagnostic operation: {name}"
                );
            }
            instrumented += usize::from(has_diagnostics);
        }

        assert_eq!(blocks.len(), 89, "update the command coverage contract");
        assert_eq!(instrumented, 87, "update the command coverage contract");
        assert!(
            instrumented * 100 > blocks.len() * 95,
            "diagnostic command coverage must remain above 95%"
        );
    }

    #[test]
    fn cleanup_removes_only_owned_archives_to_meet_cap() {
        let directory = tempfile::tempdir().unwrap();
        let active = directory.path().join("mine-mail.log");
        let unrelated = directory.path().join("other.log");
        let old_archive = directory.path().join("mine-mail_2026-01-01_00-00-00.log");
        let new_archive = directory.path().join("mine-mail_2026-01-02_00-00-00.log");
        for path in [&active, &unrelated, &old_archive, &new_archive] {
            let mut file = File::create(path).unwrap();
            file.write_all(&[0; 16]).unwrap();
        }

        let old_time = SystemTime::now() - Duration::from_secs(2);
        File::options()
            .write(true)
            .open(&old_archive)
            .unwrap()
            .set_modified(old_time)
            .unwrap();
        let report = cleanup_log_dir(directory.path(), Duration::from_secs(1), 16).unwrap();

        assert_eq!(report.removed_files, 1);
        assert!(!old_archive.exists());
        assert!(new_archive.exists());
        assert!(active.exists());
        assert!(unrelated.exists());
    }

    #[test]
    fn cleanup_removes_oldest_owned_archives_until_under_size_cap() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("mine-mail_2026-01-01_00-00-00.log");
        let second = directory.path().join("mine-mail_2026-01-02_00-00-00.log");
        let third = directory.path().join("mine-mail_2026-01-03_00-00-00.log");
        for path in [&first, &second, &third] {
            let mut file = File::create(path).unwrap();
            file.write_all(&[0; 16]).unwrap();
        }
        File::options()
            .write(true)
            .open(&first)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(3))
            .unwrap();
        File::options()
            .write(true)
            .open(&second)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(2))
            .unwrap();

        let report = cleanup_log_dir(directory.path(), Duration::from_secs(60), 16).unwrap();

        assert_eq!(report.removed_files, 2);
        assert!(!first.exists());
        assert!(!second.exists());
        assert!(third.exists());
    }

    #[test]
    fn rotation_policy_has_a_hard_twenty_mebibyte_envelope() {
        assert_eq!(
            LOG_FILE_MAX_BYTES as u64 + LOG_ARCHIVE_MAX_BYTES,
            LOG_TOTAL_MAX_BYTES
        );
        assert_eq!(LOG_ARCHIVE_COUNT, 3);
    }
}
