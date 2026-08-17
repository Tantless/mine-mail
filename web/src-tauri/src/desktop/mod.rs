mod appearance;
mod settings;

use std::{
    collections::{HashMap, HashSet},
    fs,
    future::Future,
    path::Path,
    sync::{
        Arc, Mutex as StdMutex, RwLock,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use mine_mail::{
    DraftSyncReport, InboxMessage, InboxMonitorMode, MailBackend, MailboxCapability,
    MailboxCapabilityStatus, MailboxRole, SyncBatchProgress, SyncReport,
};
use serde::Serialize;
use tauri::{
    App, AppHandle, Emitter, Manager, PhysicalPosition, WebviewWindow,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tokio::sync::{
    Mutex as AsyncMutex, MutexGuard as AsyncMutexGuard, Notify, RwLock as AsyncRwLock,
    RwLockReadGuard, RwLockWriteGuard, mpsc, watch,
};
use tokio::time::Instant as TokioInstant;

use crate::{
    account::{AccountRuntime, BackendState},
    contacts::ContactRuntime,
    diagnostics::{self, ErrorKind, Fields},
};

use appearance::AppearanceStore;
pub(crate) use appearance::{
    AppearanceSettingsDto, DeleteCustomThemeRequest, ImportCustomThemeRequest,
    SelectAppearanceThemeRequest, UpdateCustomThemeRequest,
};
pub(crate) use settings::{
    DeleteProfileAvatarRequest, DesktopSettingsDto, DesktopSettingsUpdate, MCP_ENDPOINT,
    ProfileAvatarDto, SaveProfileAvatarRequest,
};
use settings::{
    DesktopSettingsStore, NotificationBaseline, NotificationDelivery, ProfileAvatarOwnerType,
    StoredDesktopSettings, valid_poll_interval,
};

const DRAFT_SYNC_INTERVAL: Duration = Duration::from_secs(5 * 60);
const MONITOR_SUPERVISOR_INTERVAL: Duration = Duration::from_secs(2);
const IDLE_MAINTENANCE_INTERVAL: Duration = Duration::from_secs(28 * 60);
const FOREGROUND_LIGHTWEIGHT_POLL_INTERVAL: Duration = Duration::from_secs(15);
const BACKGROUND_LIGHTWEIGHT_POLL_INTERVAL: Duration = Duration::from_secs(30);
const MONITOR_RECONNECT_BACKOFF_SECONDS: [u64; 7] = [2, 5, 15, 30, 60, 120, 300];
const EXIT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(35);
const SETTINGS_DATABASE_NAME: &str = "desktop-runtime.sqlite3";
const NEW_MAIL_NOTIFICATION_WINDOW: &str = "new-mail-notification";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct McpAccess {
    pub enabled: bool,
    pub information: bool,
    pub send: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NewMailNotificationDto {
    pub notification_id: u64,
    pub sender: String,
    pub sender_email: String,
    pub sender_remark: Option<String>,
    pub sender_avatar_data_url: Option<String>,
    pub subject: String,
    pub recipient_email: String,
    pub recipient_remark: Option<String>,
    pub count: usize,
    pub web_sound: Option<settings::NotificationSound>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenMessageEvent {
    message_id: String,
    account_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NewMailNotificationTarget {
    notification_id: u64,
    account_id: String,
    public_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WindowsNotificationContent {
    title: String,
    subject: String,
    context: String,
}

#[derive(Clone, Debug)]
struct PendingNewMailNotification {
    display: NewMailNotificationDto,
    target: NewMailNotificationTarget,
}

#[derive(Clone, Debug)]
pub(crate) enum BackgroundRequest {
    Sync {
        force: bool,
        trigger: &'static str,
    },
    InboxChanged {
        account_id: String,
        trigger: &'static str,
    },
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

#[derive(Default)]
struct InboxSyncFlightState {
    running: bool,
    generation: u64,
    result: Option<Result<SyncReport, String>>,
}

#[derive(Default)]
struct InboxSyncFlight {
    state: StdMutex<InboxSyncFlightState>,
    settled: Notify,
}

struct InboxSyncLeader {
    flight: Arc<InboxSyncFlight>,
    settled: bool,
}

impl InboxSyncLeader {
    fn settle(&mut self, result: Result<SyncReport, String>) {
        if let Ok(mut state) = self.flight.state.lock() {
            state.running = false;
            state.generation = state.generation.wrapping_add(1);
            state.result = Some(result);
            self.settled = true;
        }
        self.flight.settled.notify_waiters();
    }
}

impl Drop for InboxSyncLeader {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        if let Ok(mut state) = self.flight.state.lock() {
            state.running = false;
            state.generation = state.generation.wrapping_add(1);
            state.result = Some(Err(
                "Inbox synchronization was interrupted and can be retried.".to_owned(),
            ));
        }
        self.flight.settled.notify_waiters();
    }
}

pub(crate) struct SmtpOperationGuard<'a> {
    runtime: &'a DesktopRuntime,
}

impl Drop for SmtpOperationGuard<'_> {
    fn drop(&mut self) {
        if self
            .runtime
            .smtp_operations_in_flight
            .fetch_sub(1, Ordering::AcqRel)
            == 1
        {
            self.runtime.smtp_idle.notify_waiters();
        }
    }
}

pub(crate) struct DesktopRuntime {
    settings: RwLock<StoredDesktopSettings>,
    store: Option<DesktopSettingsStore>,
    appearance_store: Option<AppearanceStore>,
    startup_error: RwLock<Option<String>>,
    sync_tx: mpsc::Sender<BackgroundRequest>,
    shutdown_tx: watch::Sender<bool>,
    account_mutation_gate: AsyncMutex<()>,
    lifecycle_gate: AsyncRwLock<()>,
    batch_sync_gate: AsyncMutex<()>,
    last_sync_started: StdMutex<Option<Instant>>,
    pending_inbox_syncs: StdMutex<HashSet<String>>,
    inbox_sync_flights: StdMutex<HashMap<String, Arc<InboxSyncFlight>>>,
    pending_notification_baselines: StdMutex<HashSet<String>>,
    exit_handshake: StdMutex<ExitHandshakeState>,
    smtp_operations_in_flight: AtomicUsize,
    smtp_idle: Notify,
    notification_sequence: AtomicU64,
    notification_popup: StdMutex<Option<PendingNewMailNotification>>,
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
        let appearance_store = AppearanceStore::open(app_data.join(SETTINGS_DATABASE_NAME))
            .map_err(|_| {
                diagnostics::error(
                    "appearance_store_open_failed",
                    Fields::default().error(ErrorKind::Database),
                );
            })
            .ok();
        // Leave room for a short burst of per-account IDLE events while the
        // serialized SQLite/IMAP synchronization actor is busy.
        let (sync_tx, sync_rx) = mpsc::channel(32);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        (
            Self {
                settings: RwLock::new(settings),
                store,
                appearance_store,
                startup_error: RwLock::new(startup_error),
                sync_tx,
                shutdown_tx,
                account_mutation_gate: AsyncMutex::new(()),
                lifecycle_gate: AsyncRwLock::new(()),
                batch_sync_gate: AsyncMutex::new(()),
                last_sync_started: StdMutex::new(None),
                pending_inbox_syncs: StdMutex::new(HashSet::new()),
                inbox_sync_flights: StdMutex::new(HashMap::new()),
                pending_notification_baselines: StdMutex::new(HashSet::new()),
                exit_handshake: StdMutex::new(ExitHandshakeState::default()),
                smtp_operations_in_flight: AtomicUsize::new(0),
                smtp_idle: Notify::new(),
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
            notification_delivery: settings.notification_delivery,
            windows_notifications_available: cfg!(target_os = "windows"),
            notification_sound_enabled: settings.notification_sound_enabled,
            notification_sound: settings.notification_sound,
            remote_image_mode: settings.remote_image_mode,
            ai_assistant_default_open: settings.ai_assistant_default_open,
            idle_poetry_enabled: settings.idle_poetry_enabled,
            mcp_enabled: settings.mcp_enabled,
            mcp_information_enabled: settings.mcp_information_enabled,
            mcp_send_enabled: settings.mcp_send_enabled,
            mcp_endpoint: MCP_ENDPOINT,
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
        if let Some(value) = update.notification_delivery {
            settings.notification_delivery = value;
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
        if let Some(value) = update.ai_assistant_default_open {
            settings.ai_assistant_default_open = value;
        }
        if let Some(value) = update.idle_poetry_enabled {
            settings.idle_poetry_enabled = value;
        }
        if let Some(value) = update.mcp_enabled {
            settings.mcp_enabled = value;
        }
        if let Some(value) = update.mcp_information_enabled {
            settings.mcp_information_enabled = value;
        }
        if let Some(value) = update.mcp_send_enabled {
            settings.mcp_send_enabled = value;
        }

        self.persist_settings(settings, "Desktop settings could not be saved.")?;
        *self
            .settings
            .write()
            .map_err(|_| "Desktop settings are temporarily unavailable.".to_owned())? = settings;
        match self.sync_tx.try_send(BackgroundRequest::ScheduleChanged) {
            Ok(()) => diagnostics::limited_recovery(
                "background_request_enqueue_failed",
                "background_request_enqueue_recovered",
                "schedule_change",
                None,
            ),
            Err(_) => diagnostics::limited_failure(
                "background_request_enqueue_failed",
                "schedule_change",
                None,
                ErrorKind::Runtime,
            ),
        }
        Ok(())
    }

    pub(crate) fn list_profile_avatars(&self) -> Result<Vec<ProfileAvatarDto>, String> {
        self.store
            .as_ref()
            .ok_or_else(|| "Avatar storage is unavailable.".to_owned())?
            .list_profile_avatars()
            .map_err(|_| "Avatars could not be loaded.".to_owned())
    }

    pub(crate) fn appearance_settings(&self) -> Result<AppearanceSettingsDto, String> {
        self.appearance_store
            .as_ref()
            .ok_or_else(|| "Appearance storage is unavailable.".to_owned())?
            .load()
    }

    pub(crate) fn select_appearance_theme(
        &self,
        request: SelectAppearanceThemeRequest,
    ) -> Result<AppearanceSettingsDto, String> {
        self.appearance_store
            .as_ref()
            .ok_or_else(|| "Appearance storage is unavailable.".to_owned())?
            .select(request)
    }

    pub(crate) fn import_custom_theme(
        &self,
        request: ImportCustomThemeRequest,
    ) -> Result<AppearanceSettingsDto, String> {
        self.appearance_store
            .as_ref()
            .ok_or_else(|| "Appearance storage is unavailable.".to_owned())?
            .import_custom(request)
    }

    pub(crate) fn update_custom_theme(
        &self,
        request: UpdateCustomThemeRequest,
    ) -> Result<AppearanceSettingsDto, String> {
        self.appearance_store
            .as_ref()
            .ok_or_else(|| "Appearance storage is unavailable.".to_owned())?
            .update_custom(request)
    }

    pub(crate) fn delete_custom_theme(
        &self,
        request: DeleteCustomThemeRequest,
    ) -> Result<AppearanceSettingsDto, String> {
        self.appearance_store
            .as_ref()
            .ok_or_else(|| "Appearance storage is unavailable.".to_owned())?
            .delete_custom(request)
    }

    fn profile_avatar_for(
        &self,
        owner_type: settings::ProfileAvatarOwnerType,
        owner_key: &str,
    ) -> Option<String> {
        self.store
            .as_ref()
            .and_then(|store| store.profile_avatar(owner_type, owner_key).ok().flatten())
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

    pub(crate) fn remove_account_avatar(&self, email: &str) -> Result<(), String> {
        self.delete_profile_avatar(DeleteProfileAvatarRequest {
            owner_type: ProfileAvatarOwnerType::Account,
            owner_key: email.to_owned(),
        })
    }

    pub(crate) fn user_settings_snapshot(&self) -> Result<DesktopSettingsUpdate, String> {
        let settings = self.settings()?;
        Ok(DesktopSettingsUpdate {
            background_enabled: Some(settings.background_enabled),
            poll_interval_minutes: Some(settings.poll_interval_minutes),
            notifications_enabled: Some(settings.notifications_enabled),
            notification_delivery: Some(settings.notification_delivery),
            notification_sound_enabled: Some(settings.notification_sound_enabled),
            notification_sound: Some(settings.notification_sound),
            remote_image_mode: Some(settings.remote_image_mode),
            ai_assistant_default_open: Some(settings.ai_assistant_default_open),
            idle_poetry_enabled: Some(settings.idle_poetry_enabled),
            mcp_enabled: Some(settings.mcp_enabled),
            mcp_information_enabled: Some(settings.mcp_information_enabled),
            mcp_send_enabled: Some(settings.mcp_send_enabled),
            autostart_enabled: None,
        })
    }

    pub(crate) fn mcp_access(&self) -> Result<McpAccess, String> {
        let settings = self.settings()?;
        Ok(McpAccess {
            enabled: settings.mcp_enabled,
            information: settings.mcp_information_enabled,
            send: settings.mcp_send_enabled,
        })
    }

    pub(crate) fn latest_new_mail_notification(
        &self,
    ) -> Result<Option<NewMailNotificationDto>, String> {
        self.notification_popup
            .lock()
            .map(|notification| {
                notification
                    .as_ref()
                    .map(|notification| notification.display.clone())
            })
            .map_err(|_| "The notification surface is temporarily unavailable.".to_owned())
    }

    fn publish_new_mail_notification(
        &self,
        sender: String,
        sender_email: String,
        sender_remark: Option<String>,
        sender_avatar_data_url: Option<String>,
        subject: String,
        recipient_email: String,
        recipient_remark: Option<String>,
        public_id: String,
        account_id: String,
        count: usize,
        web_sound: Option<settings::NotificationSound>,
    ) -> Result<NewMailNotificationDto, String> {
        let notification_id = self.notification_sequence.fetch_add(1, Ordering::Relaxed) + 1;
        let notification = NewMailNotificationDto {
            notification_id,
            sender,
            sender_email,
            sender_remark,
            sender_avatar_data_url,
            subject,
            recipient_email,
            recipient_remark,
            count,
            web_sound,
        };
        *self
            .notification_popup
            .lock()
            .map_err(|_| "The notification surface is temporarily unavailable.".to_owned())? =
            Some(PendingNewMailNotification {
                display: notification.clone(),
                target: NewMailNotificationTarget {
                    notification_id,
                    account_id,
                    public_id,
                },
            });
        Ok(notification)
    }

    fn consume_new_mail_notification(
        &self,
        notification_id: u64,
        action: impl FnOnce(&NewMailNotificationTarget) -> Result<(), String>,
    ) -> Result<bool, String> {
        let mut current = self
            .notification_popup
            .lock()
            .map_err(|_| "The notification surface is temporarily unavailable.".to_owned())?;
        if current
            .as_ref()
            .map(|notification| notification.target.notification_id)
            != Some(notification_id)
        {
            return Ok(false);
        }
        let target = &current
            .as_ref()
            .expect("the notification identifier was just matched")
            .target;
        // The pending item remains intact if the side effect fails. Holding
        // the lock through the action also prevents an old click from hiding
        // or clearing a newer notification published concurrently.
        action(target)?;
        *current = None;
        Ok(true)
    }

    fn hide_notification_popup_if_idle(
        &self,
        hide_popup: impl FnOnce() -> Result<(), String>,
    ) -> Result<bool, String> {
        let current = self
            .notification_popup
            .lock()
            .map_err(|_| "The notification surface is temporarily unavailable.".to_owned())?;
        if current.is_some() {
            return Ok(false);
        }
        // Keep publication serialized through the actual hide call. A newer
        // pending item either makes us skip this hide, or is published only
        // after the old popup has been hidden.
        hide_popup()?;
        Ok(true)
    }

    pub(crate) fn record_startup_error(&self, error: impl Into<String>) {
        diagnostics::limited_failure(
            "desktop_runtime_degraded",
            "record_startup_error",
            None,
            ErrorKind::Runtime,
        );
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
        match self.startup_error.read() {
            Ok(error) => {
                diagnostics::limited_recovery(
                    "desktop_state_read_failed",
                    "desktop_state_read_recovered",
                    "startup_error_state",
                    None,
                );
                error.is_some()
            }
            Err(_) => {
                diagnostics::limited_failure(
                    "desktop_state_read_failed",
                    "startup_error_state",
                    None,
                    ErrorKind::Runtime,
                );
                true
            }
        }
    }

    pub(crate) fn request_sync(&self, force: bool, trigger: &'static str) {
        match self
            .sync_tx
            .try_send(BackgroundRequest::Sync { force, trigger })
        {
            Ok(()) => diagnostics::limited_recovery(
                "background_request_enqueue_failed",
                "background_request_enqueue_recovered",
                "sync_request",
                None,
            ),
            Err(_) => diagnostics::limited_failure(
                "background_request_enqueue_failed",
                "sync_request",
                None,
                ErrorKind::Runtime,
            ),
        }
    }

    fn request_incremental_inbox_sync(&self, account_id: String, trigger: &'static str) {
        let mut pending = match self.pending_inbox_syncs.lock() {
            Ok(pending) => pending,
            Err(_) => {
                diagnostics::limited_failure(
                    "background_request_enqueue_failed",
                    "inbox_request_state",
                    Some(&account_id),
                    ErrorKind::Runtime,
                );
                return;
            }
        };
        diagnostics::limited_recovery(
            "background_request_enqueue_failed",
            "background_request_enqueue_recovered",
            "inbox_request_state",
            Some(&account_id),
        );
        if !pending.insert(account_id.clone()) {
            return;
        }
        if self
            .sync_tx
            .try_send(BackgroundRequest::InboxChanged {
                account_id: account_id.clone(),
                trigger,
            })
            .is_err()
        {
            pending.remove(&account_id);
            diagnostics::limited_failure(
                "background_request_enqueue_failed",
                "inbox_sync_request",
                Some(&account_id),
                ErrorKind::Runtime,
            );
        } else {
            diagnostics::limited_recovery(
                "background_request_enqueue_failed",
                "background_request_enqueue_recovered",
                "inbox_sync_request",
                Some(&account_id),
            );
        }
    }

    fn begin_incremental_inbox_sync(&self, account_id: &str) {
        match self.pending_inbox_syncs.lock() {
            Ok(mut pending) => {
                pending.remove(account_id);
                diagnostics::limited_recovery(
                    "background_request_state_failed",
                    "background_request_state_recovered",
                    "begin_inbox_sync",
                    Some(account_id),
                );
            }
            Err(_) => diagnostics::limited_failure(
                "background_request_state_failed",
                "begin_inbox_sync",
                Some(account_id),
                ErrorKind::Runtime,
            ),
        }
    }

    pub(crate) fn background_enabled(&self) -> bool {
        match self.settings() {
            Ok(settings) => {
                diagnostics::limited_recovery(
                    "desktop_state_read_failed",
                    "desktop_state_read_recovered",
                    "background_setting",
                    None,
                );
                settings.background_enabled
            }
            Err(_) => {
                diagnostics::limited_failure(
                    "desktop_state_read_failed",
                    "background_setting",
                    None,
                    ErrorKind::Runtime,
                );
                false
            }
        }
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
        match self.shutdown_tx.send(true) {
            Ok(_) => diagnostics::limited_recovery(
                "shutdown_signal_failed",
                "shutdown_signal_recovered",
                "app_exit",
                None,
            ),
            Err(_) => diagnostics::limited_failure(
                "shutdown_signal_failed",
                "app_exit",
                None,
                ErrorKind::Runtime,
            ),
        }
    }

    pub(crate) fn is_quitting(&self) -> bool {
        match self.exit_handshake.lock() {
            Ok(state) => {
                diagnostics::limited_recovery(
                    "exit_state_read_failed",
                    "exit_state_read_recovered",
                    "is_quitting",
                    None,
                );
                state.phase != ExitHandshakePhase::Idle
            }
            Err(_) => {
                diagnostics::limited_failure(
                    "exit_state_read_failed",
                    "is_quitting",
                    None,
                    ErrorKind::Runtime,
                );
                true
            }
        }
    }

    pub(crate) fn is_exit_committed(&self) -> bool {
        match self.exit_handshake.lock() {
            Ok(state) => {
                diagnostics::limited_recovery(
                    "exit_state_read_failed",
                    "exit_state_read_recovered",
                    "is_exit_committed",
                    None,
                );
                matches!(state.phase, ExitHandshakePhase::Committed(_))
            }
            Err(_) => {
                diagnostics::limited_failure(
                    "exit_state_read_failed",
                    "is_exit_committed",
                    None,
                    ErrorKind::Runtime,
                );
                false
            }
        }
    }

    pub(crate) fn begin_smtp_operation(&self) -> Result<SmtpOperationGuard<'_>, String> {
        let state = self
            .exit_handshake
            .lock()
            .map_err(|_| "The exit coordinator is temporarily unavailable.".to_owned())?;
        if state.phase != ExitHandshakePhase::Idle {
            return Err("Mail sending cannot start while Mine Mail is exiting.".to_owned());
        }
        self.smtp_operations_in_flight
            .fetch_add(1, Ordering::AcqRel);
        drop(state);
        Ok(SmtpOperationGuard { runtime: self })
    }

    fn awaiting_exit_ticket(&self, request_id: u64) -> Result<Option<ExitHandshakeTicket>, String> {
        let state = self
            .exit_handshake
            .lock()
            .map_err(|_| "The exit coordinator is temporarily unavailable.".to_owned())?;
        let ExitHandshakePhase::Awaiting(ticket) = state.phase else {
            return Ok(None);
        };
        Ok((ticket.request_id == request_id).then_some(ticket))
    }

    async fn wait_for_smtp_idle(&self, ticket: ExitHandshakeTicket) -> bool {
        loop {
            if self.smtp_operations_in_flight.load(Ordering::Acquire) == 0 {
                return true;
            }
            let notified = self.smtp_idle.notified();
            if self.smtp_operations_in_flight.load(Ordering::Acquire) == 0 {
                return true;
            }
            if tokio::time::timeout_at(TokioInstant::from_std(ticket.deadline), notified)
                .await
                .is_err()
            {
                return self.smtp_operations_in_flight.load(Ordering::Acquire) == 0;
            }
        }
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

    /// Serialize user-initiated changes to account metadata and backend slots.
    /// Acquire this before either lifecycle access mode below.
    pub(crate) async fn acquire_account_mutation_gate(&self) -> AsyncMutexGuard<'_, ()> {
        self.account_mutation_gate.lock().await
    }

    /// Prevent account replacement or cache-deleting removal while a network
    /// operation retains one of its backend handles.
    pub(crate) async fn acquire_sync_access(&self) -> RwLockReadGuard<'_, ()> {
        self.lifecycle_gate.read().await
    }

    /// Adding a distinct account does not invalidate handles retained by syncs
    /// for existing accounts, so it shares lifecycle access with those syncs.
    pub(crate) async fn acquire_account_add_access(&self) -> RwLockReadGuard<'_, ()> {
        self.lifecycle_gate.read().await
    }

    /// Disconnecting an account without deleting its cache can remove the
    /// runtime slot while an existing sync finishes through its retained Arc.
    pub(crate) async fn acquire_account_disconnect_access(&self) -> RwLockReadGuard<'_, ()> {
        self.lifecycle_gate.read().await
    }

    /// Account lifecycle changes wait for every in-flight network operation,
    /// then exclude new work until the backend slots are settled.
    pub(crate) async fn acquire_sync_gate(&self) -> RwLockWriteGuard<'_, ()> {
        self.lifecycle_gate.write().await
    }

    async fn coordinate_inbox_sync<F, Fut>(
        &self,
        account_id: &str,
        requested_operation: &'static str,
        operation: F,
    ) -> Result<SyncReport, String>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<SyncReport, String>>,
    {
        let flight = {
            let mut flights = self
                .inbox_sync_flights
                .lock()
                .map_err(|_| "The Inbox sync coordinator is temporarily unavailable.".to_owned())?;
            flights
                .entry(account_id.to_owned())
                .or_insert_with(|| Arc::new(InboxSyncFlight::default()))
                .clone()
        };
        let mut operation = Some(operation);
        loop {
            let settled = flight.settled.notified();
            let observed_generation = {
                let mut state = flight.state.lock().map_err(|_| {
                    "The Inbox sync coordinator is temporarily unavailable.".to_owned()
                })?;
                if !state.running {
                    state.running = true;
                    None
                } else {
                    Some(state.generation)
                }
            };
            let Some(observed_generation) = observed_generation else {
                let mut leader = InboxSyncLeader {
                    flight: flight.clone(),
                    settled: false,
                };
                let result = operation
                    .take()
                    .expect("only the elected Inbox sync leader runs the operation")(
                )
                .await;
                leader.settle(result.clone());
                return result;
            };

            settled.await;
            let joined = {
                let state = flight.state.lock().map_err(|_| {
                    "The Inbox sync coordinator is temporarily unavailable.".to_owned()
                })?;
                (state.generation != observed_generation)
                    .then(|| state.result.clone())
                    .flatten()
            };
            if let Some(result) = joined {
                diagnostics::info(
                    "account_sync_joined",
                    Fields::default()
                        .account(account_id)
                        .operation(requested_operation)
                        .outcome("joined"),
                );
                return result;
            }
        }
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

    pub(crate) fn begin_notification_baseline(&self, account_id: &str) -> Result<(), String> {
        self.pending_notification_baselines
            .lock()
            .map_err(|_| "The notification baseline is temporarily unavailable.".to_owned())?
            .insert(account_id.to_owned());
        if let Some(store) = self.store.as_ref() {
            store
                .delete_notification_baseline(account_id)
                .map_err(|_| "The notification baseline could not be reset.".to_owned())?;
        }
        Ok(())
    }

    fn notification_baseline_pending(&self, account_id: &str) -> Result<bool, String> {
        self.pending_notification_baselines
            .lock()
            .map(|pending| pending.contains(account_id))
            .map_err(|_| "The notification baseline is temporarily unavailable.".to_owned())
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
        self.pending_notification_baselines
            .lock()
            .map_err(|_| "The notification baseline is temporarily unavailable.".to_owned())?
            .remove(account_id);
        Ok(())
    }

    pub(crate) fn remove_notification_baseline(&self, account_id: &str) -> Result<(), String> {
        self.pending_notification_baselines
            .lock()
            .map_err(|_| "The notification baseline is temporarily unavailable.".to_owned())?
            .remove(account_id);
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
        Ok(last.is_some_and(|instant| instant.elapsed() < self.poll_duration()))
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

/// Provider mailbox names in core synchronization reports are diagnostic
/// coordinates, not desktop capabilities. This explicit boundary retains only
/// bounded counters and semantic state.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct SyncReportDto {
    pub remote_total: u32,
    pub fetched: usize,
    pub updated_flags: usize,
    pub removed: usize,
    pub cached_total: usize,
    pub uid_validity_reset: bool,
}

impl From<&SyncReport> for SyncReportDto {
    fn from(value: &SyncReport) -> Self {
        Self {
            remote_total: value.remote_total,
            fetched: value.fetched,
            updated_flags: value.updated_flags,
            removed: value.removed,
            cached_total: value.cached_total,
            uid_validity_reset: value.uid_validity_reset,
        }
    }
}

impl From<SyncReport> for SyncReportDto {
    fn from(value: SyncReport) -> Self {
        Self::from(&value)
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct DraftSyncReportDto {
    pub pulled: usize,
    pub pushed: usize,
    pub deleted_local: usize,
    pub deleted_remote: usize,
    pub conflicts: usize,
    pub skipped: usize,
    pub local_total: usize,
}

impl From<&DraftSyncReport> for DraftSyncReportDto {
    fn from(value: &DraftSyncReport) -> Self {
        Self {
            pulled: value.pulled,
            pushed: value.pushed,
            deleted_local: value.deleted_local,
            deleted_remote: value.deleted_remote,
            conflicts: value.conflicts,
            skipped: value.skipped,
            local_total: value.local_total,
        }
    }
}

impl From<DraftSyncReport> for DraftSyncReportDto {
    fn from(value: DraftSyncReport) -> Self {
        Self::from(&value)
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct SyncAllReport {
    pub inbox: SyncReportDto,
    pub sent: SyncReportDto,
    pub drafts: DraftSyncReportDto,
    pub accounts_synced: usize,
}

#[derive(Clone, Debug, Serialize)]
struct InboxUpdatedEvent {
    account_id: String,
    completed: usize,
    total: Option<usize>,
    is_complete: bool,
    report: Option<SyncReportDto>,
}

#[derive(Clone, Debug, Serialize)]
struct SentUpdatedEvent {
    account_id: String,
    completed: usize,
    total: Option<usize>,
    is_complete: bool,
    report: Option<SyncReportDto>,
}

#[derive(Clone, Debug, Serialize)]
struct MailboxUpdatedEvent {
    account_id: String,
    role: MailboxRole,
}

#[derive(Clone, Debug, Serialize)]
struct MailboxCapabilitiesUpdatedEvent {
    account_id: String,
}

#[derive(Clone, Debug, Serialize)]
struct SyncErrorEvent {
    operation: &'static str,
    trigger: &'static str,
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
    account_id: Option<String>,
    reason: &'static str,
    completed: usize,
    total: Option<usize>,
    is_complete: bool,
    report: Option<DraftSyncReportDto>,
}

impl DraftsUpdatedEvent {
    pub(crate) fn saved() -> Self {
        Self {
            account_id: None,
            reason: "saved",
            completed: 0,
            total: None,
            is_complete: true,
            report: None,
        }
    }

    pub(crate) fn deleted() -> Self {
        Self {
            account_id: None,
            reason: "deleted",
            completed: 0,
            total: None,
            is_complete: true,
            report: None,
        }
    }

    fn progress(account_id: String, progress: SyncBatchProgress) -> Self {
        Self {
            account_id: Some(account_id),
            reason: "syncing",
            completed: progress.completed,
            total: Some(progress.total),
            is_complete: false,
            report: None,
        }
    }

    fn synced(account_id: String, report: DraftSyncReport) -> Self {
        let report_dto = DraftSyncReportDto::from(&report);
        Self {
            account_id: Some(account_id),
            reason: "synced",
            completed: report.local_total,
            total: Some(report.local_total),
            is_complete: true,
            report: Some(report_dto),
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
            let backend_state = app.state::<BackendState>();
            let account_ids: HashSet<String> = app
                .state::<AccountRuntime>()
                .account_ids()
                .into_iter()
                .filter(|account_id| backend_state.network_ready_for(account_id))
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
                        .request_incremental_inbox_sync(account_id.clone(), "monitor"),
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
                        .request_incremental_inbox_sync(account_id.clone(), "monitor"),
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
                        emit_sync_error(&app, "inbox", "schedule", error);
                    }
                    inbox_deadline = TokioInstant::now() + app.state::<DesktopRuntime>().poll_duration();
                }
                _ = tokio::time::sleep_until(draft_deadline) => {
                    if let Err(error) = perform_draft_sync_all(&app).await {
                        emit_sync_error(&app, "drafts", "schedule", error);
                    }
                    draft_deadline = TokioInstant::now() + DRAFT_SYNC_INTERVAL;
                }
                request = sync_rx.recv() => {
                    match request {
                        Some(BackgroundRequest::Sync { force, trigger }) => {
                            if let Err(error) = perform_sync_all(&app, force, trigger).await {
                                emit_sync_error(&app, "all", trigger, error);
                            }
                            inbox_deadline = TokioInstant::now() + app.state::<DesktopRuntime>().poll_duration();
                            draft_deadline = TokioInstant::now() + DRAFT_SYNC_INTERVAL;
                        }
                        Some(BackgroundRequest::ScheduleChanged) => {
                            inbox_deadline = TokioInstant::now() + app.state::<DesktopRuntime>().poll_duration();
                        }
                        Some(BackgroundRequest::InboxChanged { account_id, trigger }) => {
                            app.state::<DesktopRuntime>().begin_incremental_inbox_sync(&account_id);
                            if let Err(error) = perform_incremental_inbox_sync(&app, &account_id).await {
                                emit_sync_error(&app, "inbox", trigger, error);
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

fn emit_sync_error(
    app: &AppHandle,
    operation: &'static str,
    trigger: &'static str,
    message: String,
) {
    diagnostics::emit_event(
        app,
        "mail:sync-error",
        SyncErrorEvent {
            operation,
            trigger,
            message,
        },
    );
}

fn emit_account_status(
    app: &AppHandle,
    account_runtime: &AccountRuntime,
    backend_state: &BackendState,
) {
    diagnostics::emit_event(
        app,
        "mail:account-updated",
        account_runtime.status(backend_state),
    );
}

fn trigger_discovers_mailbox_roles(trigger: &str) -> bool {
    matches!(
        trigger,
        "startup" | "manual" | "tray" | "account_change" | "single_instance"
    )
}

fn available_optional_mailbox_roles(capabilities: &[MailboxCapability]) -> Vec<MailboxRole> {
    [MailboxRole::Archive, MailboxRole::Trash]
        .into_iter()
        .filter(|role| {
            capabilities.iter().any(|capability| {
                capability.role == *role && capability.status == MailboxCapabilityStatus::Available
            })
        })
        .collect()
}

fn optional_role_participates_periodically(
    role: MailboxRole,
    capabilities: &[MailboxCapability],
    initialized_locally: bool,
    mutation_activity: bool,
) -> bool {
    available_optional_mailbox_roles(capabilities).contains(&role)
        && (initialized_locally || mutation_activity)
}

fn role_has_local_history(backend: &MailBackend, account_id: &str, role: MailboxRole) -> bool {
    backend
        .mailbox_role_initialized(account_id, role)
        .unwrap_or(false)
}

async fn sync_optional_mailbox_for(
    app: &AppHandle,
    account_id: &str,
    role: MailboxRole,
) -> Result<(), String> {
    let started = Instant::now();
    let operation = match role {
        MailboxRole::Archive => "archive_reconciliation",
        MailboxRole::Trash => "trash_reconciliation",
        _ => "optional_mailbox_reconciliation",
    };
    diagnostics::info(
        "account_sync_started",
        Fields::default().account(account_id).operation(operation),
    );
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
    if let Err(error) = backend.sync_mailbox(account_id, role).await {
        diagnostics::limited_failure(
            "account_sync_failed",
            operation,
            Some(account_id),
            diagnostics::mail_error_kind(&error),
        );
        return Err(crate::safe_mail_error(error));
    }
    diagnostics::emit_event(
        app,
        "mail:mailbox-updated",
        MailboxUpdatedEvent {
            account_id: account_id.to_owned(),
            role,
        },
    );
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
            .duration(started.elapsed()),
    );
    Ok(())
}

async fn flush_pending_message_mutations_for(
    backend: &MailBackend,
    account_id: &str,
) -> mine_mail::Result<usize> {
    match backend.flush_pending_message_mutations(account_id).await {
        Ok(changed) => {
            diagnostics::limited_recovery(
                "message_mutation_flush_failed",
                "message_mutation_flush_recovered",
                "queued_message_mutation_flush",
                Some(account_id),
            );
            if changed > 0 {
                diagnostics::info(
                    "message_mutation_flush_completed",
                    Fields::default()
                        .account(account_id)
                        .operation("queued_message_mutation_flush")
                        .outcome("completed")
                        .changes(changed),
                );
            }
            Ok(changed)
        }
        Err(error) => {
            diagnostics::limited_failure(
                "message_mutation_flush_failed",
                "queued_message_mutation_flush",
                Some(account_id),
                diagnostics::mail_error_kind(&error),
            );
            Err(error)
        }
    }
}

pub(crate) async fn perform_sync_all(
    app: &AppHandle,
    force: bool,
    trigger: &'static str,
) -> Result<Option<SyncAllReport>, String> {
    let started = Instant::now();
    let runtime = app.state::<DesktopRuntime>();
    let _batch_guard = runtime.batch_sync_gate.lock().await;
    let _access_guard = runtime.acquire_sync_access().await;
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
    emit_account_status(app, &account_runtime, &backend_state);
    let active_account_id = backend_state.active_account_id();
    let account_ids =
        prioritize_active_account(account_runtime.account_ids(), active_account_id.as_deref());
    if account_ids.is_empty() {
        diagnostics::info(
            "sync_skipped",
            Fields::default()
                .operation_id(operation_id)
                .operation("all")
                .trigger(trigger)
                .outcome("no_accounts")
                .duration(started.elapsed()),
        );
        return Ok(None);
    }
    let account_count = account_ids.len();
    let mut active_inbox = None;
    let mut active_sent = None;
    let mut active_drafts = None;
    let mut accounts_synced = 0;
    let mut errors = Vec::new();

    for account_id in account_ids {
        if !backend_state.network_ready_for(&account_id) {
            continue;
        }
        let network = match backend_state.network_for(&account_id) {
            Ok(network) => network,
            Err(error) => {
                errors.push(format!("{account_id} runtime: {error}"));
                continue;
            }
        };
        let mut account_errors = Vec::new();
        if trigger_discovers_mailbox_roles(trigger) {
            match network.discover_mailbox_roles(&account_id).await {
                Ok(_) => {
                    diagnostics::emit_event(
                        app,
                        "mail:mailbox-capabilities-updated",
                        MailboxCapabilitiesUpdatedEvent {
                            account_id: account_id.clone(),
                        },
                    );
                }
                Err(error) => account_errors.push(format!(
                    "{account_id} mailbox discovery: {}",
                    crate::safe_mail_error(error)
                )),
            }
        }
        if let Err(error) = flush_pending_message_mutations_for(&network, &account_id).await {
            account_errors.push(format!(
                "{account_id} queued mutations: {}",
                crate::safe_mail_error(error)
            ));
        }
        let seeded_gmail_cursor = match network.initialize_gmail_history_cursor().await {
            Ok(seeded) => seeded,
            Err(error) => {
                // Cursor initialization is optional. The full IMAP sync below
                // remains authoritative, and seeding before it prevents a
                // change arriving during reconciliation from being skipped.
                diagnostics::limited_failure(
                    "gmail_history_cursor_failed",
                    "gmail_history_cursor_initialize",
                    Some(&account_id),
                    diagnostics::mail_error_kind(&error),
                );
                false
            }
        };
        let mut full_reconciliation_failed = false;
        let optional_roles = network
            .get_mailbox_capabilities(&account_id)
            .map(|capabilities| available_optional_mailbox_roles(&capabilities))
            .unwrap_or_default();
        let (inbox, sent, drafts) = tokio::join!(
            sync_inbox_for(app, &account_id),
            sync_sent_for(app, &account_id),
            sync_drafts_for(app, &account_id),
        );
        let is_active = active_account_id.as_deref() == Some(account_id.as_str());
        match inbox {
            Ok(report) => {
                if is_active {
                    active_inbox = Some(report);
                }
            }
            Err(error) => {
                full_reconciliation_failed = true;
                account_errors.push(format!("{account_id} Inbox: {error}"));
            }
        }
        match drafts {
            Ok(report) => {
                if is_active {
                    active_drafts = Some(report);
                }
            }
            Err(error) => account_errors.push(format!("{account_id} Drafts: {error}")),
        }
        match sent {
            Ok(report) => {
                if is_active {
                    active_sent = Some(report);
                }
            }
            Err(error) => {
                full_reconciliation_failed = true;
                account_errors.push(format!("{account_id} Sent: {error}"));
            }
        }
        for role in optional_roles {
            if let Err(error) = sync_optional_mailbox_for(app, &account_id, role).await {
                full_reconciliation_failed = true;
                account_errors.push(format!("{account_id} {role:?}: {error}"));
            }
        }
        if seeded_gmail_cursor && full_reconciliation_failed {
            if let Err(error) = network.discard_gmail_history_cursor() {
                diagnostics::limited_failure(
                    "gmail_history_cursor_discard_failed",
                    "gmail_history_cursor_discard",
                    Some(&account_id),
                    diagnostics::mail_error_kind(&error),
                );
            }
        }
        if account_errors.is_empty() {
            accounts_synced += 1;
        } else {
            errors.extend(account_errors);
        }
    }
    if let Some(error) = refresh_error {
        errors.push(error);
    }
    // Emit again after mailbox work so a frontend that mounted while the
    // refresh was in flight still receives the settled readiness snapshot.
    emit_account_status(app, &account_runtime, &backend_state);
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
        return Err("部分账户同步失败，请检查网络或账户凭证。".to_owned());
    }
    let (Some(inbox), Some(sent), Some(drafts)) = (active_inbox, active_sent, active_drafts) else {
        diagnostics::info(
            "sync_completed",
            Fields::default()
                .operation_id(operation_id)
                .operation("all")
                .trigger(trigger)
                .outcome("active_account_unavailable")
                .accounts(account_count)
                .successes(accounts_synced)
                .duration(started.elapsed()),
        );
        return if trigger == "manual" {
            Err("当前账户凭证失效，请重新登录或更新授权凭证。".to_owned())
        } else {
            Ok(None)
        };
    };
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
        inbox: inbox.into(),
        sent: sent.into(),
        drafts: drafts.into(),
        accounts_synced,
    }))
}

fn prioritize_active_account(
    mut account_ids: Vec<String>,
    active_account_id: Option<&str>,
) -> Vec<String> {
    let Some(active_account_id) = active_account_id else {
        return account_ids;
    };
    let Some(index) = account_ids
        .iter()
        .position(|account_id| account_id == active_account_id)
    else {
        return account_ids;
    };
    if index > 0 {
        let active = account_ids.remove(index);
        account_ids.insert(0, active);
    }
    account_ids
}

async fn perform_inbox_reconciliation_all(app: &AppHandle) -> Result<(), String> {
    let runtime = app.state::<DesktopRuntime>();
    let _batch_guard = runtime.batch_sync_gate.lock().await;
    let _access_guard = runtime.acquire_sync_access().await;
    runtime.record_sync_start()?;
    let account_runtime = app.state::<AccountRuntime>();
    let backend_state = app.state::<BackendState>();
    let refresh_error = account_runtime
        .refresh_oauth_backends(&backend_state)
        .await
        .err();
    emit_account_status(app, &account_runtime, &backend_state);
    let mut errors = Vec::new();
    let account_ids = prioritize_active_account(
        account_runtime.account_ids(),
        backend_state.active_account_id().as_deref(),
    );
    for account_id in account_ids {
        if !backend_state.network_ready_for(&account_id) {
            continue;
        }
        let network = match backend_state.network_for(&account_id) {
            Ok(network) => network,
            Err(error) => {
                errors.push(format!("{account_id} runtime: {error}"));
                continue;
            }
        };
        let mutation_activity_before_flush = network
            .has_message_mutation_activity(&account_id)
            .unwrap_or(false);
        let mutation_activity =
            match flush_pending_message_mutations_for(&network, &account_id).await {
                Ok(confirmed) => confirmed > 0,
                Err(error) => {
                    errors.push(format!(
                        "{account_id} queued mutations: {}",
                        crate::safe_mail_error(error)
                    ));
                    false
                }
            } || mutation_activity_before_flush
                || network
                    .has_message_mutation_activity(&account_id)
                    .unwrap_or(false);
        let optional_roles = network
            .get_mailbox_capabilities(&account_id)
            .map(|capabilities| {
                [MailboxRole::Archive, MailboxRole::Trash]
                    .into_iter()
                    .filter(|role| {
                        optional_role_participates_periodically(
                            *role,
                            &capabilities,
                            role_has_local_history(&network, &account_id, *role),
                            mutation_activity,
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        // Only OAuth-authorized Gmail accounts can consume the provider's
        // account-wide change log. A custom app-password account pointing at
        // imap.gmail.com remains on the IMAP branch. History failures also
        // fall through without advancing the cursor.
        match network.sync_gmail_history().await {
            Ok(Some(changed_roles)) => {
                let inbox_changed = changed_roles.contains(&MailboxRole::Inbox);
                for role in changed_roles {
                    match role {
                        MailboxRole::Inbox => {}
                        MailboxRole::Sent => diagnostics::emit_event(
                            app,
                            "mail:sent-updated",
                            SentUpdatedEvent {
                                account_id: account_id.clone(),
                                completed: 0,
                                total: Some(0),
                                is_complete: true,
                                report: None,
                            },
                        ),
                        _ => diagnostics::emit_event(
                            app,
                            "mail:mailbox-updated",
                            MailboxUpdatedEvent {
                                account_id: account_id.clone(),
                                role,
                            },
                        ),
                    }
                }
                if inbox_changed {
                    let report = SyncReport {
                        mailbox: "INBOX".to_owned(),
                        ..SyncReport::default()
                    };
                    if let Err(error) = finish_inbox_sync(app, &account_id, network.clone(), report)
                    {
                        errors.push(format!("{account_id} Inbox: {error}"));
                    }
                }
                continue;
            }
            Ok(None) => {}
            Err(error) => diagnostics::limited_failure(
                "gmail_history_sync_failed",
                "gmail_history_sync",
                Some(&account_id),
                diagnostics::mail_error_kind(&error),
            ),
        }
        let seeded_gmail_cursor = match network.initialize_gmail_history_cursor().await {
            Ok(seeded) => seeded,
            Err(error) => {
                diagnostics::limited_failure(
                    "gmail_history_cursor_failed",
                    "gmail_history_cursor_initialize",
                    Some(&account_id),
                    diagnostics::mail_error_kind(&error),
                );
                false
            }
        };
        let (inbox, sent) = tokio::join!(
            sync_inbox_for(app, &account_id),
            sync_sent_for(app, &account_id),
        );
        let mut full_reconciliation_failed = false;
        if let Err(error) = inbox {
            full_reconciliation_failed = true;
            errors.push(format!("{account_id} Inbox: {error}"));
        }
        if let Err(error) = sent {
            full_reconciliation_failed = true;
            errors.push(format!("{account_id} Sent: {error}"));
        }
        for role in optional_roles {
            if let Err(error) = sync_optional_mailbox_for(app, &account_id, role).await {
                full_reconciliation_failed = true;
                errors.push(format!("{account_id} {role:?}: {error}"));
            }
        }
        if seeded_gmail_cursor && full_reconciliation_failed {
            if let Err(error) = network.discard_gmail_history_cursor() {
                diagnostics::limited_failure(
                    "gmail_history_cursor_discard_failed",
                    "gmail_history_cursor_discard",
                    Some(&account_id),
                    diagnostics::mail_error_kind(&error),
                );
            }
        }
    }
    if let Some(error) = refresh_error {
        errors.push(error);
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err("后台邮箱校准未完成，将在稍后自动重试。".to_owned())
    }
}

async fn perform_draft_sync_all(app: &AppHandle) -> Result<(), String> {
    let runtime = app.state::<DesktopRuntime>();
    let _batch_guard = runtime.batch_sync_gate.lock().await;
    let _access_guard = runtime.acquire_sync_access().await;
    let account_runtime = app.state::<AccountRuntime>();
    let backend_state = app.state::<BackendState>();
    let refresh_error = account_runtime
        .refresh_oauth_backends(&backend_state)
        .await
        .err();
    emit_account_status(app, &account_runtime, &backend_state);
    let mut errors = Vec::new();
    let account_ids = prioritize_active_account(
        account_runtime.account_ids(),
        backend_state.active_account_id().as_deref(),
    );
    for account_id in account_ids {
        if !backend_state.network_ready_for(&account_id) {
            continue;
        }
        if let Err(error) = sync_drafts_for(app, &account_id).await {
            errors.push(format!("{account_id} Drafts: {error}"));
        }
    }
    if let Some(error) = refresh_error {
        errors.push(error);
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err("后台草稿同步未完成，将在稍后自动重试。".to_owned())
    }
}

async fn perform_incremental_inbox_sync(
    app: &AppHandle,
    account_id: &str,
) -> Result<SyncReport, String> {
    let runtime = app.state::<DesktopRuntime>();
    let _access_guard = runtime.acquire_sync_access().await;
    let account_runtime = app.state::<AccountRuntime>();
    let backend_state = app.state::<BackendState>();
    // Usually a no-op; it ensures a monitor event near OAuth expiry uses the
    // refreshed backend before opening the short-lived incremental session.
    let _ = account_runtime.refresh_oauth_backends(&backend_state).await;
    emit_account_status(app, &account_runtime, &backend_state);
    sync_new_inbox_for(app, account_id).await
}

pub(crate) async fn perform_sent_sync(app: &AppHandle) -> Result<SyncReport, String> {
    let runtime = app.state::<DesktopRuntime>();
    let _access_guard = runtime.acquire_sync_access().await;
    runtime.record_sync_start()?;
    let account_runtime = app.state::<AccountRuntime>();
    let backend_state = app.state::<BackendState>();
    let refresh_result = account_runtime
        .refresh_active_oauth_backend(&backend_state)
        .await;
    emit_account_status(app, &account_runtime, &backend_state);
    refresh_result?;
    let account_id = backend_state
        .active_account_id()
        .ok_or_else(|| "No mail account is selected.".to_owned())?;
    sync_sent_for(app, &account_id).await
}

pub(crate) async fn perform_draft_sync(app: &AppHandle) -> Result<DraftSyncReport, String> {
    let runtime = app.state::<DesktopRuntime>();
    let _access_guard = runtime.acquire_sync_access().await;
    let account_runtime = app.state::<AccountRuntime>();
    let backend_state = app.state::<BackendState>();
    let refresh_result = account_runtime
        .refresh_active_oauth_backend(&backend_state)
        .await;
    emit_account_status(app, &account_runtime, &backend_state);
    refresh_result?;
    let account_id = backend_state
        .active_account_id()
        .ok_or_else(|| "No mail account is selected.".to_owned())?;
    sync_drafts_for(app, &account_id).await
}

async fn sync_inbox_for(app: &AppHandle, account_id: &str) -> Result<SyncReport, String> {
    sync_inbox_with_operation(app, account_id, false).await
}

pub(crate) async fn perform_inbox_mailbox_sync(
    app: &AppHandle,
    account_id: &str,
) -> Result<SyncReport, String> {
    let backend = app.state::<BackendState>().network_for(account_id)?;
    app.state::<DesktopRuntime>()
        .coordinate_inbox_sync(account_id, "inbox_reconciliation", || async move {
            backend
                .sync_inbox(crate::INBOX_SYNC_LIMIT)
                .await
                .map_err(crate::safe_mail_error)
        })
        .await
}

async fn sync_inbox_with_operation(
    app: &AppHandle,
    account_id: &str,
    incremental: bool,
) -> Result<SyncReport, String> {
    let operation = if incremental {
        "inbox_incremental"
    } else {
        "inbox_reconciliation"
    };
    app.state::<DesktopRuntime>()
        .coordinate_inbox_sync(account_id, operation, || {
            sync_inbox_network_with_operation(app, account_id, incremental)
        })
        .await
}

async fn sync_inbox_network_with_operation(
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
    diagnostics::info(
        "account_sync_started",
        Fields::default().account(account_id).operation(operation),
    );
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
    diagnostics::emit_event(
        app,
        "mail:inbox-updated",
        InboxUpdatedEvent {
            account_id: account_id.to_owned(),
            completed: 0,
            total: None,
            is_complete: false,
            report: None,
        },
    );
    let progress_app = app.clone();
    let progress_account_id = account_id.to_owned();
    let report = match if incremental {
        backend
            .sync_new_inbox_with_progress(crate::INBOX_SYNC_LIMIT, move |progress| {
                diagnostics::emit_event(
                    &progress_app,
                    "mail:inbox-updated",
                    InboxUpdatedEvent {
                        account_id: progress_account_id.clone(),
                        completed: progress.completed,
                        total: Some(progress.total),
                        is_complete: false,
                        report: None,
                    },
                );
            })
            .await
    } else {
        backend
            .sync_inbox_with_progress(crate::INBOX_SYNC_LIMIT, move |progress| {
                diagnostics::emit_event(
                    &progress_app,
                    "mail:inbox-updated",
                    InboxUpdatedEvent {
                        account_id: progress_account_id.clone(),
                        completed: progress.completed,
                        total: Some(progress.total),
                        is_complete: false,
                        report: None,
                    },
                );
            })
            .await
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

async fn sync_sent_for(app: &AppHandle, account_id: &str) -> Result<SyncReport, String> {
    let started = Instant::now();
    let operation = "sent_reconciliation";
    diagnostics::info(
        "account_sync_started",
        Fields::default().account(account_id).operation(operation),
    );
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
    diagnostics::emit_event(
        app,
        "mail:sent-updated",
        SentUpdatedEvent {
            account_id: account_id.to_owned(),
            completed: 0,
            total: None,
            is_complete: false,
            report: None,
        },
    );
    let progress_app = app.clone();
    let progress_account_id = account_id.to_owned();
    let report = match backend
        .sync_sent_with_progress(crate::SENT_SYNC_LIMIT, move |progress| {
            diagnostics::emit_event(
                &progress_app,
                "mail:sent-updated",
                SentUpdatedEvent {
                    account_id: progress_account_id.clone(),
                    completed: progress.completed,
                    total: Some(progress.total),
                    is_complete: false,
                    report: None,
                },
            );
        })
        .await
    {
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
    diagnostics::emit_event(
        app,
        "mail:sent-updated",
        SentUpdatedEvent {
            account_id: account_id.to_owned(),
            completed: report.fetched,
            total: Some(report.fetched),
            is_complete: true,
            report: Some(SyncReportDto::from(&report)),
        },
    );
    match backend.schedule_sent_body_prefetch(
        crate::INBOX_PREFETCH_LIMIT,
        crate::INBOX_PREFETCH_TOTAL_BYTES,
        crate::INBOX_PREFETCH_MESSAGE_BYTES,
    ) {
        Ok(_) => diagnostics::limited_recovery(
            "body_prefetch_schedule_failed",
            "body_prefetch_schedule_recovered",
            "sent_body_prefetch",
            Some(account_id),
        ),
        Err(error) => diagnostics::limited_failure(
            "body_prefetch_schedule_failed",
            "sent_body_prefetch",
            Some(account_id),
            diagnostics::mail_error_kind(&error),
        ),
    }
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

fn finish_inbox_sync(
    app: &AppHandle,
    account_id: &str,
    backend: std::sync::Arc<mine_mail::MailBackend>,
    report: SyncReport,
) -> Result<SyncReport, String> {
    if let Ok(messages) = backend.list_inbox(crate::INBOX_LIST_LIMIT) {
        update_notification_baseline_and_notify(app, account_id, &report, &messages);
    }
    diagnostics::emit_event(
        app,
        "mail:inbox-updated",
        InboxUpdatedEvent {
            account_id: account_id.to_owned(),
            completed: report.fetched,
            total: Some(report.fetched),
            is_complete: true,
            report: Some(SyncReportDto::from(&report)),
        },
    );
    match backend.schedule_inbox_body_prefetch(
        crate::INBOX_PREFETCH_LIMIT,
        crate::INBOX_PREFETCH_TOTAL_BYTES,
        crate::INBOX_PREFETCH_MESSAGE_BYTES,
    ) {
        Ok(_) => diagnostics::limited_recovery(
            "body_prefetch_schedule_failed",
            "body_prefetch_schedule_recovered",
            "inbox_body_prefetch",
            Some(account_id),
        ),
        Err(error) => diagnostics::limited_failure(
            "body_prefetch_schedule_failed",
            "inbox_body_prefetch",
            Some(account_id),
            diagnostics::mail_error_kind(&error),
        ),
    }
    Ok(report)
}

async fn sync_drafts_for(app: &AppHandle, account_id: &str) -> Result<DraftSyncReport, String> {
    let started = Instant::now();
    diagnostics::info(
        "account_sync_started",
        Fields::default().account(account_id).operation("drafts"),
    );
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
    diagnostics::emit_event(
        app,
        "mail:drafts-updated",
        DraftsUpdatedEvent::progress(
            account_id.to_owned(),
            SyncBatchProgress {
                completed: 0,
                total: 0,
            },
        ),
    );
    let progress_app = app.clone();
    let progress_account_id = account_id.to_owned();
    let report = match backend
        .sync_drafts_with_progress(None, move |progress| {
            diagnostics::emit_event(
                &progress_app,
                "mail:drafts-updated",
                DraftsUpdatedEvent::progress(progress_account_id.clone(), progress),
            );
        })
        .await
    {
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
    diagnostics::emit_event(
        app,
        "mail:drafts-updated",
        DraftsUpdatedEvent::synced(account_id.to_owned(), report.clone()),
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
    let (settings, mut baseline, baseline_pending) = match (
        runtime.settings(),
        runtime.notification_baseline(account_id),
        runtime.notification_baseline_pending(account_id),
    ) {
        (Ok(settings), Ok(baseline), Ok(pending)) => {
            diagnostics::limited_recovery(
                "notification_baseline_read_failed",
                "notification_baseline_read_recovered",
                "new_mail_notification",
                Some(account_id),
            );
            (settings, baseline, pending)
        }
        _ => {
            diagnostics::limited_failure(
                "notification_baseline_read_failed",
                "new_mail_notification",
                Some(account_id),
                ErrorKind::Database,
            );
            return;
        }
    };
    if baseline_pending {
        baseline = NotificationBaseline::default();
    }
    let (next_baseline_uid, mut new_unread) =
        notification_candidates(baseline, report.uid_validity_reset, messages);
    if runtime
        .update_notification_baseline(account_id, next_baseline_uid)
        .is_err()
    {
        diagnostics::limited_failure(
            "notification_baseline_write_failed",
            "new_mail_notification",
            Some(account_id),
            ErrorKind::Database,
        );
        return;
    }
    diagnostics::limited_recovery(
        "notification_baseline_write_failed",
        "notification_baseline_write_recovered",
        "new_mail_notification",
        Some(account_id),
    );
    new_unread.sort_by_key(|message| message.uid);
    if new_unread.is_empty()
        || !should_deliver_new_mail_notification(settings, main_window_is_active(app))
    {
        return;
    }

    let newest = new_unread
        .last()
        .expect("new_unread is known to contain at least one message");
    let count = new_unread.len();
    let sender_email = notification_sender_email(newest);
    let sender_remark = (!sender_email.is_empty())
        .then(|| {
            app.state::<ContactRuntime>()
                .remark_for(&sender_email)
                .ok()
                .flatten()
        })
        .flatten();
    let sender = notification_sender(newest, sender_remark.as_deref());
    let sender_avatar_data_url = (!sender_email.is_empty())
        .then(|| {
            runtime.profile_avatar_for(settings::ProfileAvatarOwnerType::Contact, &sender_email)
        })
        .flatten();
    let (recipient_email, recipient_remark) = app
        .state::<AccountRuntime>()
        .account_email_and_remark(account_id)
        .unwrap_or_else(|| (account_id.to_owned(), None));
    let public_id = match app.state::<BackendState>().local_for(account_id) {
        Ok(backend) => match backend.public_id_for_cached_inbox_message(newest) {
            Ok(public_id) => public_id,
            Err(_) => return,
        },
        Err(_) => return,
    };
    show_new_mail_notification(
        app,
        sender,
        sender_email,
        sender_remark,
        sender_avatar_data_url,
        notification_subject(newest),
        recipient_email,
        recipient_remark,
        public_id,
        account_id.to_owned(),
        count,
        settings,
    );
}

fn notification_candidates(
    baseline: NotificationBaseline,
    uid_validity_reset: bool,
    messages: &[InboxMessage],
) -> (u32, Vec<&InboxMessage>) {
    let current_highest_uid = messages
        .iter()
        .map(|message| message.uid)
        .max()
        .unwrap_or(0);
    if !baseline.initialized || uid_validity_reset {
        return (current_highest_uid, Vec::new());
    }
    (
        current_highest_uid.max(baseline.uid),
        messages
            .iter()
            .filter(|message| message.uid > baseline.uid && !is_seen(message))
            .collect(),
    )
}

fn should_deliver_new_mail_notification(
    settings: StoredDesktopSettings,
    _main_window_is_active: bool,
) -> bool {
    settings.notifications_enabled
}

fn show_new_mail_notification(
    app: &AppHandle,
    sender: String,
    sender_email: String,
    sender_remark: Option<String>,
    sender_avatar_data_url: Option<String>,
    subject: String,
    recipient_email: String,
    recipient_remark: Option<String>,
    public_id: String,
    account_id: String,
    count: usize,
    settings: StoredDesktopSettings,
) {
    let sender = sanitize_notification_text(&sender, 80);
    let sender_email = sanitize_notification_text(&sender_email, 320);
    let sender_remark = sender_remark.map(|value| sanitize_notification_text(&value, 40));
    let subject = sanitize_notification_text(&subject, 140);
    let recipient_email = sanitize_notification_text(&recipient_email, 320);
    let recipient_remark = recipient_remark.map(|value| sanitize_notification_text(&value, 40));

    if effective_notification_delivery(settings.notification_delivery, cfg!(target_os = "windows"))
        == NotificationDelivery::Windows
    {
        #[cfg(target_os = "windows")]
        show_windows_new_mail_notification(
            app,
            windows_notification_content(
                &sender,
                &sender_email,
                recipient_remark.as_deref(),
                &recipient_email,
                &subject,
                count,
            ),
            NewMailNotificationTarget {
                notification_id: 0,
                account_id,
                public_id,
            },
            settings,
        );
        return;
    }

    show_mine_mail_new_mail_notification(
        app,
        sender,
        sender_email,
        sender_remark,
        sender_avatar_data_url,
        subject,
        recipient_email,
        recipient_remark,
        public_id,
        account_id,
        count,
        settings,
    );
}

fn effective_notification_delivery(
    requested: NotificationDelivery,
    windows_notifications_available: bool,
) -> NotificationDelivery {
    if windows_notifications_available && requested == NotificationDelivery::Windows {
        NotificationDelivery::Windows
    } else {
        NotificationDelivery::MineMail
    }
}

fn windows_notification_content(
    sender: &str,
    sender_email: &str,
    recipient_remark: Option<&str>,
    recipient_email: &str,
    subject: &str,
    count: usize,
) -> WindowsNotificationContent {
    let title = notification_identity_label(sender, sender_email, 120);
    let recipient = notification_identity_label(
        recipient_remark.unwrap_or(recipient_email),
        recipient_email,
        180,
    );
    let count_label = if count > 99 {
        "99+ 封新邮件".to_owned()
    } else {
        format!("{} 封新邮件", count.max(1))
    };
    let context = match (count > 1, recipient.is_empty()) {
        (true, false) => format!("{count_label} · 收信至 {recipient}"),
        (true, true) => count_label,
        (false, false) => format!("收信至 {recipient}"),
        (false, true) => String::new(),
    };
    WindowsNotificationContent {
        title,
        subject: sanitize_notification_text(subject, 140),
        context: sanitize_notification_text(&context, 220),
    }
}

fn notification_identity_label(display: &str, email: &str, max_characters: usize) -> String {
    let display = display.trim();
    let email = email.trim();
    let label = match (display.is_empty(), email.is_empty()) {
        (true, true) => String::new(),
        (true, false) => email.to_owned(),
        (false, true) => display.to_owned(),
        (false, false) if display.eq_ignore_ascii_case(email) => display.to_owned(),
        (false, false) => format!("{display} · {email}"),
    };
    sanitize_notification_text(&label, max_characters)
}

fn show_mine_mail_new_mail_notification(
    app: &AppHandle,
    sender: String,
    sender_email: String,
    sender_remark: Option<String>,
    sender_avatar_data_url: Option<String>,
    subject: String,
    recipient_email: String,
    recipient_remark: Option<String>,
    public_id: String,
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
        sender,
        sender_email,
        sender_remark,
        sender_avatar_data_url,
        subject,
        recipient_email,
        recipient_remark,
        public_id,
        account_id,
        count,
        web_sound,
    ) else {
        diagnostics::limited_failure(
            "notification_publish_failed",
            "new_mail_notification",
            None,
            ErrorKind::Runtime,
        );
        return;
    };
    diagnostics::limited_recovery(
        "notification_publish_failed",
        "notification_publish_recovered",
        "new_mail_notification",
        None,
    );

    if settings.notification_sound_enabled {
        play_native_notification_sound(settings.notification_sound);
    }

    if let Some(window) = app.get_webview_window(NEW_MAIL_NOTIFICATION_WINDOW) {
        diagnostics::limited_recovery(
            "notification_window_unavailable",
            "notification_window_recovered",
            "new_mail_notification",
            None,
        );
        position_notification_window(app, &window);
        match window.show() {
            Ok(()) => diagnostics::limited_recovery(
                "notification_window_show_failed",
                "notification_window_show_recovered",
                "new_mail_notification",
                None,
            ),
            Err(_) => diagnostics::limited_failure(
                "notification_window_show_failed",
                "new_mail_notification",
                None,
                ErrorKind::Runtime,
            ),
        }
        diagnostics::emit_to_event(
            app,
            NEW_MAIL_NOTIFICATION_WINDOW,
            "mail:new-mail-notification",
            notification,
        );
    } else {
        diagnostics::limited_failure(
            "notification_window_unavailable",
            "new_mail_notification",
            None,
            ErrorKind::NotFound,
        );
    }
}

#[cfg(target_os = "windows")]
fn show_windows_new_mail_notification(
    app: &AppHandle,
    content: WindowsNotificationContent,
    target: NewMailNotificationTarget,
    settings: StoredDesktopSettings,
) {
    let app = app.clone();
    if app
        .clone()
        .run_on_main_thread(move || {
            use tauri_winrt_notification::{Duration as ToastDuration, Sound, Toast};

            let activation_app = app.clone();
            let activation_target = target.clone();
            let sound =
                settings
                    .notification_sound_enabled
                    .then_some(match settings.notification_sound {
                        settings::NotificationSound::Default => Sound::Default,
                        settings::NotificationSound::Mail => Sound::Mail,
                        settings::NotificationSound::Im => Sound::IM,
                        settings::NotificationSound::Reminder => Sound::Reminder,
                    });
            let mut toast = Toast::new(&app.config().identifier)
                .title(&content.title)
                .text1(&content.subject)
                .duration(ToastDuration::Short)
                .sound(sound)
                .on_activated(move |_| {
                    match open_notification_target(&activation_app, &activation_target) {
                        Ok(()) => diagnostics::limited_recovery(
                            "notification_windows_open_failed",
                            "notification_windows_open_recovered",
                            "new_mail_notification",
                            None,
                        ),
                        Err(_) => diagnostics::limited_failure(
                            "notification_windows_open_failed",
                            "new_mail_notification",
                            None,
                            ErrorKind::Runtime,
                        ),
                    }
                    Ok(())
                });
            if !content.context.is_empty() {
                toast = toast.text2(&content.context);
            }
            match toast.show() {
                Ok(()) => diagnostics::limited_recovery(
                    "notification_windows_delivery_failed",
                    "notification_windows_delivery_recovered",
                    "new_mail_notification",
                    None,
                ),
                Err(_) => diagnostics::limited_failure(
                    "notification_windows_delivery_failed",
                    "new_mail_notification",
                    None,
                    ErrorKind::Runtime,
                ),
            }
        })
        .is_err()
    {
        diagnostics::limited_failure(
            "notification_windows_schedule_failed",
            "new_mail_notification",
            None,
            ErrorKind::Runtime,
        );
    }
}

fn notification_sender_email(message: &InboxMessage) -> String {
    message
        .sender
        .as_ref()
        .map(|address| address.email.trim().to_owned())
        .unwrap_or_default()
}

fn notification_sender(message: &InboxMessage, remark: Option<&str>) -> String {
    if let Some(remark) = remark.filter(|value| !value.trim().is_empty()) {
        return remark.to_owned();
    }
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
    let x = align_notification_axis(
        work_area.position.x,
        work_area.size.width,
        window_size.width,
    );
    let y = align_notification_axis(
        work_area.position.y,
        work_area.size.height,
        window_size.height,
    );
    observe_window_action(
        "notification_window_position",
        window.set_position(PhysicalPosition::new(x, y)),
    );
}

fn align_notification_axis(work_area_start: i32, work_area_length: u32, window_length: u32) -> i32 {
    let work_area_start = i64::from(work_area_start);
    let aligned = work_area_start + i64::from(work_area_length) - i64::from(window_length);
    aligned
        .max(work_area_start)
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
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
    let Some(window) = app.get_webview_window("main") else {
        diagnostics::limited_failure(
            "main_window_state_failed",
            "main_window_activity",
            None,
            ErrorKind::NotFound,
        );
        return false;
    };
    match (window.is_visible(), window.is_focused()) {
        (Ok(visible), Ok(focused)) => {
            diagnostics::limited_recovery(
                "main_window_state_failed",
                "main_window_state_recovered",
                "main_window_activity",
                None,
            );
            visible && focused
        }
        _ => {
            diagnostics::limited_failure(
                "main_window_state_failed",
                "main_window_activity",
                None,
                ErrorKind::Runtime,
            );
            false
        }
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
            "open" => show_main_window_and_refresh(app),
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
                show_main_window_and_refresh(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

pub(crate) fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        diagnostics::limited_recovery(
            "main_window_unavailable",
            "main_window_recovered",
            "show_main_window",
            None,
        );
        observe_window_action("main_window_unminimize", window.unminimize());
        observe_window_action("main_window_show", window.show());
        observe_window_action("main_window_focus", window.set_focus());
    } else {
        diagnostics::limited_failure(
            "main_window_unavailable",
            "show_main_window",
            None,
            ErrorKind::NotFound,
        );
    }
}

pub(crate) fn show_main_window_and_refresh(app: &AppHandle) {
    show_main_window(app);
    request_sync(app, false, "window_open");
}

pub(crate) fn observe_window_action(operation: &'static str, result: tauri::Result<()>) {
    match result {
        Ok(()) => diagnostics::limited_recovery(
            "window_action_failed",
            "window_action_recovered",
            operation,
            None,
        ),
        Err(_) => diagnostics::limited_failure(
            "window_action_failed",
            operation,
            None,
            ErrorKind::Runtime,
        ),
    }
}

pub(crate) fn dismiss_new_mail_notification(
    app: &AppHandle,
    notification_id: u64,
) -> Result<bool, String> {
    let runtime = app.state::<DesktopRuntime>();
    runtime.consume_new_mail_notification(notification_id, |_| {
        if let Some(window) = app.get_webview_window(NEW_MAIL_NOTIFICATION_WINDOW) {
            window
                .hide()
                .map_err(|_| "The notification window could not be hidden.".to_owned())?;
        }
        Ok(())
    })
}

fn consume_open_new_mail_notification(
    runtime: &DesktopRuntime,
    notification_id: u64,
    emit_open: impl FnOnce(&NewMailNotificationTarget) -> Result<(), String>,
    hide_popup: impl FnOnce() -> Result<(), String>,
) -> Result<bool, String> {
    let consumed = runtime.consume_new_mail_notification(notification_id, emit_open)?;
    if !consumed {
        return Ok(false);
    }
    if runtime.hide_notification_popup_if_idle(hide_popup).is_err() {
        // Emitting the main-window navigation event is the commit point. A
        // popup hide failure must not restore the consumed target and make a
        // retry emit the same navigation event twice.
        diagnostics::warn(
            "notification_window_hide_failed",
            Fields::default()
                .operation("notification_open")
                .outcome("target_consumed"),
        );
    }
    Ok(true)
}

fn open_notification_target(
    app: &AppHandle,
    target: &NewMailNotificationTarget,
) -> Result<(), String> {
    show_main_window(app);
    app.emit_to(
        "main",
        "mail:open-message",
        OpenMessageEvent {
            message_id: target.public_id.clone(),
            account_id: target.account_id.clone(),
        },
    )
    .map_err(|_| "The selected message could not be opened.".to_owned())
}

pub(crate) fn open_new_mail_notification(
    app: &AppHandle,
    notification_id: u64,
) -> Result<bool, String> {
    let runtime = app.state::<DesktopRuntime>();
    consume_open_new_mail_notification(
        &runtime,
        notification_id,
        |target| open_notification_target(app, target),
        || {
            if let Some(window) = app.get_webview_window(NEW_MAIL_NOTIFICATION_WINDOW) {
                window
                    .hide()
                    .map_err(|_| "The notification window could not be hidden.".to_owned())?;
            }
            Ok(())
        },
    )
}

pub(crate) fn request_sync(app: &AppHandle, force: bool, trigger: &'static str) {
    if let Some(runtime) = app.try_state::<DesktopRuntime>() {
        runtime.request_sync(force, trigger);
    }
}

/// Window visibility changes need fresh Inbox counters, not a multi-account,
/// all-role reconciliation. Each account is still checked, and the existing
/// pending set coalesces a simultaneous monitor notification.
pub(crate) fn request_incremental_inbox_refresh(app: &AppHandle, trigger: &'static str) {
    let Some(runtime) = app.try_state::<DesktopRuntime>() else {
        return;
    };
    let Some(accounts) = app.try_state::<AccountRuntime>() else {
        return;
    };
    let Some(backends) = app.try_state::<BackendState>() else {
        return;
    };
    for account_id in accounts
        .account_ids()
        .into_iter()
        .filter(|account_id| backends.network_ready_for(account_id))
    {
        runtime.request_incremental_inbox_sync(account_id, trigger);
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

    diagnostics::emit_event(
        app,
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

pub(crate) async fn complete_exit(app: &AppHandle, request_id: u64) -> Result<bool, String> {
    let Some(runtime) = app.try_state::<DesktopRuntime>() else {
        return Ok(false);
    };
    let Some(ticket) = runtime.awaiting_exit_ticket(request_id)? else {
        return Ok(false);
    };
    let smtp_idle = runtime.wait_for_smtp_idle(ticket).await;
    if !smtp_idle {
        if !runtime.commit_quit_timeout(ticket, Instant::now())? {
            return Ok(false);
        }
        runtime.finish_quit();
        diagnostics::warn(
            "shutdown_committed",
            Fields::default()
                .operation("app_exit")
                .outcome("smtp_timeout"),
        );
        app.exit(0);
        return Ok(true);
    }
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
    use std::{
        cell::Cell,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::{Duration, Instant},
    };

    use mine_mail::{
        DraftSyncReport, InboxMessage, MailAddress, MailboxCapability, MailboxCapabilityStatus,
        MailboxRole, SyncReport,
    };
    use tokio::sync::Notify;

    use super::settings::{
        NotificationBaseline, NotificationDelivery, NotificationSound, StoredDesktopSettings,
    };
    use super::{
        BeforeExitEvent, DesktopRuntime, EXIT_HANDSHAKE_TIMEOUT, SyncAllReport,
        align_notification_axis, available_optional_mailbox_roles,
        consume_open_new_mail_notification, effective_notification_delivery, is_seen,
        notification_candidates, notification_sender, notification_sender_email,
        optional_role_participates_periodically, prioritize_active_account,
        sanitize_notification_text, should_deliver_new_mail_notification,
        trigger_discovers_mailbox_roles, windows_notification_content,
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
            bcc: vec![],
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
    fn notification_axis_sits_flush_with_the_work_area_end() {
        assert_eq!(align_notification_axis(0, 1920, 388), 1532);
        assert_eq!(align_notification_axis(-1920, 1920, 388), -388);
    }

    #[test]
    fn notification_axis_stays_inside_a_work_area_smaller_than_the_window() {
        assert_eq!(align_notification_axis(320, 300, 388), 320);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn account_add_and_disconnect_share_access_with_an_existing_sync() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let (runtime, _sync_rx, _shutdown_rx) = DesktopRuntime::open(directory.path());
        let sync_guard = runtime.acquire_sync_access().await;

        let add_guard = tokio::time::timeout(
            Duration::from_millis(20),
            runtime.acquire_account_add_access(),
        )
        .await
        .expect("adding a distinct account must not wait for an existing sync");
        drop(add_guard);
        let disconnect_guard = tokio::time::timeout(
            Duration::from_millis(20),
            runtime.acquire_account_disconnect_access(),
        )
        .await
        .expect("disconnect without cache deletion must not wait for an existing sync");
        drop(disconnect_guard);

        let mut exclusive = Box::pin(runtime.acquire_sync_gate());
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut exclusive)
                .await
                .is_err(),
            "replacement and cache deletion must still wait for retained handles"
        );
        drop(sync_guard);
        let _exclusive_guard = tokio::time::timeout(Duration::from_millis(100), exclusive)
            .await
            .expect("exclusive lifecycle access after the sync settles");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn concurrent_inbox_requests_join_one_account_flight() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let (runtime, _sync_rx, _shutdown_rx) = DesktopRuntime::open(directory.path());
        let runtime = Arc::new(runtime);
        let executions = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let started_wait = started.notified();
        let expected = SyncReport {
            mailbox: "INBOX".to_owned(),
            remote_total: 4,
            fetched: 1,
            updated_flags: 0,
            removed: 0,
            cached_total: 4,
            uid_validity_reset: false,
        };

        let first = {
            let runtime = runtime.clone();
            let executions = executions.clone();
            let started = started.clone();
            let release = release.clone();
            let expected = expected.clone();
            tokio::spawn(async move {
                runtime
                    .coordinate_inbox_sync("account-a", "inbox_reconciliation", || async move {
                        executions.fetch_add(1, Ordering::SeqCst);
                        started.notify_one();
                        release.notified().await;
                        Ok(expected)
                    })
                    .await
            })
        };
        started_wait.await;
        let second = {
            let runtime = runtime.clone();
            let executions = executions.clone();
            tokio::spawn(async move {
                runtime
                    .coordinate_inbox_sync("account-a", "inbox_reconciliation", || async move {
                        executions.fetch_add(1, Ordering::SeqCst);
                        Ok(SyncReport::default())
                    })
                    .await
            })
        };
        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
        assert!(!second.is_finished());
        release.notify_one();

        assert_eq!(
            first.await.expect("first task").expect("first sync"),
            expected
        );
        assert_eq!(
            second.await.expect("second task").expect("joined sync"),
            expected
        );
        assert_eq!(executions.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn seen_flag_check_is_case_insensitive() {
        assert!(is_seen(&message(vec!["\\seen".to_owned()])));
        assert!(!is_seen(&message(vec!["\\Flagged".to_owned()])));
    }

    #[test]
    fn active_account_is_synchronized_before_other_accounts() {
        assert_eq!(
            prioritize_active_account(
                vec![
                    "account-a".to_owned(),
                    "account-b".to_owned(),
                    "account-c".to_owned(),
                ],
                Some("account-c"),
            ),
            vec![
                "account-c".to_owned(),
                "account-a".to_owned(),
                "account-b".to_owned(),
            ],
        );
    }

    #[test]
    fn startup_and_explicit_refresh_discover_roles_but_periodic_paths_do_not() {
        for trigger in [
            "startup",
            "manual",
            "tray",
            "account_change",
            "single_instance",
        ] {
            assert!(trigger_discovers_mailbox_roles(trigger), "{trigger}");
        }
        for trigger in ["schedule", "resume", "window_focus", "monitor"] {
            assert!(!trigger_discovers_mailbox_roles(trigger), "{trigger}");
        }
    }

    #[test]
    fn optional_mailboxes_require_capability_and_periodic_participation() {
        let capabilities = vec![
            MailboxCapability {
                role: MailboxRole::Archive,
                status: MailboxCapabilityStatus::Available,
                display_name: Some("[Gmail]/所有邮件".to_owned()),
                unavailable_reason: None,
                retryable: false,
            },
            MailboxCapability {
                role: MailboxRole::Trash,
                status: MailboxCapabilityStatus::NeedsCreationConfirmation,
                display_name: None,
                unavailable_reason: None,
                retryable: false,
            },
        ];

        assert_eq!(
            available_optional_mailbox_roles(&capabilities),
            vec![MailboxRole::Archive]
        );
        assert!(optional_role_participates_periodically(
            MailboxRole::Archive,
            &capabilities,
            true,
            false,
        ));
        assert!(optional_role_participates_periodically(
            MailboxRole::Archive,
            &capabilities,
            false,
            true,
        ));
        assert!(!optional_role_participates_periodically(
            MailboxRole::Archive,
            &capabilities,
            false,
            false,
        ));
        assert!(!optional_role_participates_periodically(
            MailboxRole::Trash,
            &capabilities,
            true,
            true,
        ));
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
    fn notification_sender_keeps_address_and_prefers_local_remark() {
        let message = message(Vec::new());
        assert_eq!(notification_sender_email(&message), "sender@example.com");
        assert_eq!(notification_sender(&message, None), "Sender");
        assert_eq!(notification_sender(&message, Some("产品团队")), "产品团队");
    }

    #[test]
    fn windows_delivery_is_used_only_when_the_platform_capability_is_available() {
        assert_eq!(
            effective_notification_delivery(NotificationDelivery::Windows, true),
            NotificationDelivery::Windows
        );
        assert_eq!(
            effective_notification_delivery(NotificationDelivery::Windows, false),
            NotificationDelivery::MineMail
        );
        assert_eq!(
            effective_notification_delivery(NotificationDelivery::MineMail, true),
            NotificationDelivery::MineMail
        );
    }

    #[test]
    fn windows_notification_content_contains_identity_subject_account_and_bounded_count() {
        let single = windows_notification_content(
            "产品团队",
            "sender@example.com",
            Some("工作邮箱"),
            "me@example.com",
            "发布说明",
            1,
        );
        assert_eq!(single.title, "产品团队 · sender@example.com");
        assert_eq!(single.subject, "发布说明");
        assert_eq!(single.context, "收信至 工作邮箱 · me@example.com");

        let batch = windows_notification_content(
            "sender@example.com",
            "sender@example.com",
            None,
            "me@example.com",
            "最新邮件",
            120,
        );
        assert_eq!(batch.title, "sender@example.com");
        assert_eq!(batch.context, "99+ 封新邮件 · 收信至 me@example.com");
    }

    #[test]
    fn desktop_notifications_use_one_switch_in_foreground_and_background() {
        let mut settings = StoredDesktopSettings::default();
        assert!(should_deliver_new_mail_notification(settings, false));
        assert!(should_deliver_new_mail_notification(settings, true));

        settings.notifications_enabled = false;
        assert!(!should_deliver_new_mail_notification(settings, false));
        assert!(!should_deliver_new_mail_notification(settings, true));
    }

    #[test]
    fn first_historical_import_establishes_baseline_without_notification_candidates() {
        let mut messages = Vec::new();
        for uid in 1..=250 {
            let mut item = message(Vec::new());
            item.uid = uid;
            messages.push(item);
        }

        let (next_uid, candidates) =
            notification_candidates(NotificationBaseline::default(), false, &messages);

        assert_eq!(next_uid, 250);
        assert!(candidates.is_empty());
    }

    #[test]
    fn binding_an_account_resets_a_retained_notification_baseline() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let (runtime, _sync_rx, _shutdown_rx) = DesktopRuntime::open(directory.path());
        runtime
            .update_notification_baseline("retained-account", 42)
            .expect("save stale baseline");

        runtime
            .begin_notification_baseline("retained-account")
            .expect("reset baseline for binding");

        assert!(
            runtime
                .notification_baseline_pending("retained-account")
                .expect("pending baseline state")
        );
        assert_eq!(
            runtime
                .notification_baseline("retained-account")
                .expect("reset baseline"),
            NotificationBaseline::default()
        );
        runtime
            .update_notification_baseline("retained-account", 88)
            .expect("establish new baseline");
        assert!(
            !runtime
                .notification_baseline_pending("retained-account")
                .expect("settled baseline state")
        );
        assert_eq!(
            runtime
                .notification_baseline("retained-account")
                .expect("established baseline")
                .uid,
            88
        );
    }

    #[test]
    fn synchronization_dtos_drop_provider_mailbox_coordinates_recursively() {
        let inbox = SyncReport {
            mailbox: "[Gmail]/所有邮件".to_owned(),
            remote_total: 8,
            fetched: 3,
            updated_flags: 2,
            removed: 1,
            cached_total: 7,
            uid_validity_reset: false,
        };
        let sent = SyncReport {
            mailbox: "[Gmail]/已发送邮件".to_owned(),
            ..inbox.clone()
        };
        let drafts = DraftSyncReport {
            mailbox: "[Gmail]/草稿".to_owned(),
            pulled: 1,
            pushed: 2,
            deleted_local: 0,
            deleted_remote: 0,
            conflicts: 1,
            skipped: 0,
            local_total: 4,
        };
        let dto = SyncAllReport {
            inbox: inbox.into(),
            sent: sent.into(),
            drafts: drafts.into(),
            accounts_synced: 2,
        };
        let json = serde_json::to_value(dto).expect("serialize safe synchronization report");

        assert_eq!(json["inbox"]["fetched"], 3);
        assert_eq!(json["drafts"]["conflicts"], 1);
        assert!(!json.to_string().contains("Gmail"));
        crate::assert_no_private_mail_coordinates(&json);
    }

    #[test]
    fn notification_surface_keeps_targets_private_and_consumes_only_current_successes() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let (runtime, _sync_rx, _shutdown_rx) = DesktopRuntime::open(directory.path());
        let first = runtime
            .publish_new_mail_notification(
                "Sender".to_owned(),
                "sender@example.com".to_owned(),
                None,
                None,
                "Subject".to_owned(),
                "mine@example.com".to_owned(),
                Some("工作邮箱".to_owned()),
                "opaque-message-1".to_owned(),
                "account-test".to_owned(),
                1,
                Some(NotificationSound::Mail),
            )
            .expect("publish notification");
        let display_json =
            serde_json::to_value(&first).expect("serialize notification display boundary");
        assert_eq!(display_json["notificationId"], first.notification_id);
        crate::assert_no_private_mail_coordinates(&display_json);
        for private_key in [
            "uid",
            "accountId",
            "account_id",
            "messageId",
            "message_id",
            "publicId",
            "public_id",
        ] {
            assert!(
                display_json.get(private_key).is_none(),
                "private notification target field crossed the display boundary: {private_key}"
            );
        }

        let second = runtime
            .publish_new_mail_notification(
                "New Sender".to_owned(),
                "new@example.com".to_owned(),
                None,
                None,
                "New Subject".to_owned(),
                "mine@example.com".to_owned(),
                None,
                "opaque-message-2".to_owned(),
                "account-new".to_owned(),
                2,
                None,
            )
            .expect("publish updated notification");
        let stale_action_ran = Cell::new(false);
        assert!(
            !runtime
                .consume_new_mail_notification(first.notification_id, |_| {
                    stale_action_ran.set(true);
                    Ok(())
                })
                .unwrap()
        );
        assert!(!stale_action_ran.get());

        let failure = runtime.consume_new_mail_notification(second.notification_id, |_| {
            Err("simulated emit failure".to_owned())
        });
        assert_eq!(failure.unwrap_err(), "simulated emit failure");
        let pending = runtime
            .latest_new_mail_notification()
            .expect("notification state")
            .expect("pending notification");
        assert_eq!(pending.notification_id, second.notification_id);
        assert_eq!(pending.sender_email, "new@example.com");
        assert_eq!(pending.recipient_email, "mine@example.com");

        let emit_count = Cell::new(0);
        let hide_count = Cell::new(0);
        assert!(
            consume_open_new_mail_notification(
                &runtime,
                second.notification_id,
                |target| {
                    emit_count.set(emit_count.get() + 1);
                    assert_eq!(target.notification_id, second.notification_id);
                    assert_eq!(target.account_id, "account-new");
                    assert_eq!(target.public_id, "opaque-message-2");
                    Ok(())
                },
                || {
                    hide_count.set(hide_count.get() + 1);
                    Err("simulated hide failure".to_owned())
                },
            )
            .unwrap()
        );
        assert!(runtime.latest_new_mail_notification().unwrap().is_none());
        assert!(
            !consume_open_new_mail_notification(
                &runtime,
                second.notification_id,
                |_| {
                    emit_count.set(emit_count.get() + 1);
                    Ok(())
                },
                || {
                    hide_count.set(hide_count.get() + 1);
                    Ok(())
                },
            )
            .unwrap()
        );
        assert_eq!(emit_count.get(), 1);
        assert_eq!(hide_count.get(), 1);

        let third = runtime
            .publish_new_mail_notification(
                "Newest Sender".to_owned(),
                "newest@example.com".to_owned(),
                None,
                None,
                "Newest Subject".to_owned(),
                "mine@example.com".to_owned(),
                None,
                "opaque-message-3".to_owned(),
                "account-newest".to_owned(),
                1,
                None,
            )
            .expect("publish notification racing a stale hide");
        assert!(
            !runtime
                .hide_notification_popup_if_idle(|| {
                    hide_count.set(hide_count.get() + 1);
                    Ok(())
                })
                .unwrap()
        );
        assert_eq!(hide_count.get(), 1);
        assert_eq!(
            runtime
                .latest_new_mail_notification()
                .unwrap()
                .expect("new notification remains visible")
                .notification_id,
            third.notification_id
        );
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

    #[tokio::test]
    async fn quit_waits_for_active_smtp_and_rejects_new_attempts() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let (runtime, _sync_rx, _shutdown_rx) = DesktopRuntime::open(directory.path());
        let smtp = runtime
            .begin_smtp_operation()
            .expect("start SMTP operation");
        let ticket = runtime
            .start_quit_handshake_at(Instant::now(), Duration::from_secs(1))
            .expect("start handshake")
            .expect("exit ticket");

        assert!(runtime.begin_smtp_operation().is_err());
        let mut waiting = Box::pin(runtime.wait_for_smtp_idle(ticket));
        assert!(
            tokio::time::timeout(Duration::from_millis(10), &mut waiting)
                .await
                .is_err()
        );

        drop(smtp);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), waiting)
                .await
                .expect("SMTP idle notification")
        );
        assert!(
            runtime
                .complete_quit_handshake(ticket.request_id)
                .expect("commit current exit")
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
