mod settings;

use std::{
    fs,
    path::Path,
    sync::{Mutex as StdMutex, RwLock},
    time::{Duration, Instant},
};

use mine_mail::{DraftSyncReport, InboxMessage, SyncReport};
use serde::Serialize;
use tauri::{
    App, AppHandle, Emitter, Manager,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_notification::{NotificationExt, PermissionState};
use tokio::sync::{Mutex as AsyncMutex, MutexGuard as AsyncMutexGuard, mpsc, watch};
use tokio::time::Instant as TokioInstant;

use crate::account::BackendState;

pub(crate) use settings::{DesktopSettingsDto, DesktopSettingsUpdate};
use settings::{DesktopSettingsStore, StoredDesktopSettings, valid_poll_interval};

const MINIMUM_AUTOMATIC_SYNC_GAP: Duration = Duration::from_secs(30);
const DRAFT_SYNC_INTERVAL: Duration = Duration::from_secs(5 * 60);
const EXIT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
const SETTINGS_DATABASE_NAME: &str = "desktop-runtime.sqlite3";

#[derive(Clone, Copy, Debug)]
pub(crate) enum BackgroundRequest {
    Sync { force: bool },
    ScheduleChanged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ExitHandshakeTicket {
    request_id: u64,
    generation: u64,
    deadline: Instant,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ExitHandshakePhase {
    #[default]
    Idle,
    Awaiting(ExitHandshakeTicket),
    Committed(ExitHandshakeTicket),
}

#[derive(Debug, Default)]
struct ExitHandshakeState {
    last_request_id: u64,
    generation: u64,
    phase: ExitHandshakePhase,
}

pub(crate) struct DesktopRuntime {
    settings: RwLock<StoredDesktopSettings>,
    store: Option<DesktopSettingsStore>,
    startup_error: RwLock<Option<String>>,
    sync_tx: mpsc::Sender<BackgroundRequest>,
    shutdown_tx: watch::Sender<bool>,
    sync_gate: AsyncMutex<()>,
    last_sync_started: StdMutex<Option<Instant>>,
    exit_handshake: StdMutex<ExitHandshakeState>,
}

impl DesktopRuntime {
    pub(crate) fn open(
        app_data: &Path,
    ) -> (
        Self,
        mpsc::Receiver<BackgroundRequest>,
        watch::Receiver<bool>,
    ) {
        let (store, settings, startup_error) = if fs::create_dir_all(app_data).is_err() {
            (
                None,
                StoredDesktopSettings::default(),
                Some("The desktop settings directory is unavailable.".to_owned()),
            )
        } else {
            match DesktopSettingsStore::open(app_data.join(SETTINGS_DATABASE_NAME)) {
                Ok(store) => match store.load() {
                    Ok(settings) => (Some(store), settings, None),
                    Err(_) => (
                        None,
                        StoredDesktopSettings::default(),
                        Some(
                            "Desktop settings could not be loaded; safe in-memory defaults are active."
                                .to_owned(),
                        ),
                    ),
                },
                Err(_) => (
                    None,
                    StoredDesktopSettings::default(),
                    Some(
                        "Desktop settings could not be initialized; safe in-memory defaults are active."
                            .to_owned(),
                    ),
                ),
            }
        };
        let (sync_tx, sync_rx) = mpsc::channel(8);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        (
            Self {
                settings: RwLock::new(settings),
                store,
                startup_error: RwLock::new(startup_error),
                sync_tx,
                shutdown_tx,
                sync_gate: AsyncMutex::new(()),
                last_sync_started: StdMutex::new(None),
                exit_handshake: StdMutex::new(ExitHandshakeState::default()),
            },
            sync_rx,
            shutdown_rx,
        )
    }

    pub(crate) fn settings_dto(
        &self,
        autostart_enabled: bool,
    ) -> Result<DesktopSettingsDto, String> {
        let settings = self.settings()?;
        Ok(DesktopSettingsDto {
            background_enabled: settings.background_enabled,
            poll_interval_minutes: settings.poll_interval_minutes,
            notifications_enabled: settings.notifications_enabled,
            autostart_enabled,
            startup_error: self
                .startup_error
                .read()
                .map_err(|_| "Desktop diagnostics are temporarily unavailable.".to_owned())?
                .clone(),
        })
    }

    pub(crate) fn update_settings(&self, update: DesktopSettingsUpdate) -> Result<(), String> {
        let mut settings = self.settings()?;
        if let Some(value) = update.background_enabled {
            settings.background_enabled = value;
        }
        if let Some(value) = update.poll_interval_minutes {
            if !valid_poll_interval(value) {
                return Err("Polling interval must be 1, 3, or 5 minutes.".to_owned());
            }
            settings.poll_interval_minutes = value;
        }
        if let Some(value) = update.notifications_enabled {
            settings.notifications_enabled = value;
        }

        self.persist_settings(settings, "Desktop settings could not be saved.")?;
        *self
            .settings
            .write()
            .map_err(|_| "Desktop settings are temporarily unavailable.".to_owned())? = settings;
        let _ = self.sync_tx.try_send(BackgroundRequest::ScheduleChanged);
        Ok(())
    }

    pub(crate) fn user_settings_snapshot(&self) -> Result<DesktopSettingsUpdate, String> {
        let settings = self.settings()?;
        Ok(DesktopSettingsUpdate {
            background_enabled: Some(settings.background_enabled),
            poll_interval_minutes: Some(settings.poll_interval_minutes),
            notifications_enabled: Some(settings.notifications_enabled),
            autostart_enabled: None,
        })
    }

    pub(crate) fn record_startup_error(&self, error: impl Into<String>) {
        let error = error.into();
        if let Ok(mut current) = self.startup_error.write() {
            match current.as_mut() {
                Some(message) if !message.contains(&error) => {
                    message.push(' ');
                    message.push_str(&error);
                }
                None => *current = Some(error),
                _ => {}
            }
        }
    }

    pub(crate) fn has_startup_error(&self) -> bool {
        self.startup_error
            .read()
            .map(|error| error.is_some())
            .unwrap_or(true)
    }

    pub(crate) fn request_sync(&self, force: bool) {
        let _ = self.sync_tx.try_send(BackgroundRequest::Sync { force });
    }

    pub(crate) fn background_enabled(&self) -> bool {
        self.settings()
            .map(|settings| settings.background_enabled)
            .unwrap_or(false)
    }

    fn start_quit_handshake(&self) -> Result<Option<ExitHandshakeTicket>, String> {
        self.start_quit_handshake_at(Instant::now(), EXIT_HANDSHAKE_TIMEOUT)
    }

    fn start_quit_handshake_at(
        &self,
        started_at: Instant,
        timeout: Duration,
    ) -> Result<Option<ExitHandshakeTicket>, String> {
        let mut state = self
            .exit_handshake
            .lock()
            .map_err(|_| "The exit coordinator is temporarily unavailable.".to_owned())?;
        if state.phase != ExitHandshakePhase::Idle {
            return Ok(None);
        }

        let request_id = state
            .last_request_id
            .checked_add(1)
            .ok_or_else(|| "The exit request counter is exhausted.".to_owned())?;
        let generation = state
            .generation
            .checked_add(1)
            .ok_or_else(|| "The exit generation counter is exhausted.".to_owned())?;
        let deadline = started_at
            .checked_add(timeout)
            .ok_or_else(|| "The exit deadline could not be scheduled.".to_owned())?;
        let ticket = ExitHandshakeTicket {
            request_id,
            generation,
            deadline,
        };
        state.last_request_id = request_id;
        state.generation = generation;
        state.phase = ExitHandshakePhase::Awaiting(ticket);
        Ok(Some(ticket))
    }

    pub(crate) fn finish_quit(&self) {
        let _ = self.shutdown_tx.send(true);
    }

    pub(crate) fn is_quitting(&self) -> bool {
        self.exit_handshake
            .lock()
            .map(|state| state.phase != ExitHandshakePhase::Idle)
            .unwrap_or(true)
    }

    pub(crate) fn is_exit_committed(&self) -> bool {
        self.exit_handshake
            .lock()
            .map(|state| matches!(state.phase, ExitHandshakePhase::Committed(_)))
            .unwrap_or(false)
    }

    fn complete_quit_handshake(&self, request_id: u64) -> Result<bool, String> {
        let mut state = self
            .exit_handshake
            .lock()
            .map_err(|_| "The exit coordinator is temporarily unavailable.".to_owned())?;
        let ExitHandshakePhase::Awaiting(ticket) = state.phase else {
            return Ok(false);
        };
        if ticket.request_id != request_id {
            return Ok(false);
        }
        state.phase = ExitHandshakePhase::Committed(ticket);
        Ok(true)
    }

    fn cancel_quit_handshake(&self, request_id: u64) -> Result<bool, String> {
        let mut state = self
            .exit_handshake
            .lock()
            .map_err(|_| "The exit coordinator is temporarily unavailable.".to_owned())?;
        let ExitHandshakePhase::Awaiting(ticket) = state.phase else {
            return Ok(false);
        };
        if ticket.request_id != request_id {
            return Ok(false);
        }
        state.phase = ExitHandshakePhase::Idle;
        Ok(true)
    }

    fn commit_quit_timeout(
        &self,
        ticket: ExitHandshakeTicket,
        now: Instant,
    ) -> Result<bool, String> {
        let mut state = self
            .exit_handshake
            .lock()
            .map_err(|_| "The exit coordinator is temporarily unavailable.".to_owned())?;
        if state.phase != ExitHandshakePhase::Awaiting(ticket) {
            return Ok(false);
        }
        if now < ticket.deadline {
            return Ok(false);
        }
        state.phase = ExitHandshakePhase::Committed(ticket);
        Ok(true)
    }

    pub(crate) async fn acquire_sync_gate(&self) -> AsyncMutexGuard<'_, ()> {
        self.sync_gate.lock().await
    }

    fn poll_duration(&self) -> Duration {
        let minutes = self
            .settings()
            .map(|settings| settings.poll_interval_minutes)
            .unwrap_or(5);
        Duration::from_secs(u64::from(minutes) * 60)
    }

    fn settings(&self) -> Result<StoredDesktopSettings, String> {
        self.settings
            .read()
            .map(|settings| *settings)
            .map_err(|_| "Desktop settings are temporarily unavailable.".to_owned())
    }

    fn update_notification_baseline(&self, uid: u32) -> Result<(), String> {
        let mut settings = self.settings()?;
        settings.notification_baseline_initialized = true;
        settings.notification_baseline_uid = uid;
        self.persist_settings(settings, "The notification baseline could not be saved.")?;
        *self
            .settings
            .write()
            .map_err(|_| "Desktop settings are temporarily unavailable.".to_owned())? = settings;
        Ok(())
    }

    pub(crate) fn reset_notification_baseline(&self) -> Result<(), String> {
        let mut settings = self.settings()?;
        settings.notification_baseline_initialized = false;
        settings.notification_baseline_uid = 0;
        // The in-memory account boundary must change even if the settings
        // database is temporarily unwritable. Otherwise a newly configured
        // account could inherit the previous account's UID baseline.
        *self
            .settings
            .write()
            .map_err(|_| "Desktop settings are temporarily unavailable.".to_owned())? = settings;
        self.persist_settings(settings, "The notification baseline could not be reset.")?;
        Ok(())
    }

    fn persist_settings(
        &self,
        settings: StoredDesktopSettings,
        safe_error: &str,
    ) -> Result<(), String> {
        if let Some(store) = self.store.as_ref() {
            store.save(settings).map_err(|_| safe_error.to_owned())?;
        }
        Ok(())
    }

    fn should_skip_automatic_sync(&self) -> Result<bool, String> {
        let last = self
            .last_sync_started
            .lock()
            .map_err(|_| "The sync coordinator is temporarily unavailable.".to_owned())?;
        Ok(last.is_some_and(|instant| instant.elapsed() < MINIMUM_AUTOMATIC_SYNC_GAP))
    }

    fn record_sync_start(&self) -> Result<(), String> {
        *self
            .last_sync_started
            .lock()
            .map_err(|_| "The sync coordinator is temporarily unavailable.".to_owned())? =
            Some(Instant::now());
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct SyncAllReport {
    pub inbox: SyncReport,
    pub drafts: DraftSyncReport,
}

#[derive(Clone, Debug, Serialize)]
struct InboxUpdatedEvent {
    report: SyncReport,
}

#[derive(Clone, Debug, Serialize)]
struct SyncErrorEvent {
    operation: &'static str,
    message: String,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BeforeExitEvent {
    request_id: u64,
    timeout_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct DraftsUpdatedEvent {
    reason: &'static str,
    report: Option<DraftSyncReport>,
}

impl DraftsUpdatedEvent {
    pub(crate) fn saved() -> Self {
        Self {
            reason: "saved",
            report: None,
        }
    }

    pub(crate) fn deleted() -> Self {
        Self {
            reason: "deleted",
            report: None,
        }
    }

    fn synced(report: DraftSyncReport) -> Self {
        Self {
            reason: "synced",
            report: Some(report),
        }
    }
}

pub(crate) fn start_background_loop(
    app: AppHandle,
    mut sync_rx: mpsc::Receiver<BackgroundRequest>,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    tauri::async_runtime::spawn(async move {
        let mut inbox_deadline =
            TokioInstant::now() + app.state::<DesktopRuntime>().poll_duration();
        let mut draft_deadline = TokioInstant::now() + DRAFT_SYNC_INTERVAL;
        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(inbox_deadline) => {
                    if let Err(error) = perform_inbox_sync(&app).await {
                        emit_sync_error(&app, "inbox", error);
                    }
                    inbox_deadline = TokioInstant::now() + app.state::<DesktopRuntime>().poll_duration();
                }
                _ = tokio::time::sleep_until(draft_deadline) => {
                    if let Err(error) = perform_draft_sync(&app).await {
                        emit_sync_error(&app, "drafts", error);
                    }
                    draft_deadline = TokioInstant::now() + DRAFT_SYNC_INTERVAL;
                }
                request = sync_rx.recv() => {
                    match request {
                        Some(BackgroundRequest::Sync { mut force }) => {
                            while let Ok(queued) = sync_rx.try_recv() {
                                if let BackgroundRequest::Sync { force: queued_force } = queued {
                                    force |= queued_force;
                                }
                            }
                            if let Err(error) = perform_sync_all(&app, force).await {
                                emit_sync_error(&app, "all", error);
                            }
                            inbox_deadline = TokioInstant::now() + app.state::<DesktopRuntime>().poll_duration();
                            draft_deadline = TokioInstant::now() + DRAFT_SYNC_INTERVAL;
                        }
                        Some(BackgroundRequest::ScheduleChanged) => {
                            inbox_deadline = TokioInstant::now() + app.state::<DesktopRuntime>().poll_duration();
                        }
                        None => break,
                    }
                }
                changed = shutdown_rx.changed() => {
                    if changed.is_err() || *shutdown_rx.borrow() {
                        break;
                    }
                }
            }
        }
    });
}

fn emit_sync_error(app: &AppHandle, operation: &'static str, message: String) {
    let _ = app.emit("mail:sync-error", SyncErrorEvent { operation, message });
}

pub(crate) async fn perform_sync_all(
    app: &AppHandle,
    force: bool,
) -> Result<Option<SyncAllReport>, String> {
    let runtime = app.state::<DesktopRuntime>();
    let _guard = runtime.sync_gate.lock().await;
    if !force && runtime.should_skip_automatic_sync()? {
        return Ok(None);
    }
    runtime.record_sync_start()?;

    // Inbox and Drafts are independent remote mailboxes. Always attempt both
    // so a temporary INBOX failure cannot starve five-minute Drafts sync (and
    // vice versa); report a partial failure only after both attempts finish.
    let inbox = sync_inbox_unlocked(app).await;
    let drafts = sync_drafts_unlocked(app).await;
    match (inbox, drafts) {
        (Ok(inbox), Ok(drafts)) => Ok(Some(SyncAllReport { inbox, drafts })),
        (Err(inbox_error), Ok(_)) => Err(format!(
            "Inbox synchronization failed, but Drafts synchronization completed: {inbox_error}"
        )),
        (Ok(_), Err(drafts_error)) => Err(format!(
            "Drafts synchronization failed, but Inbox synchronization completed: {drafts_error}"
        )),
        (Err(inbox_error), Err(drafts_error)) => Err(format!(
            "Inbox synchronization failed: {inbox_error} Drafts synchronization failed: {drafts_error}"
        )),
    }
}

pub(crate) async fn perform_inbox_sync(app: &AppHandle) -> Result<SyncReport, String> {
    let runtime = app.state::<DesktopRuntime>();
    let _guard = runtime.sync_gate.lock().await;
    runtime.record_sync_start()?;
    sync_inbox_unlocked(app).await
}

pub(crate) async fn perform_draft_sync(app: &AppHandle) -> Result<DraftSyncReport, String> {
    let runtime = app.state::<DesktopRuntime>();
    let _guard = runtime.sync_gate.lock().await;
    sync_drafts_unlocked(app).await
}

async fn sync_inbox_unlocked(app: &AppHandle) -> Result<SyncReport, String> {
    let backend = app.state::<BackendState>().network()?;
    let report = backend
        .sync_inbox(crate::INBOX_SYNC_LIMIT)
        .await
        .map_err(crate::safe_mail_error)?;

    if let Ok(messages) = backend.list_inbox(crate::INBOX_LIST_LIMIT) {
        update_notification_baseline_and_notify(app, &report, &messages);
    }
    let _ = app.emit(
        "mail:inbox-updated",
        InboxUpdatedEvent {
            report: report.clone(),
        },
    );
    Ok(report)
}

async fn sync_drafts_unlocked(app: &AppHandle) -> Result<DraftSyncReport, String> {
    let backend = app.state::<BackendState>().network()?;
    let report = backend
        .sync_drafts(None)
        .await
        .map_err(crate::safe_mail_error)?;
    let _ = app.emit(
        "mail:drafts-updated",
        DraftsUpdatedEvent::synced(report.clone()),
    );
    Ok(report)
}

fn update_notification_baseline_and_notify(
    app: &AppHandle,
    report: &SyncReport,
    messages: &[InboxMessage],
) {
    let runtime = app.state::<DesktopRuntime>();
    let Ok(settings) = runtime.settings() else {
        return;
    };
    let current_highest_uid = messages
        .iter()
        .map(|message| message.uid)
        .max()
        .unwrap_or(0);

    if !settings.notification_baseline_initialized || report.uid_validity_reset {
        let _ = runtime.update_notification_baseline(current_highest_uid);
        return;
    }

    let baseline = settings.notification_baseline_uid;
    if runtime
        .update_notification_baseline(current_highest_uid.max(baseline))
        .is_err()
    {
        return;
    }
    if !settings.notifications_enabled {
        return;
    }

    let mut new_unread: Vec<&InboxMessage> = messages
        .iter()
        .filter(|message| message.uid > baseline && !is_seen(message))
        .collect();
    new_unread.sort_by_key(|message| message.uid);
    if new_unread.is_empty() || main_window_is_active(app) || !notification_permission_granted(app)
    {
        return;
    }

    if new_unread.len() > 3 {
        let _ = app
            .notification()
            .builder()
            .title("Mine Mail")
            .body(format!("收到 {} 封新未读邮件", new_unread.len()))
            .show();
        return;
    }

    for message in new_unread {
        let sender = message
            .sender
            .as_ref()
            .map(|address| {
                address
                    .name
                    .as_deref()
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or(&address.email)
            })
            .unwrap_or("未知发件人");
        let subject = if message.subject.trim().is_empty() {
            "(无主题)"
        } else {
            &message.subject
        };
        let _ = app
            .notification()
            .builder()
            .title(sanitize_notification_text(sender, 80))
            .body(sanitize_notification_text(subject, 140))
            .show();
    }
}

fn main_window_is_active(app: &AppHandle) -> bool {
    app.get_webview_window("main").is_some_and(|window| {
        window.is_visible().unwrap_or(false) && window.is_focused().unwrap_or(false)
    })
}

fn notification_permission_granted(app: &AppHandle) -> bool {
    let notifications = app.notification();
    match notifications.permission_state() {
        Ok(PermissionState::Granted) => true,
        Ok(PermissionState::Prompt | PermissionState::PromptWithRationale) => {
            matches!(
                notifications.request_permission(),
                Ok(PermissionState::Granted)
            )
        }
        Ok(PermissionState::Denied) | Err(_) => false,
    }
}

fn is_seen(message: &InboxMessage) -> bool {
    message
        .flags
        .iter()
        .any(|flag| flag.eq_ignore_ascii_case("\\Seen"))
}

fn sanitize_notification_text(value: &str, max_characters: usize) -> String {
    let normalized: String = value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    let compact = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut characters = compact.chars();
    let truncated: String = characters.by_ref().take(max_characters).collect();
    if characters.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

pub(crate) fn build_tray(app: &App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "打开", true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", "刷新", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &refresh, &quit])?;

    let mut builder = TrayIconBuilder::new()
        .tooltip("Mine Mail")
        .menu(&menu)
        .show_menu_on_left_click(!cfg!(target_os = "windows"))
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main_window(app, true),
            "refresh" => request_sync(app, true),
            "quit" => quit_app(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle(), true);
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

pub(crate) fn show_main_window(app: &AppHandle, force_sync: bool) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    request_sync(app, force_sync);
}

pub(crate) fn request_sync(app: &AppHandle, force: bool) {
    if let Some(runtime) = app.try_state::<DesktopRuntime>() {
        runtime.request_sync(force);
    }
}

pub(crate) fn quit_app(app: &AppHandle) {
    let Some(runtime) = app.try_state::<DesktopRuntime>() else {
        app.exit(0);
        return;
    };
    let ticket = match runtime.start_quit_handshake() {
        Ok(Some(ticket)) => ticket,
        Ok(None) => return,
        Err(error) => {
            runtime.record_startup_error(error);
            return;
        }
    };

    let _ = app.emit(
        "mail:before-exit",
        BeforeExitEvent {
            request_id: ticket.request_id,
            timeout_ms: EXIT_HANDSHAKE_TIMEOUT.as_millis() as u64,
        },
    );

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(EXIT_HANDSHAKE_TIMEOUT).await;
        complete_exit_on_timeout(&app, ticket);
    });
}

pub(crate) fn complete_exit(app: &AppHandle, request_id: u64) -> Result<bool, String> {
    let Some(runtime) = app.try_state::<DesktopRuntime>() else {
        return Ok(false);
    };
    if !runtime.complete_quit_handshake(request_id)? {
        return Ok(false);
    }
    runtime.finish_quit();
    app.exit(0);
    Ok(true)
}

pub(crate) fn cancel_exit(app: &AppHandle, request_id: u64) -> Result<bool, String> {
    let Some(runtime) = app.try_state::<DesktopRuntime>() else {
        return Ok(false);
    };
    runtime.cancel_quit_handshake(request_id)
}

fn complete_exit_on_timeout(app: &AppHandle, ticket: ExitHandshakeTicket) {
    let Some(runtime) = app.try_state::<DesktopRuntime>() else {
        return;
    };
    if !matches!(
        runtime.commit_quit_timeout(ticket, Instant::now()),
        Ok(true)
    ) {
        return;
    }
    runtime.finish_quit();
    app.exit(0);
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use mine_mail::{InboxMessage, MailAddress};

    use super::{
        BeforeExitEvent, DesktopRuntime, EXIT_HANDSHAKE_TIMEOUT, is_seen,
        sanitize_notification_text,
    };

    fn message(flags: Vec<String>) -> InboxMessage {
        InboxMessage {
            id: 1,
            account_id: "primary".to_owned(),
            mailbox: "INBOX".to_owned(),
            uid: 1,
            message_id: None,
            subject: "Subject".to_owned(),
            sender: Some(MailAddress {
                name: Some("Sender".to_owned()),
                email: "sender@example.com".to_owned(),
            }),
            to: vec![],
            cc: vec![],
            sent_at: None,
            internal_date: None,
            flags,
            size_bytes: 0,
            preview: String::new(),
            body_text: None,
            body_html: None,
            attachment_names: vec![],
            body_fetched: false,
            raw_rfc822: vec![],
            synced_at: "2026-07-14T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn seen_flag_check_is_case_insensitive() {
        assert!(is_seen(&message(vec!["\\seen".to_owned()])));
        assert!(!is_seen(&message(vec!["\\Flagged".to_owned()])));
    }

    #[test]
    fn notification_text_removes_control_characters_and_is_bounded() {
        assert_eq!(
            sanitize_notification_text("Hello\n  world", 20),
            "Hello world"
        );
        assert_eq!(sanitize_notification_text("abcdef", 3), "abc…");
    }

    #[test]
    fn desktop_runtime_falls_back_when_settings_path_is_unusable() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let unusable_path = directory.path().join("not-a-directory");
        std::fs::write(&unusable_path, b"occupied").expect("create regular file");

        let (runtime, _sync_rx, _shutdown_rx) = DesktopRuntime::open(&unusable_path);
        let settings = runtime.settings_dto(false).expect("fallback settings");

        assert_eq!(settings.poll_interval_minutes, 5);
        assert!(settings.startup_error.is_some());
    }

    #[test]
    fn quit_handshake_can_be_cancelled_and_stale_requests_cannot_commit() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let (runtime, _sync_rx, _shutdown_rx) = DesktopRuntime::open(directory.path());

        let first = runtime
            .start_quit_handshake()
            .expect("start first handshake")
            .expect("first ticket");
        assert!(
            runtime
                .start_quit_handshake()
                .expect("duplicate request")
                .is_none()
        );
        assert!(runtime.is_quitting());
        assert!(!runtime.is_exit_committed());
        assert!(
            !runtime
                .cancel_quit_handshake(first.request_id + 1)
                .expect("reject wrong cancellation")
        );
        assert!(
            runtime
                .cancel_quit_handshake(first.request_id)
                .expect("cancel current request")
        );
        assert!(!runtime.is_quitting());

        let second = runtime
            .start_quit_handshake()
            .expect("start second handshake")
            .expect("second ticket");
        assert!(second.request_id > first.request_id);
        assert!(second.generation > first.generation);
        assert!(
            !runtime
                .commit_quit_timeout(first, second.deadline)
                .expect("stale timer is ignored")
        );
        assert!(
            !runtime
                .complete_quit_handshake(first.request_id)
                .expect("stale completion is ignored")
        );
        assert!(
            runtime
                .complete_quit_handshake(second.request_id)
                .expect("complete current request")
        );
        assert!(runtime.is_exit_committed());
        assert!(
            !runtime
                .cancel_quit_handshake(second.request_id)
                .expect("committed request cannot be cancelled")
        );
    }

    #[test]
    fn current_exit_timer_cannot_commit_before_its_deadline() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let (runtime, _sync_rx, _shutdown_rx) = DesktopRuntime::open(directory.path());
        let started_at = Instant::now();
        let ticket = runtime
            .start_quit_handshake_at(started_at, EXIT_HANDSHAKE_TIMEOUT)
            .expect("start handshake")
            .expect("exit ticket");
        let just_before_deadline = ticket
            .deadline
            .checked_sub(Duration::from_nanos(1))
            .expect("time before deadline");

        assert!(
            !runtime
                .commit_quit_timeout(ticket, just_before_deadline)
                .expect("early timer is rejected")
        );
        assert!(runtime.is_quitting());
        assert!(!runtime.is_exit_committed());
        assert!(
            runtime
                .commit_quit_timeout(ticket, ticket.deadline)
                .expect("timer commits at deadline")
        );
        assert!(runtime.is_exit_committed());
        assert!(
            !runtime
                .commit_quit_timeout(ticket, ticket.deadline)
                .expect("timer cannot commit twice")
        );
    }

    #[test]
    fn before_exit_payload_is_camel_case_and_allows_sqlite_timeout() {
        assert!(EXIT_HANDSHAKE_TIMEOUT >= Duration::from_secs(30));
        let payload = serde_json::to_value(BeforeExitEvent {
            request_id: 42,
            timeout_ms: EXIT_HANDSHAKE_TIMEOUT.as_millis() as u64,
        })
        .expect("serialize exit payload");

        assert_eq!(payload["requestId"], 42);
        assert_eq!(
            payload["timeoutMs"],
            EXIT_HANDSHAKE_TIMEOUT.as_millis() as u64
        );
        assert!(payload.get("request_id").is_none());
    }
}
