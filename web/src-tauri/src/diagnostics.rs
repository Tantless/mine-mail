use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use log::{Level, LevelFilter};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager, Runtime, plugin::TauriPlugin};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};
use uuid::Uuid;

pub(crate) const LOG_FILE_NAME: &str = "mine-mail";
pub(crate) const LOG_FILE_MAX_BYTES: u128 = 5 * 1024 * 1024;
pub(crate) const LOG_ARCHIVE_COUNT: usize = 3;
pub(crate) const LOG_TOTAL_MAX_BYTES: u64 = 20 * 1024 * 1024;
pub(crate) const LOG_RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);

const DIAGNOSTIC_TARGET: &str = "mine_mail::diagnostics";
const LOG_ARCHIVE_MAX_BYTES: u64 = LOG_TOTAL_MAX_BYTES - LOG_FILE_MAX_BYTES as u64;
const FAILURE_EMIT_INTERVAL: Duration = Duration::from_secs(60);
const FAILURE_KEY_LIMIT: usize = 128;

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
        MailError::Database(_) => ErrorKind::Database,
        MailError::Io(_) => ErrorKind::Io,
        MailError::Serialization(_) => ErrorKind::Serialization,
        MailError::Imap(_) => ErrorKind::Imap,
        MailError::Smtp(_) => ErrorKind::Smtp,
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
    removed_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    conflict_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    attempt_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    suppressed_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    draft_version: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cleanup_removed_files: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cleanup_removed_bytes: Option<u64>,
}

impl Fields {
    pub(crate) fn operation_id(mut self, value: OperationId) -> Self {
        self.operation_id = Some(value.0);
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

    pub(crate) fn draft_version(mut self, value: u64) -> Self {
        self.draft_version = Some(value);
        self
    }

    fn failure_summary(mut self, attempts: u64, suppressed: u64, duration: Duration) -> Self {
        self.attempt_count = Some(attempts);
        self.suppressed_count = Some(suppressed);
        self.duration(duration)
    }

    fn cleanup(mut self, report: CleanupReport) -> Self {
        self.cleanup_removed_files = Some(report.removed_files);
        self.cleanup_removed_bytes = Some(report.removed_bytes);
        self
    }
}

#[derive(Serialize)]
struct DiagnosticEvent<'a> {
    timestamp_utc_ms: u64,
    session_id: &'a str,
    level: &'static str,
    event: &'static str,
    #[serde(flatten)]
    fields: Fields,
}

#[derive(Clone, Debug)]
pub(crate) struct OperationId(String);

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

fn emit(level: Level, event: &'static str, fields: Fields) {
    let line = serialize_event(level, event, fields);
    log::log!(target: DIAGNOSTIC_TARGET, level, "{line}");
}

fn serialize_event(level: Level, event: &'static str, fields: Fields) -> String {
    let record = DiagnosticEvent {
        timestamp_utc_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
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
            "{{\"timestamp_utc_ms\":0,\"session_id\":\"{}\",\"level\":\"error\",\"event\":\"diagnostic_serialization_failed\"}}",
            session_id()
        )
    })
}

fn session_id() -> &'static str {
    static SESSION_ID: OnceLock<String> = OnceLock::new();
    SESSION_ID.get_or_init(|| Uuid::new_v4().to_string())
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
    let account_ref = account_id.map(|value| private_ref("account", value));
    let key = FailureKey {
        event,
        operation,
        account_ref: account_ref.clone(),
        error_kind,
    };
    let summary = failure_limiter()
        .lock()
        .ok()
        .and_then(|mut limiter| limiter.failure(key, Instant::now()));
    if let Some(summary) = summary {
        let mut fields = Fields::default()
            .operation(operation)
            .error(error_kind)
            .failure_summary(summary.attempts, summary.suppressed, summary.duration);
        fields.account_ref = account_ref;
        error(event, fields);
    }
}

pub(crate) fn limited_recovery(
    failure_event: &'static str,
    recovery_event: &'static str,
    operation: &'static str,
    account_id: Option<&str>,
) {
    let account_ref = account_id.map(|value| private_ref("account", value));
    let summaries = failure_limiter()
        .lock()
        .ok()
        .map(|mut limiter| {
            limiter.recoveries(
                failure_event,
                operation,
                account_ref.as_deref(),
                Instant::now(),
            )
        })
        .unwrap_or_default();
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
        let mut fields = Fields::default()
            .operation(operation)
            .outcome("recovered")
            .failure_summary(summary.attempts, summary.suppressed, summary.duration);
        fields.account_ref = account_ref;
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
                .draft_version(7),
        );
        let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed["event"], "send_completed");
        assert_eq!(parsed["draft_version"], 7);
        assert!(!line.contains("person@example.com"));
        assert!(!line.contains("raw-draft-id"));
        assert!(!line.contains("password"));
        assert!(!line.contains("RFC822"));
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
