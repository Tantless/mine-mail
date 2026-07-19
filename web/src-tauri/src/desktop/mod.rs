mod settings;

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
    sync::{
        Mutex as StdMutex, RwLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use mine_mail::{DraftSyncReport, InboxMessage, InboxMonitorMode, SyncReport};
use serde::Serialize;
use tauri::{
    App, AppHandle, Emitter, Manager, PhysicalPosition, WebviewWindow,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tokio::sync::{Mutex as AsyncMutex, MutexGuard as AsyncMutexGuard, mpsc, watch};
use tokio::time::Instant as TokioInstant;

use crate::{
    account::{AccountRuntime, BackendState},
    diagnostics::{self, ErrorKind, Fields},
};

pub(crate) use settings::{
    DeleteProfileAvatarRequest, DesktopSettingsDto, DesktopSettingsUpdate, ProfileAvatarDto,
    SaveProfileAvatarRequest,
};
use settings::{
    DesktopSettingsStore, NotificationBaseline, StoredDesktopSettings, valid_poll_interval,
};

const MINIMUM_AUTOMATIC_SYNC_GAP: Duration = Duration::from_secs(30);
const DRAFT_SYNC_INTERVAL: Duration = Duration::from_secs(5 * 60);
const MONITOR_SUPERVISOR_INTERVAL: Duration = Duration::from_secs(2);
const IDLE_MAINTENANCE_INTERVAL: Duration = Duration::from_secs(28 * 60);
const FOREGROUND_LIGHTWEIGHT_POLL_INTERVAL: Duration = Duration::from_secs(15);
const BACKGROUND_LIGHTWEIGHT_POLL_INTERVAL: Duration = Duration::from_secs(30);
const MONITOR_RECONNECT_BACKOFF_SECONDS: [u64; 7] = [2, 5, 15, 30, 60, 120, 300];
const EXIT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
const SETTINGS_DATABASE_NAME: &str = "desktop-runtime.sqlite3";
const NEW_MAIL_NOTIFICATION_WINDOW: &str = "new-mail-notification";
const NEW_MAIL_NOTIFICATION_MARGIN: i32 = 18;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NewMailNotificationDto {
    pub notification_id: u64,
    pub sender: String,
    pub subject: String,
    pub uid: u32,
    pub account_id: String,
    pub count: usize,
    pub web_sound: Option<settings::NotificationSound>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenMessageEvent {
    uid: u32,
    account_id: String,
}

#[derive(Clone, Debug)]
pub(crate) enum BackgroundRequest {
    Sync { force: bool, trigger: &'static str },
    InboxChanged { account_id: String },
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
    pending_inbox_syncs: StdMutex<HashSet<String>>,
    exit_handshake: StdMutex<ExitHandshakeState>,
    notification_sequence: AtomicU64,
    notification_popup: StdMutex<Option<NewMailNotificationDto>>,
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
            diagnostics::error(
                "settings_store_open_failed",
                Fields::default().error(ErrorKind::Io),
            );
            (
                None,
                StoredDesktopSettings::default(),
                Some("The desktop settings directory is unavailable.".to_owned()),
            )
        } else {
            match DesktopSettingsStore::open(app_data.join(SETTINGS_DATABASE_NAME)) {
                Ok(store) => match store.load() {
                    Ok(settings) => {
                        diagnostics::info(
                            "settings_store_opened",
                            Fields::default().outcome("ready"),
                        );
                        (Some(store), settings, None)
                    }
                    Err(_) => {
                        diagnostics::error(
                            "settings_store_open_failed",
                            Fields::default().error(ErrorKind::Database),
                        );
                        (
                            None,
                            StoredDesktopSettings::default(),
                            Some(
                                "Desktop settings could not be loaded; safe in-memory defaults are active."
                                    .to_owned(),
                            ),
                        )
                    }
                },
                Err(_) => {
                    diagnostics::error(
                        "settings_store_open_failed",
                        Fields::default().error(ErrorKind::Database),
                    );
                    (
                        None,
                        StoredDesktopSettings::default(),
                        Some(
                            "Desktop settings could not be initialized; safe in-memory defaults are active."
                                .to_owned(),
                        ),
                    )
                }
            }
        };
        // Leave room for a short burst of per-account IDLE events while the
        // serialized SQLite/IMAP synchronization actor is busy.
        let (sync_tx, sync_rx) = mpsc::channel(32);
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
                pending_inbox_syncs: StdMutex::new(HashSet::new()),
                exit_handshake: StdMutex::new(ExitHandshakeState::default()),
                notification_sequence: AtomicU64::new(0),
                notification_popup: StdMutex::new(None),
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
            foreground_notifications_enabled: settings.foreground_notifications_enabled,
            notification_sound_enabled: settings.notification_sound_enabled,
            notification_sound: settings.notification_sound,
            remote_image_mode: settings.remote_image_mode,
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
        if let Some(value) = update.foreground_notifications_enabled {
            settings.foreground_notifications_enabled = value;
        }
        if let Some(value) = update.notification_sound_enabled {
            settings.notification_sound_enabled = value;
        }
        if let Some(value) = update.notification_sound {
            settings.notification_sound = value;
        }
        if let Some(value) = update.remote_image_mode {
            settings.remote_image_mode = value;
        }

        self.persist_settings(settings, "Desktop settings could not be saved.")?;
        *self
            .settings
            .write()
            .map_err(|_| "Desktop settings are temporarily unavailable.".to_owned())? = settings;
        let _ = self.sync_tx.try_send(BackgroundRequest::ScheduleChanged);
        Ok(())
    }

    pub(crate) fn list_profile_avatars(&self) -> Result<Vec<ProfileAvatarDto>, String> {
        self.store
            .as_ref()
            .ok_or_else(|| "Avatar storage is unavailable.".to_owned())?
            .list_profile_avatars()
            .map_err(|_| "Avatars could not be loaded.".to_owned())
    }

    pub(crate) fn save_profile_avatar(
        &self,
        request: SaveProfileAvatarRequest,
    ) -> Result<ProfileAvatarDto, String> {
        self.store
            .as_ref()
            .ok_or_else(|| "Avatar storage is unavailable.".to_owned())?
            .save_profile_avatar(request)
    }

    pub(crate) fn delete_profile_avatar(
        &self,
        request: DeleteProfileAvatarRequest,
    ) -> Result<(), String> {
        self.store
            .as_ref()
            .ok_or_else(|| "Avatar storage is unavailable.".to_owned())?
            .delete_profile_avatar(request)
    }

    pub(crate) fn user_settings_snapshot(&self) -> Result<DesktopSettingsUpdate, String> {
        let settings = self.settings()?;
        Ok(DesktopSettingsUpdate {
            background_enabled: Some(settings.background_enabled),
            poll_interval_minutes: Some(settings.poll_interval_minutes),
            notifications_enabled: Some(settings.notifications_enabled),
            foreground_notifications_enabled: Some(settings.foreground_notifications_enabled),
            notification_sound_enabled: Some(settings.notification_sound_enabled),
            notification_sound: Some(settings.notification_sound),
            remote_image_mode: Some(settings.remote_image_mode),
            autostart_enabled: None,
        })
    }

    pub(crate) fn latest_new_mail_notification(
        &self,
    ) -> Result<Option<NewMailNotificationDto>, String> {
        self.notification_popup
            .lock()
            .map(|notification| notification.clone())
            .map_err(|_| "The notification surface is temporarily unavailable.".to_owned())
    }

    fn publish_new_mail_notification(
        &self,
        sender: String,
        subject: String,
        uid: u32,
        account_id: String,
        count: usize,
        web_sound: Option<settings::NotificationSound>,
    ) -> Result<NewMailNotificationDto, String> {
        let notification = NewMailNotificationDto {
            notification_id: self.notification_sequence.fetch_add(1, Ordering::Relaxed) + 1,
            sender,
            subject,
            uid,
            account_id,
            count,
            web_sound,
        };
        *self
            .notification_popup
            .lock()
            .map_err(|_| "The notification surface is temporarily unavailable.".to_owned())? =
            Some(notification.clone());
        Ok(notification)
    }

    fn clear_new_mail_notification(&self, notification_id: u64) -> Result<bool, String> {
        let mut current = self
            .notification_popup
            .lock()
            .map_err(|_| "The notification surface is temporarily unavailable.".to_owned())?;
        if current
            .as_ref()
            .map(|notification| notification.notification_id)
            != Some(notification_id)
        {
            return Ok(false);
        }
        *current = None;
        Ok(true)
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

    pub(crate) fn request_sync(&self, force: bool, trigger: &'static str) {
        let _ = self
            .sync_tx
            .try_send(BackgroundRequest::Sync { force, trigger });
    }

    fn request_incremental_inbox_sync(&self, account_id: String) {
        let Ok(mut pending) = self.pending_inbox_syncs.lock() else {
            return;
        };
        if !pending.insert(account_id.clone()) {
            return;
        }
        if self
            .sync_tx
            .try_send(BackgroundRequest::InboxChanged {
                account_id: account_id.clone(),
            })
            .is_err()
        {
            pending.remove(&account_id);
        }
    }

    fn begin_incremental_inbox_sync(&self, account_id: &str) {
        if let Ok(mut pending) = self.pending_inbox_syncs.lock() {
            pending.remove(account_id);
        }
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

    fn notification_baseline(&self, account_id: &str) -> Result<NotificationBaseline, String> {
        self.store
            .as_ref()
            .map(|store| store.load_notification_baseline(account_id))
            .transpose()
            .map_err(|_| "The notification baseline could not be read.".to_owned())
            .map(|baseline| baseline.unwrap_or_default())
    }

    fn update_notification_baseline(&self, account_id: &str, uid: u32) -> Result<(), String> {
        if let Some(store) = self.store.as_ref() {
            store
                .save_notification_baseline(
                    account_id,
                    NotificationBaseline {
                        initialized: true,
                        uid,
                    },
                )
                .map_err(|_| "The notification baseline could not be saved.".to_owned())?;
        }
        Ok(())
    }

    pub(crate) fn remove_notification_baseline(&self, account_id: &str) -> Result<(), String> {
        if let Some(store) = self.store.as_ref() {
            store
                .delete_notification_baseline(account_id)
                .map_err(|_| "The notification baseline could not be removed.".to_owned())?;
        }
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
    pub accounts_synced: usize,
}

#[derive(Clone, Debug, Serialize)]
struct InboxUpdatedEvent {
    account_id: String,
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

/// Keep one capability-driven Inbox monitor per configured account. IDLE is
/// selected at runtime when advertised; other servers retain one authenticated
/// connection and perform counter-only probes instead of full synchronization.
pub(crate) fn start_inbox_monitor_supervisor(
    app: AppHandle,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    tauri::async_runtime::spawn(async move {
        let mut monitors: HashMap<String, tauri::async_runtime::JoinHandle<()>> = HashMap::new();
        loop {
            let account_ids: HashSet<String> = app
                .state::<AccountRuntime>()
                .account_ids()
                .into_iter()
                .collect();

            monitors.retain(|account_id, task| {
                let keep = account_ids.contains(account_id) && !task.inner().is_finished();
                if !keep {
                    task.abort();
                }
                keep
            });
            for account_id in account_ids {
                monitors.entry(account_id.clone()).or_insert_with(|| {
                    let app = app.clone();
                    let shutdown_rx = shutdown_rx.clone();
                    tauri::async_runtime::spawn(run_inbox_monitor(app, account_id, shutdown_rx))
                });
            }

            tokio::select! {
                _ = tokio::time::sleep(MONITOR_SUPERVISOR_INTERVAL) => {}
                changed = shutdown_rx.changed() => {
                    if changed.is_err() || *shutdown_rx.borrow() {
                        for (_, task) in monitors.drain() {
                            task.abort();
                        }
                        break;
                    }
                }
            }
        }
    });
}

async fn run_inbox_monitor(
    app: AppHandle,
    account_id: String,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    diagnostics::info(
        "inbox_monitor_started",
        Fields::default().account(&account_id),
    );
    let mut failures = 0usize;
    loop {
        if *shutdown_rx.borrow() {
            return;
        }
        let backend = match app.state::<BackendState>().network_for(&account_id) {
            Ok(backend) => {
                diagnostics::limited_recovery(
                    "inbox_monitor_failed",
                    "inbox_monitor_recovered",
                    "backend_access",
                    Some(&account_id),
                );
                backend
            }
            Err(_) => {
                diagnostics::limited_failure(
                    "inbox_monitor_failed",
                    "backend_access",
                    Some(&account_id),
                    ErrorKind::Runtime,
                );
                if wait_for_monitor_retry(&mut shutdown_rx, failures).await {
                    return;
                }
                failures = failures.saturating_add(1);
                continue;
            }
        };
        let mut monitor = match backend.connect_inbox_monitor().await {
            Ok(monitor) => {
                diagnostics::limited_recovery(
                    "inbox_monitor_failed",
                    "inbox_monitor_recovered",
                    "monitor_connect",
                    Some(&account_id),
                );
                diagnostics::limited_recovery(
                    "inbox_monitor_failed",
                    "inbox_monitor_recovered",
                    "monitor_session",
                    Some(&account_id),
                );
                failures = 0;
                monitor
            }
            Err(error) => {
                diagnostics::limited_failure(
                    "inbox_monitor_failed",
                    "monitor_connect",
                    Some(&account_id),
                    diagnostics::mail_error_kind(&error),
                );
                if wait_for_monitor_retry(&mut shutdown_rx, failures).await {
                    return;
                }
                failures = failures.saturating_add(1);
                continue;
            }
        };

        let monitor_mode = monitor.mode();
        diagnostics::info(
            "inbox_monitor_connected",
            Fields::default()
                .account(&account_id)
                .mode(match monitor_mode {
                    InboxMonitorMode::Idle => "idle",
                    InboxMonitorMode::LightweightPoll => "lightweight_poll",
                }),
        );
        let result = match monitor_mode {
            InboxMonitorMode::Idle => loop {
                let changed = tokio::select! {
                    result = monitor.wait_for_idle_change(IDLE_MAINTENANCE_INTERVAL) => result,
                    shutdown = shutdown_rx.changed() => {
                        if shutdown.is_err() || *shutdown_rx.borrow() {
                            return;
                        }
                        continue;
                    }
                };
                match changed {
                    Ok(true) => app
                        .state::<DesktopRuntime>()
                        .request_incremental_inbox_sync(account_id.clone()),
                    // Reconnect before the RFC 2177 29-minute ceiling. This
                    // also picks up a refreshed OAuth backend instance.
                    Ok(false) => break Ok(()),
                    Err(error) => break Err(error),
                }
            },
            InboxMonitorMode::LightweightPoll => loop {
                let delay = lightweight_poll_interval(&app);
                tokio::select! {
                    _ = tokio::time::sleep(delay) => {}
                    shutdown = shutdown_rx.changed() => {
                        if shutdown.is_err() || *shutdown_rx.borrow() {
                            return;
                        }
                        continue;
                    }
                }
                match monitor.poll_for_change().await {
                    Ok(true) => app
                        .state::<DesktopRuntime>()
                        .request_incremental_inbox_sync(account_id.clone()),
                    Ok(false) => {}
                    Err(error) => break Err(error),
                }
            },
        };

        if let Err(error) = result {
            diagnostics::limited_failure(
                "inbox_monitor_failed",
                "monitor_session",
                Some(&account_id),
                diagnostics::mail_error_kind(&error),
            );
            if wait_for_monitor_retry(&mut shutdown_rx, failures).await {
                return;
            }
            failures = failures.saturating_add(1);
        }
    }
}

fn lightweight_poll_interval(app: &AppHandle) -> Duration {
    let visible = app
        .get_webview_window("main")
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false);
    if visible {
        FOREGROUND_LIGHTWEIGHT_POLL_INTERVAL
    } else {
        BACKGROUND_LIGHTWEIGHT_POLL_INTERVAL
    }
}

async fn wait_for_monitor_retry(shutdown_rx: &mut watch::Receiver<bool>, failures: usize) -> bool {
    let seconds = MONITOR_RECONNECT_BACKOFF_SECONDS
        [failures.min(MONITOR_RECONNECT_BACKOFF_SECONDS.len().saturating_sub(1))];
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_secs(seconds)) => false,
        changed = shutdown_rx.changed() => changed.is_err() || *shutdown_rx.borrow(),
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
                    if let Err(error) = perform_inbox_reconciliation_all(&app).await {
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
                        Some(BackgroundRequest::Sync { force, trigger }) => {
                            if let Err(error) = perform_sync_all(&app, force, trigger).await {
                                emit_sync_error(&app, "all", error);
                            }
                            inbox_deadline = TokioInstant::now() + app.state::<DesktopRuntime>().poll_duration();
                            draft_deadline = TokioInstant::now() + DRAFT_SYNC_INTERVAL;
                        }
                        Some(BackgroundRequest::ScheduleChanged) => {
                            inbox_deadline = TokioInstant::now() + app.state::<DesktopRuntime>().poll_duration();
                        }
                        Some(BackgroundRequest::InboxChanged { account_id }) => {
                            app.state::<DesktopRuntime>().begin_incremental_inbox_sync(&account_id);
                            if let Err(error) = perform_incremental_inbox_sync(&app, &account_id).await {
                                emit_sync_error(&app, "inbox", error);
                            }
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
    trigger: &'static str,
) -> Result<Option<SyncAllReport>, String> {
    let started = Instant::now();
    let runtime = app.state::<DesktopRuntime>();
    let _guard = runtime.sync_gate.lock().await;
    if !force && runtime.should_skip_automatic_sync()? {
        return Ok(None);
    }
    let operation_id = diagnostics::operation_id();
    diagnostics::info(
        "sync_started",
        Fields::default()
            .operation_id(operation_id.clone())
            .operation("all")
            .trigger(trigger)
            .force(force),
    );
    runtime.record_sync_start()?;

    let account_runtime = app.state::<AccountRuntime>();
    let backend_state = app.state::<BackendState>();
    // Refresh every Google token that is near expiry before opening IMAP or
    // SMTP. One failed refresh must not prevent the other configured accounts
    // from synchronizing.
    let refresh_error = account_runtime
        .refresh_oauth_backends(&backend_state)
        .await
        .err();
    let account_ids = account_runtime.account_ids();
    if account_ids.is_empty() {
        diagnostics::warn(
            "sync_skipped",
            Fields::default()
                .operation_id(operation_id)
                .operation("all")
                .trigger(trigger)
                .outcome("no_accounts")
                .error(ErrorKind::Config)
                .duration(started.elapsed()),
        );
        return Err("No mail account is configured.".to_owned());
    }
    let account_count = account_ids.len();
    let active_account_id = backend_state.active_account_id();
    let mut active_inbox = None;
    let mut active_drafts = None;
    let mut accounts_synced = 0;
    let mut errors = Vec::new();

    for account_id in account_ids {
        let inbox = sync_inbox_for(app, &account_id).await;
        let drafts = sync_drafts_for(app, &account_id).await;
        let is_active = active_account_id.as_deref() == Some(account_id.as_str());
        match inbox {
            Ok(report) => {
                if is_active {
                    active_inbox = Some(report);
                }
            }
            Err(error) => errors.push(format!("{account_id} Inbox: {error}")),
        }
        match drafts {
            Ok(report) => {
                if is_active {
                    active_drafts = Some(report);
                }
            }
            Err(error) => errors.push(format!("{account_id} Drafts: {error}")),
        }
        if !errors.iter().any(|error| error.starts_with(&account_id)) {
            accounts_synced += 1;
        }
    }
    if let Some(error) = refresh_error {
        errors.push(error);
    }
    if !errors.is_empty() {
        diagnostics::limited_failure("sync_failed", "sync_all", None, ErrorKind::Runtime);
        diagnostics::error(
            "sync_completed",
            Fields::default()
                .operation_id(operation_id)
                .operation("all")
                .trigger(trigger)
                .outcome("failed")
                .accounts(account_count)
                .successes(accounts_synced)
                .failures(errors.len())
                .duration(started.elapsed()),
        );
        return Err(format!(
            "Some account synchronization failed: {}",
            errors.join(" ")
        ));
    }
    diagnostics::limited_recovery("sync_failed", "sync_recovered", "sync_all", None);
    diagnostics::info(
        "sync_completed",
        Fields::default()
            .operation_id(operation_id)
            .operation("all")
            .trigger(trigger)
            .outcome("completed")
            .accounts(account_count)
            .successes(accounts_synced)
            .duration(started.elapsed()),
    );
    Ok(Some(SyncAllReport {
        inbox: active_inbox.ok_or_else(|| "The active Inbox was not synchronized.".to_owned())?,
        drafts: active_drafts
            .ok_or_else(|| "The active Drafts mailbox was not synchronized.".to_owned())?,
        accounts_synced,
    }))
}

async fn perform_inbox_reconciliation_all(app: &AppHandle) -> Result<(), String> {
    let runtime = app.state::<DesktopRuntime>();
    let _guard = runtime.sync_gate.lock().await;
    runtime.record_sync_start()?;
    let account_runtime = app.state::<AccountRuntime>();
    let backend_state = app.state::<BackendState>();
    let refresh_error = account_runtime
        .refresh_oauth_backends(&backend_state)
        .await
        .err();
    let mut errors = Vec::new();
    for account_id in account_runtime.account_ids() {
        if let Err(error) = sync_inbox_for(app, &account_id).await {
            errors.push(format!("{account_id} Inbox: {error}"));
        }
    }
    if let Some(error) = refresh_error {
        errors.push(error);
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Some account Inbox reconciliation failed: {}",
            errors.join(" ")
        ))
    }
}

async fn perform_incremental_inbox_sync(
    app: &AppHandle,
    account_id: &str,
) -> Result<SyncReport, String> {
    let runtime = app.state::<DesktopRuntime>();
    let _guard = runtime.sync_gate.lock().await;
    let account_runtime = app.state::<AccountRuntime>();
    let backend_state = app.state::<BackendState>();
    // Usually a no-op; it ensures a monitor event near OAuth expiry uses the
    // refreshed backend before opening the short-lived incremental session.
    let _ = account_runtime.refresh_oauth_backends(&backend_state).await;
    sync_new_inbox_for(app, account_id).await
}

pub(crate) async fn perform_inbox_sync(app: &AppHandle) -> Result<SyncReport, String> {
    let runtime = app.state::<DesktopRuntime>();
    let _guard = runtime.sync_gate.lock().await;
    runtime.record_sync_start()?;
    let account_runtime = app.state::<AccountRuntime>();
    let backend_state = app.state::<BackendState>();
    account_runtime
        .refresh_active_oauth_backend(&backend_state)
        .await?;
    let account_id = backend_state
        .active_account_id()
        .ok_or_else(|| "No mail account is selected.".to_owned())?;
    sync_inbox_for(app, &account_id).await
}

pub(crate) async fn perform_draft_sync(app: &AppHandle) -> Result<DraftSyncReport, String> {
    let runtime = app.state::<DesktopRuntime>();
    let _guard = runtime.sync_gate.lock().await;
    let account_runtime = app.state::<AccountRuntime>();
    let backend_state = app.state::<BackendState>();
    account_runtime
        .refresh_active_oauth_backend(&backend_state)
        .await?;
    let account_id = backend_state
        .active_account_id()
        .ok_or_else(|| "No mail account is selected.".to_owned())?;
    sync_drafts_for(app, &account_id).await
}

async fn sync_inbox_for(app: &AppHandle, account_id: &str) -> Result<SyncReport, String> {
    sync_inbox_with_operation(app, account_id, false).await
}

async fn sync_inbox_with_operation(
    app: &AppHandle,
    account_id: &str,
    incremental: bool,
) -> Result<SyncReport, String> {
    let started = Instant::now();
    let operation = if incremental {
        "inbox_incremental"
    } else {
        "inbox_reconciliation"
    };
    let backend = match app.state::<BackendState>().network_for(account_id) {
        Ok(backend) => backend,
        Err(error) => {
            diagnostics::limited_failure(
                "account_sync_failed",
                operation,
                Some(account_id),
                ErrorKind::Runtime,
            );
            return Err(error);
        }
    };
    let report = match if incremental {
        backend.sync_new_inbox(crate::INBOX_SYNC_LIMIT).await
    } else {
        backend.sync_inbox(crate::INBOX_SYNC_LIMIT).await
    } {
        Ok(report) => report,
        Err(error) => {
            diagnostics::limited_failure(
                "account_sync_failed",
                operation,
                Some(account_id),
                diagnostics::mail_error_kind(&error),
            );
            return Err(crate::safe_mail_error(error));
        }
    };

    let report = finish_inbox_sync(app, account_id, backend, report)?;
    diagnostics::limited_recovery(
        "account_sync_failed",
        "account_sync_recovered",
        operation,
        Some(account_id),
    );
    diagnostics::info(
        "account_sync_completed",
        Fields::default()
            .account(account_id)
            .operation(operation)
            .outcome("completed")
            .duration(started.elapsed())
            .inbox_counts(report.fetched, report.updated_flags, report.removed),
    );
    Ok(report)
}

async fn sync_new_inbox_for(app: &AppHandle, account_id: &str) -> Result<SyncReport, String> {
    sync_inbox_with_operation(app, account_id, true).await
}

fn finish_inbox_sync(
    app: &AppHandle,
    account_id: &str,
    backend: std::sync::Arc<mine_mail::MailBackend>,
    report: SyncReport,
) -> Result<SyncReport, String> {
    if let Ok(messages) = backend.list_inbox(crate::INBOX_LIST_LIMIT) {
        update_notification_baseline_and_notify(app, account_id, &report, &messages);
    }
    let _ = app.emit(
        "mail:inbox-updated",
        InboxUpdatedEvent {
            account_id: account_id.to_owned(),
            report: report.clone(),
        },
    );
    let prefetch_backend = backend.clone();
    let prefetch_app = app.clone();
    let prefetch_report = report.clone();
    let prefetch_account_id = account_id.to_owned();
    tauri::async_runtime::spawn(async move {
        if let Ok(prefetched) = prefetch_backend
            .prefetch_inbox_bodies(
                crate::INBOX_PREFETCH_LIMIT,
                crate::INBOX_PREFETCH_TOTAL_BYTES,
                crate::INBOX_PREFETCH_MESSAGE_BYTES,
            )
            .await
            && prefetched > 0
        {
            let _ = prefetch_app.emit(
                "mail:inbox-updated",
                InboxUpdatedEvent {
                    account_id: prefetch_account_id,
                    report: prefetch_report,
                },
            );
        }
    });
    Ok(report)
}

async fn sync_drafts_for(app: &AppHandle, account_id: &str) -> Result<DraftSyncReport, String> {
    let started = Instant::now();
    let backend = match app.state::<BackendState>().network_for(account_id) {
        Ok(backend) => backend,
        Err(error) => {
            diagnostics::limited_failure(
                "account_sync_failed",
                "drafts",
                Some(account_id),
                ErrorKind::Runtime,
            );
            return Err(error);
        }
    };
    let report = match backend.sync_drafts(None).await {
        Ok(report) => report,
        Err(error) => {
            diagnostics::limited_failure(
                "account_sync_failed",
                "drafts",
                Some(account_id),
                diagnostics::mail_error_kind(&error),
            );
            return Err(crate::safe_mail_error(error));
        }
    };
    let _ = app.emit(
        "mail:drafts-updated",
        DraftsUpdatedEvent::synced(report.clone()),
    );
    diagnostics::limited_recovery(
        "account_sync_failed",
        "account_sync_recovered",
        "drafts",
        Some(account_id),
    );
    diagnostics::info(
        "account_sync_completed",
        Fields::default()
            .account(account_id)
            .operation("drafts")
            .outcome("completed")
            .duration(started.elapsed())
            .conflicts(report.conflicts),
    );
    Ok(report)
}

fn update_notification_baseline_and_notify(
    app: &AppHandle,
    account_id: &str,
    report: &SyncReport,
    messages: &[InboxMessage],
) {
    let runtime = app.state::<DesktopRuntime>();
    let (Ok(settings), Ok(baseline)) = (
        runtime.settings(),
        runtime.notification_baseline(account_id),
    ) else {
        return;
    };
    let current_highest_uid = messages
        .iter()
        .map(|message| message.uid)
        .max()
        .unwrap_or(0);

    if !baseline.initialized || report.uid_validity_reset {
        let _ = runtime.update_notification_baseline(account_id, current_highest_uid);
        return;
    }

    let baseline = baseline.uid;
    if runtime
        .update_notification_baseline(account_id, current_highest_uid.max(baseline))
        .is_err()
    {
        return;
    }
    let mut new_unread: Vec<&InboxMessage> = messages
        .iter()
        .filter(|message| message.uid > baseline && !is_seen(message))
        .collect();
    new_unread.sort_by_key(|message| message.uid);
    if new_unread.is_empty()
        || !should_deliver_new_mail_notification(settings, main_window_is_active(app))
    {
        return;
    }

    let newest = new_unread
        .last()
        .expect("new_unread is known to contain at least one message");
    let newest_sender = notification_sender(newest);
    let newest_subject = notification_subject(newest);
    let count = new_unread.len();
    let (sender, subject) = if count == 1 {
        (newest_sender, newest_subject)
    } else {
        (
            format!("收到 {count} 封新邮件"),
            format!("最新：{newest_sender} · {newest_subject}"),
        )
    };
    show_new_mail_notification(
        app,
        sender,
        subject,
        newest.uid,
        account_id.to_owned(),
        count,
        settings,
    );
}

fn should_deliver_new_mail_notification(
    settings: StoredDesktopSettings,
    main_window_is_active: bool,
) -> bool {
    settings.notifications_enabled
        && (!main_window_is_active || settings.foreground_notifications_enabled)
}

fn show_new_mail_notification(
    app: &AppHandle,
    sender: String,
    subject: String,
    uid: u32,
    account_id: String,
    count: usize,
    settings: StoredDesktopSettings,
) {
    let runtime = app.state::<DesktopRuntime>();
    let web_sound = if settings.notification_sound_enabled {
        web_sound(settings.notification_sound)
    } else {
        None
    };
    let Ok(notification) = runtime.publish_new_mail_notification(
        sanitize_notification_text(&sender, 80),
        sanitize_notification_text(&subject, 140),
        uid,
        account_id,
        count,
        web_sound,
    ) else {
        return;
    };

    if settings.notification_sound_enabled {
        play_native_notification_sound(settings.notification_sound);
    }

    if let Some(window) = app.get_webview_window(NEW_MAIL_NOTIFICATION_WINDOW) {
        position_notification_window(app, &window);
        let _ = window.show();
        let _ = app.emit_to(
            NEW_MAIL_NOTIFICATION_WINDOW,
            "mail:new-mail-notification",
            notification,
        );
    }
}

fn notification_sender(message: &InboxMessage) -> String {
    message
        .sender
        .as_ref()
        .map(|address| {
            address
                .name
                .as_deref()
                .filter(|name| !name.trim().is_empty())
                .unwrap_or(&address.email)
                .to_owned()
        })
        .unwrap_or_else(|| "未知发件人".to_owned())
}

fn notification_subject(message: &InboxMessage) -> String {
    if message.subject.trim().is_empty() {
        "(无主题)".to_owned()
    } else {
        message.subject.clone()
    }
}

fn position_notification_window(app: &AppHandle, window: &WebviewWindow) {
    let monitor = app
        .get_webview_window("main")
        .and_then(|main| main.current_monitor().ok().flatten())
        .or_else(|| window.current_monitor().ok().flatten())
        .or_else(|| window.primary_monitor().ok().flatten());
    let (Some(monitor), Ok(window_size)) = (monitor, window.outer_size()) else {
        return;
    };
    let work_area = monitor.work_area();
    let x = work_area.position.x + work_area.size.width as i32
        - window_size.width as i32
        - NEW_MAIL_NOTIFICATION_MARGIN;
    let y = work_area.position.y + work_area.size.height as i32
        - window_size.height as i32
        - NEW_MAIL_NOTIFICATION_MARGIN;
    let _ = window.set_position(PhysicalPosition::new(
        x.max(work_area.position.x),
        y.max(work_area.position.y),
    ));
}

#[cfg(target_os = "windows")]
fn web_sound(_sound: settings::NotificationSound) -> Option<settings::NotificationSound> {
    None
}

#[cfg(not(target_os = "windows"))]
fn web_sound(sound: settings::NotificationSound) -> Option<settings::NotificationSound> {
    Some(sound)
}

#[cfg(target_os = "windows")]
fn play_native_notification_sound(sound: settings::NotificationSound) {
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_NODEFAULT};

    let alias: Vec<u16> = sound
        .system_resource_name()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `alias` is a NUL-terminated UTF-16 string that remains alive for
    // the duration of the call. SND_ASYNC makes winmm retain its own copy.
    unsafe {
        PlaySoundW(
            alias.as_ptr(),
            std::ptr::null_mut(),
            SND_ALIAS | SND_ASYNC | SND_NODEFAULT,
        );
    }
}

#[cfg(not(target_os = "windows"))]
fn play_native_notification_sound(_sound: settings::NotificationSound) {}

fn main_window_is_active(app: &AppHandle) -> bool {
    app.get_webview_window("main").is_some_and(|window| {
        window.is_visible().unwrap_or(false) && window.is_focused().unwrap_or(false)
    })
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
            "refresh" => request_sync(app, true, "tray"),
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
    request_sync(app, force_sync, "window_open");
}

pub(crate) fn dismiss_new_mail_notification(
    app: &AppHandle,
    notification_id: u64,
) -> Result<bool, String> {
    let runtime = app.state::<DesktopRuntime>();
    if !runtime.clear_new_mail_notification(notification_id)? {
        return Ok(false);
    }
    if let Some(window) = app.get_webview_window(NEW_MAIL_NOTIFICATION_WINDOW) {
        window
            .hide()
            .map_err(|_| "The notification window could not be hidden.".to_owned())?;
    }
    Ok(true)
}

pub(crate) fn open_new_mail_notification(
    app: &AppHandle,
    notification_id: u64,
    uid: u32,
    account_id: String,
) -> Result<bool, String> {
    let runtime = app.state::<DesktopRuntime>();
    if !runtime.clear_new_mail_notification(notification_id)? {
        return Ok(false);
    }
    if let Some(window) = app.get_webview_window(NEW_MAIL_NOTIFICATION_WINDOW) {
        let _ = window.hide();
    }
    show_main_window(app, false);
    app.emit_to(
        "main",
        "mail:open-message",
        OpenMessageEvent { uid, account_id },
    )
    .map_err(|_| "The selected message could not be opened.".to_owned())?;
    Ok(true)
}

pub(crate) fn request_sync(app: &AppHandle, force: bool, trigger: &'static str) {
    if let Some(runtime) = app.try_state::<DesktopRuntime>() {
        runtime.request_sync(force, trigger);
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
    diagnostics::info(
        "shutdown_handshake_started",
        Fields::default().operation("app_exit"),
    );

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
    diagnostics::info(
        "shutdown_committed",
        Fields::default()
            .operation("app_exit")
            .outcome("frontend_ready"),
    );
    app.exit(0);
    Ok(true)
}

pub(crate) fn cancel_exit(app: &AppHandle, request_id: u64) -> Result<bool, String> {
    let Some(runtime) = app.try_state::<DesktopRuntime>() else {
        return Ok(false);
    };
    let cancelled = runtime.cancel_quit_handshake(request_id)?;
    if cancelled {
        diagnostics::info(
            "shutdown_cancelled",
            Fields::default().operation("app_exit"),
        );
    }
    Ok(cancelled)
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
    diagnostics::warn(
        "shutdown_committed",
        Fields::default()
            .operation("app_exit")
            .outcome("frontend_timeout"),
    );
    app.exit(0);
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use mine_mail::{InboxMessage, MailAddress};

    use super::settings::NotificationSound;
    use super::settings::StoredDesktopSettings;
    use super::{
        BeforeExitEvent, DesktopRuntime, EXIT_HANDSHAKE_TIMEOUT, is_seen,
        sanitize_notification_text, should_deliver_new_mail_notification,
    };

    fn message(flags: Vec<String>) -> InboxMessage {
        InboxMessage {
            id: 1,
            account_id: "primary".to_owned(),
            mailbox: "INBOX".to_owned(),
            uid: 1,
            message_id: None,
            in_reply_to: Vec::new(),
            references: Vec::new(),
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
    fn foreground_notifications_can_be_disabled_without_silencing_background_mail() {
        let mut settings = StoredDesktopSettings::default();
        assert!(should_deliver_new_mail_notification(settings, false));
        assert!(should_deliver_new_mail_notification(settings, true));

        settings.foreground_notifications_enabled = false;
        assert!(should_deliver_new_mail_notification(settings, false));
        assert!(!should_deliver_new_mail_notification(settings, true));

        settings.notifications_enabled = false;
        assert!(!should_deliver_new_mail_notification(settings, false));
    }

    #[test]
    fn notification_surface_rejects_stale_dismissals() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let (runtime, _sync_rx, _shutdown_rx) = DesktopRuntime::open(directory.path());
        let notification = runtime
            .publish_new_mail_notification(
                "Sender".to_owned(),
                "Subject".to_owned(),
                42,
                "account-test".to_owned(),
                1,
                Some(NotificationSound::Mail),
            )
            .expect("publish notification");

        assert!(
            !runtime
                .clear_new_mail_notification(notification.notification_id + 1)
                .unwrap()
        );
        assert_eq!(
            runtime
                .latest_new_mail_notification()
                .expect("notification state")
                .expect("pending notification")
                .uid,
            42,
        );
        assert!(
            runtime
                .clear_new_mail_notification(notification.notification_id)
                .unwrap()
        );
        assert!(runtime.latest_new_mail_notification().unwrap().is_none());
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
