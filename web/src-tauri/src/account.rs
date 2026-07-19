use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use keyring::Entry;
use mine_mail::{AccountConfig, MailBackend, ServerConfig, SmtpSecurity};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    time::timeout,
};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

use crate::diagnostics::{self, ErrorKind as DiagnosticErrorKind, Fields as DiagnosticFields};

include!(concat!(env!("OUT_DIR"), "/google_oauth_config.rs"));

const ACCOUNT_METADATA_FILE: &str = "account.json";
const ACCOUNT_STORE_VERSION: u8 = 2;
const ACCOUNT_METADATA_VERSION: u8 = 1;
const MAX_ACCOUNTS: usize = 3;
const KEYRING_SERVICE: &str = "com.minemail.desktop";
const LEGACY_KEYRING_USERNAME: &str = "primary";
const KEYRING_USERNAME_PREFIX: &str = "account-";
const LOCAL_ONLY_PLACEHOLDER_SECRET: &str = "mine-mail-local-cache-only";
const OUTLOOK_NOTICE: &str = "Outlook 需要 OAuth / Modern Auth；当前 MVP 尚未实现，暂不支持登录。";
const GOOGLE_CLIENT_ID: &str =
    "609932488435-4h4fffcvl0hcpe0u9svc8k610tstvia7.apps.googleusercontent.com";
const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_USERINFO_URL: &str = "https://openidconnect.googleapis.com/v1/userinfo";
const GOOGLE_MAIL_SCOPE: &str = "https://mail.google.com/";
const OAUTH_CALLBACK_TIMEOUT: Duration = Duration::from_secs(180);
const OAUTH_HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const OAUTH_REFRESH_MARGIN_SECONDS: u64 = 300;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AccountProvider {
    #[serde(rename = "163")]
    NetEase163,
    Gmail,
    Outlook,
    Custom,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum AccountAuthentication {
    #[default]
    Password,
    GoogleOAuth,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SmtpSecurityInput {
    #[serde(alias = "tls", alias = "implicitTls")]
    ImplicitTls,
    #[serde(alias = "starttls", alias = "startTls")]
    StartTls,
}

impl From<SmtpSecurityInput> for SmtpSecurity {
    fn from(value: SmtpSecurityInput) -> Self {
        match value {
            SmtpSecurityInput::ImplicitTls => Self::ImplicitTls,
            SmtpSecurityInput::StartTls => Self::StartTls,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct AccountMetadata {
    schema_version: u8,
    account_id: String,
    provider: AccountProvider,
    #[serde(default)]
    authentication: AccountAuthentication,
    email: String,
    imap: ServerConfig,
    smtp: ServerConfig,
    smtp_security: SmtpSecurity,
}

impl AccountMetadata {
    fn preset(provider: AccountProvider, email: String) -> Result<Self, String> {
        let (imap, smtp, smtp_security) = match provider {
            AccountProvider::NetEase163 => (
                server("imap.163.com", 993),
                server("smtp.163.com", 465),
                SmtpSecurity::ImplicitTls,
            ),
            AccountProvider::Gmail => (
                server("imap.gmail.com", 993),
                server("smtp.gmail.com", 465),
                SmtpSecurity::ImplicitTls,
            ),
            AccountProvider::Outlook => (
                server("outlook.office365.com", 993),
                server("smtp-mail.outlook.com", 587),
                SmtpSecurity::StartTls,
            ),
            AccountProvider::Custom => {
                return Err("Custom accounts require explicit IMAP and SMTP settings.".to_owned());
            }
        };
        let mut metadata = Self {
            schema_version: ACCOUNT_METADATA_VERSION,
            account_id: String::new(),
            provider,
            authentication: AccountAuthentication::Password,
            email: email.trim().to_owned(),
            imap,
            smtp,
            smtp_security,
        };
        metadata.account_id = generated_account_id(&metadata);
        Ok(metadata)
    }

    fn google(email: String) -> Result<Self, String> {
        let mut metadata = Self::preset(AccountProvider::Gmail, email)?;
        metadata.authentication = AccountAuthentication::GoogleOAuth;
        Ok(metadata)
    }

    fn from_input(input: &ConfigureAccountRequest) -> Result<Self, String> {
        if input.provider == AccountProvider::Outlook {
            return Err(OUTLOOK_NOTICE.to_owned());
        }
        if input.provider == AccountProvider::NetEase163
            && !input
                .email
                .trim()
                .to_ascii_lowercase()
                .ends_with("@163.com")
        {
            return Err("The 163 preset requires an @163.com address.".to_owned());
        }

        if input.provider != AccountProvider::Custom {
            return Self::preset(input.provider, input.email.trim().to_owned());
        }

        let imap_host = required_text(input.imap_host.as_deref(), "IMAP host")?;
        let smtp_host = required_text(input.smtp_host.as_deref(), "SMTP host")?;
        let imap_port = input
            .imap_port
            .filter(|port| *port > 0)
            .ok_or_else(|| "A valid IMAP port is required.".to_owned())?;
        let smtp_port = input
            .smtp_port
            .filter(|port| *port > 0)
            .ok_or_else(|| "A valid SMTP port is required.".to_owned())?;
        let smtp_security = input
            .smtp_security
            .map(Into::into)
            .ok_or_else(|| "SMTP security must be implicit TLS or STARTTLS.".to_owned())?;

        let mut metadata = Self {
            schema_version: ACCOUNT_METADATA_VERSION,
            account_id: String::new(),
            provider: AccountProvider::Custom,
            authentication: AccountAuthentication::Password,
            email: input.email.trim().to_owned(),
            imap: server(imap_host, imap_port),
            smtp: server(smtp_host, smtp_port),
            smtp_security,
        };
        metadata.account_id = generated_account_id(&metadata);
        Ok(metadata)
    }

    fn account_config(&self, secret: &str) -> Result<AccountConfig, String> {
        let result = match self.authentication {
            AccountAuthentication::Password => AccountConfig::new(
                self.account_id.clone(),
                self.email.clone(),
                secret,
                self.imap.clone(),
                self.smtp.clone(),
                self.smtp_security,
            ),
            AccountAuthentication::GoogleOAuth => AccountConfig::new_oauth2(
                self.account_id.clone(),
                self.email.clone(),
                secret,
                self.imap.clone(),
                self.smtp.clone(),
                self.smtp_security,
            ),
        };
        result.map_err(|_| "The account settings are invalid.".to_owned())
    }

    fn same_identity(&self, other: &Self) -> bool {
        account_identity_hash(self) == account_identity_hash(other)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct StoredAccounts {
    schema_version: u8,
    active_account_id: Option<String>,
    accounts: Vec<AccountMetadata>,
}

impl Default for StoredAccounts {
    fn default() -> Self {
        Self {
            schema_version: ACCOUNT_STORE_VERSION,
            active_account_id: None,
            accounts: Vec::new(),
        }
    }
}

impl StoredAccounts {
    fn normalize(&mut self) -> Result<(), String> {
        if self.schema_version != ACCOUNT_STORE_VERSION {
            return Err("The saved account metadata version is unsupported.".to_owned());
        }
        if self.accounts.len() > MAX_ACCOUNTS {
            return Err(
                "The saved account list exceeds Mine Mail's three-account limit.".to_owned(),
            );
        }
        let mut ids = HashSet::new();
        for account in &self.accounts {
            if account.schema_version != ACCOUNT_METADATA_VERSION
                || account.account_id.trim().is_empty()
                || !ids.insert(account.account_id.clone())
            {
                return Err("The saved account metadata is invalid.".to_owned());
            }
        }
        if self
            .active_account_id
            .as_ref()
            .is_none_or(|active| !ids.contains(active))
        {
            self.active_account_id = self
                .accounts
                .first()
                .map(|account| account.account_id.clone());
        }
        Ok(())
    }

    fn upsert_and_activate(&mut self, mut metadata: AccountMetadata) -> Result<(), String> {
        if let Some(existing) = self
            .accounts
            .iter_mut()
            .find(|existing| existing.same_identity(&metadata))
        {
            metadata.account_id = existing.account_id.clone();
            *existing = metadata.clone();
        } else {
            if self.accounts.len() >= MAX_ACCOUNTS {
                return Err("Mine Mail currently supports at most three accounts.".to_owned());
            }
            self.accounts.push(metadata.clone());
        }
        self.active_account_id = Some(metadata.account_id);
        Ok(())
    }
}

#[derive(Deserialize)]
pub(crate) struct ConfigureAccountRequest {
    provider: AccountProvider,
    email: String,
    #[serde(alias = "authorization_password", alias = "authorizationPassword")]
    secret: String,
    #[serde(alias = "imapHost")]
    imap_host: Option<String>,
    #[serde(alias = "imapPort")]
    imap_port: Option<u16>,
    #[serde(alias = "smtpHost")]
    smtp_host: Option<String>,
    #[serde(alias = "smtpPort")]
    smtp_port: Option<u16>,
    #[serde(alias = "smtpSecurity")]
    smtp_security: Option<SmtpSecurityInput>,
}

impl ConfigureAccountRequest {
    fn take_password(&mut self) -> Result<Zeroizing<String>, String> {
        let password = Zeroizing::new(std::mem::take(&mut self.secret));
        if password.trim().is_empty() {
            return Err("An authorization password or app password is required.".to_owned());
        }
        Ok(password)
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountSummaryDto {
    account_id: String,
    provider: AccountProvider,
    email: String,
    authentication: AccountAuthentication,
    backend_ready: bool,
    network_ready: bool,
    credential_available: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountStatusDto {
    pub(crate) configured: bool,
    backend_ready: bool,
    network_ready: bool,
    credential_available: bool,
    account_id: Option<String>,
    provider: Option<AccountProvider>,
    email: Option<String>,
    imap: Option<ServerConfig>,
    smtp: Option<ServerConfig>,
    smtp_security: Option<SmtpSecurity>,
    authentication: Option<AccountAuthentication>,
    authentication_notice: Option<&'static str>,
    startup_error: Option<String>,
    accounts: Vec<AccountSummaryDto>,
    active_account_id: Option<String>,
    account_count: usize,
    max_accounts: usize,
    can_add_account: bool,
    google_oauth_configured: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountPresetDto {
    id: AccountProvider,
    label: &'static str,
    imap: Option<ServerConfig>,
    smtp: Option<ServerConfig>,
    smtp_security: Option<SmtpSecurity>,
    available_in_mvp: bool,
    authentication_note: &'static str,
    disabled: bool,
    note: &'static str,
    secret_label: &'static str,
    oauth: bool,
}

pub(crate) fn account_presets() -> Vec<AccountPresetDto> {
    vec![
        preset_dto(
            AccountProvider::NetEase163,
            "163 邮箱",
            true,
            "请使用 163 邮箱生成的 IMAP / SMTP 客户端授权密码。",
            "客户端授权密码",
            false,
        ),
        preset_dto(
            AccountProvider::Gmail,
            "Gmail",
            true,
            "使用系统默认浏览器登录 Google；Mine Mail 只在系统凭据库中保存 OAuth 令牌。",
            "Google OAuth",
            true,
        ),
        preset_dto(
            AccountProvider::Outlook,
            "Outlook",
            false,
            OUTLOOK_NOTICE,
            "Modern Auth",
            false,
        ),
        AccountPresetDto {
            id: AccountProvider::Custom,
            label: "自定义 IMAP/SMTP",
            imap: None,
            smtp: None,
            smtp_security: None,
            available_in_mvp: true,
            authentication_note: "请输入服务商提供的 IMAP / SMTP 配置和对应授权凭据。",
            disabled: false,
            note: "请输入服务商提供的 IMAP / SMTP 配置。",
            secret_label: "邮箱密码或授权密码",
            oauth: false,
        },
    ]
}

fn preset_dto(
    provider: AccountProvider,
    label: &'static str,
    available_in_mvp: bool,
    authentication_note: &'static str,
    secret_label: &'static str,
    oauth: bool,
) -> AccountPresetDto {
    let metadata = AccountMetadata::preset(provider, "example@example.com".to_owned())
        .expect("built-in account presets must stay valid");
    AccountPresetDto {
        id: provider,
        label,
        imap: Some(metadata.imap),
        smtp: Some(metadata.smtp),
        smtp_security: Some(metadata.smtp_security),
        available_in_mvp,
        authentication_note,
        disabled: !available_in_mvp,
        note: authentication_note,
        secret_label,
        oauth,
    }
}

struct BackendAccountSlots {
    local: Arc<MailBackend>,
    network: Option<Arc<MailBackend>>,
    credential_available: bool,
}

struct BackendSlots {
    active_account_id: Option<String>,
    accounts: HashMap<String, BackendAccountSlots>,
}

pub(crate) struct BackendState {
    slots: RwLock<BackendSlots>,
}

impl BackendState {
    fn new(
        accounts: Vec<(String, MailBackend, Option<MailBackend>, bool)>,
        active_account_id: Option<String>,
    ) -> Self {
        let accounts = accounts
            .into_iter()
            .map(|(account_id, local, network, credential_available)| {
                (
                    account_id,
                    BackendAccountSlots {
                        local: Arc::new(local),
                        network: network.map(Arc::new),
                        credential_available,
                    },
                )
            })
            .collect();
        Self {
            slots: RwLock::new(BackendSlots {
                active_account_id,
                accounts,
            }),
        }
    }

    fn empty() -> Self {
        Self::new(Vec::new(), None)
    }

    pub(crate) fn local(&self) -> Result<Arc<MailBackend>, String> {
        let account_id = self
            .active_account_id()
            .ok_or_else(|| "No mail account is selected.".to_owned())?;
        self.local_for(&account_id)
    }

    pub(crate) fn local_for(&self, account_id: &str) -> Result<Arc<MailBackend>, String> {
        self.slots
            .read()
            .map_err(|_| "The mail backend is temporarily unavailable.".to_owned())?
            .accounts
            .get(account_id)
            .map(|slots| slots.local.clone())
            .ok_or_else(|| "The local mail database is unavailable.".to_owned())
    }

    pub(crate) fn network(&self) -> Result<Arc<MailBackend>, String> {
        let account_id = self
            .active_account_id()
            .ok_or_else(|| "No mail account is selected.".to_owned())?;
        self.network_for(&account_id)
    }

    pub(crate) fn network_for(&self, account_id: &str) -> Result<Arc<MailBackend>, String> {
        self.slots
            .read()
            .map_err(|_| "The mail backend is temporarily unavailable.".to_owned())?
            .accounts
            .get(account_id)
            .and_then(|slots| slots.network.clone())
            .ok_or_else(|| {
                "Network mail features are unavailable until the account credential is restored."
                    .to_owned()
            })
    }

    fn replace_account(
        &self,
        account_id: String,
        local: MailBackend,
        network: MailBackend,
    ) -> Result<(), String> {
        let mut slots = self
            .slots
            .write()
            .map_err(|_| "The mail backend is temporarily unavailable.".to_owned())?;
        slots.accounts.insert(
            account_id.clone(),
            BackendAccountSlots {
                local: Arc::new(local),
                network: Some(Arc::new(network)),
                credential_available: true,
            },
        );
        slots.active_account_id = Some(account_id);
        Ok(())
    }

    fn replace_network(
        &self,
        account_id: &str,
        network: MailBackend,
        credential_available: bool,
    ) -> Result<(), String> {
        let mut slots = self
            .slots
            .write()
            .map_err(|_| "The mail backend is temporarily unavailable.".to_owned())?;
        let account = slots
            .accounts
            .get_mut(account_id)
            .ok_or_else(|| "The selected account is unavailable.".to_owned())?;
        account.network = Some(Arc::new(network));
        account.credential_available = credential_available;
        Ok(())
    }

    fn set_active(&self, account_id: &str) -> Result<(), String> {
        let mut slots = self
            .slots
            .write()
            .map_err(|_| "The mail backend is temporarily unavailable.".to_owned())?;
        if !slots.accounts.contains_key(account_id) {
            return Err("The selected account is unavailable.".to_owned());
        }
        slots.active_account_id = Some(account_id.to_owned());
        Ok(())
    }

    fn remove(&self, account_id: &str, active_account_id: Option<String>) -> Result<(), String> {
        let mut slots = self
            .slots
            .write()
            .map_err(|_| "The mail backend is temporarily unavailable.".to_owned())?;
        slots.accounts.remove(account_id);
        slots.active_account_id = active_account_id;
        Ok(())
    }

    pub(crate) fn active_account_id(&self) -> Option<String> {
        self.slots
            .read()
            .ok()
            .and_then(|slots| slots.active_account_id.clone())
    }

    fn readiness(&self, account_id: &str) -> (bool, bool, bool) {
        self.slots
            .read()
            .ok()
            .and_then(|slots| {
                slots.accounts.get(account_id).map(|account| {
                    (
                        true,
                        account.network.is_some(),
                        account.credential_available,
                    )
                })
            })
            .unwrap_or((false, false, false))
    }

    pub(crate) fn is_local_ready(&self) -> bool {
        self.active_account_id()
            .is_some_and(|account_id| self.readiness(&account_id).0)
    }

    fn is_network_ready(&self) -> bool {
        self.active_account_id()
            .is_some_and(|account_id| self.readiness(&account_id).1)
    }

    fn credential_available(&self) -> bool {
        self.active_account_id()
            .is_some_and(|account_id| self.readiness(&account_id).2)
    }
}

#[derive(Clone, Debug)]
struct AccountStore {
    path: PathBuf,
}

impl AccountStore {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn load(&self) -> Result<StoredAccounts, String> {
        let contents = match fs::read(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(StoredAccounts::default());
            }
            Err(_) => return Err("The saved account metadata could not be read.".to_owned()),
        };
        let value: serde_json::Value = serde_json::from_slice(&contents)
            .map_err(|_| "The saved account metadata is invalid.".to_owned())?;
        let mut stored = if value.get("accounts").is_some() {
            serde_json::from_value(value)
                .map_err(|_| "The saved account metadata is invalid.".to_owned())?
        } else {
            let metadata: AccountMetadata = serde_json::from_value(value)
                .map_err(|_| "The saved account metadata is invalid.".to_owned())?;
            StoredAccounts {
                schema_version: ACCOUNT_STORE_VERSION,
                active_account_id: Some(metadata.account_id.clone()),
                accounts: vec![metadata],
            }
        };
        stored.normalize()?;
        Ok(stored)
    }

    fn save(&self, stored: &StoredAccounts) -> Result<(), String> {
        let contents = serde_json::to_vec_pretty(stored)
            .map_err(|_| "Account metadata could not be encoded.".to_owned())?;
        let directory = self
            .path
            .parent()
            .ok_or_else(|| "The account metadata directory is unavailable.".to_owned())?;
        let mut temporary = tempfile::NamedTempFile::new_in(directory)
            .map_err(|_| "Account metadata could not be saved.".to_owned())?;
        temporary
            .write_all(&contents)
            .and_then(|_| temporary.flush())
            .and_then(|_| temporary.as_file().sync_all())
            .map_err(|_| "Account metadata could not be saved.".to_owned())?;
        temporary
            .persist(&self.path)
            .map_err(|_| "Account metadata could not be committed.".to_owned())?;
        Ok(())
    }
}

pub(crate) struct AccountRuntime {
    store: AccountStore,
    app_data: PathBuf,
    stored: RwLock<StoredAccounts>,
    startup_error: RwLock<Option<String>>,
}

impl AccountRuntime {
    pub(crate) fn open(app_data: &Path) -> Result<(Self, BackendState), String> {
        fs::create_dir_all(app_data)
            .map_err(|_| "The application data directory is unavailable.".to_owned())?;
        let store = AccountStore::new(app_data.join(ACCOUNT_METADATA_FILE));
        let (stored, mut startup_error) = match store.load() {
            Ok(stored) => (stored, None),
            Err(error) => (StoredAccounts::default(), Some(error)),
        };

        let mut backends = Vec::new();
        for metadata in &stored.accounts {
            let database_path = account_database_path(app_data, metadata);
            match open_local_backend(metadata, &database_path) {
                Ok(local) => {
                    let (network, credential_available) =
                        match load_network_backend(metadata, &database_path) {
                            Ok(result) => result,
                            Err(error) => {
                                record_startup_error(&mut startup_error, error);
                                (None, false)
                            }
                        };
                    backends.push((
                        metadata.account_id.clone(),
                        local,
                        network,
                        credential_available,
                    ));
                }
                Err(error) => record_startup_error(&mut startup_error, error),
            }
        }
        let backend_state = BackendState::new(backends, stored.active_account_id.clone());
        let runtime = Self {
            store,
            app_data: app_data.to_path_buf(),
            stored: RwLock::new(stored),
            startup_error: RwLock::new(startup_error),
        };
        Ok((runtime, backend_state))
    }

    pub(crate) fn fallback(app_data: &Path, error: String) -> (Self, BackendState) {
        (
            Self {
                store: AccountStore::new(app_data.join(ACCOUNT_METADATA_FILE)),
                app_data: app_data.to_path_buf(),
                stored: RwLock::new(StoredAccounts::default()),
                startup_error: RwLock::new(Some(error)),
            },
            BackendState::empty(),
        )
    }

    pub(crate) fn status(&self, backend: &BackendState) -> AccountStatusDto {
        let stored = self
            .stored
            .read()
            .map(|stored| stored.clone())
            .unwrap_or_default();
        let active = stored.active_account_id.as_ref().and_then(|active_id| {
            stored
                .accounts
                .iter()
                .find(|metadata| &metadata.account_id == active_id)
        });
        let accounts = stored
            .accounts
            .iter()
            .map(|metadata| {
                let (backend_ready, network_ready, credential_available) =
                    backend.readiness(&metadata.account_id);
                AccountSummaryDto {
                    account_id: metadata.account_id.clone(),
                    provider: metadata.provider,
                    email: metadata.email.clone(),
                    authentication: metadata.authentication,
                    backend_ready,
                    network_ready,
                    credential_available,
                }
            })
            .collect();

        AccountStatusDto {
            configured: !stored.accounts.is_empty(),
            backend_ready: backend.is_local_ready(),
            network_ready: backend.is_network_ready(),
            credential_available: backend.credential_available(),
            account_id: active.map(|metadata| metadata.account_id.clone()),
            provider: active.map(|metadata| metadata.provider),
            email: active.map(|metadata| metadata.email.clone()),
            imap: active.map(|metadata| metadata.imap.clone()),
            smtp: active.map(|metadata| metadata.smtp.clone()),
            smtp_security: active.map(|metadata| metadata.smtp_security),
            authentication: active.map(|metadata| metadata.authentication),
            authentication_notice: active.and_then(|metadata| {
                (metadata.provider == AccountProvider::Outlook).then_some(OUTLOOK_NOTICE)
            }),
            startup_error: self
                .startup_error
                .read()
                .ok()
                .and_then(|value| value.clone()),
            accounts,
            active_account_id: stored.active_account_id,
            account_count: stored.accounts.len(),
            max_accounts: MAX_ACCOUNTS,
            can_add_account: stored.accounts.len() < MAX_ACCOUNTS,
            google_oauth_configured: google_oauth_configured(),
        }
    }

    pub(crate) fn account_ids(&self) -> Vec<String> {
        self.stored
            .read()
            .map(|stored| {
                stored
                    .accounts
                    .iter()
                    .map(|metadata| metadata.account_id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) async fn configure(
        &self,
        backend_state: &BackendState,
        mut input: ConfigureAccountRequest,
    ) -> Result<(AccountStatusDto, bool), String> {
        let password = input.take_password()?;
        let mut metadata = AccountMetadata::from_input(&input)?;
        let previous_stored = self
            .stored
            .read()
            .map_err(|_| "Account state is temporarily unavailable.".to_owned())?
            .clone();
        if let Some(existing) = previous_stored
            .accounts
            .iter()
            .find(|existing| existing.same_identity(&metadata))
        {
            metadata.account_id = existing.account_id.clone();
        }
        let database_path = account_database_path(&self.app_data, &metadata);
        let local_backend = open_local_backend(&metadata, &database_path)?;
        let network_backend = open_backend(&metadata, &database_path, password.as_str())?;
        verify_connections(&network_backend).await?;

        let entry = keyring_entry(&metadata)?;
        let previous_credential = read_previous_credential(&entry)?;
        entry
            .set_password(password.as_str())
            .map_err(|_| "The OS credential store could not save this account.".to_owned())?;

        let mut next_stored = previous_stored.clone();
        if let Err(error) = next_stored.upsert_and_activate(metadata.clone()) {
            let _ = restore_previous_credential(&entry, previous_credential.as_ref());
            return Err(error);
        }
        if let Err(error) = self.store.save(&next_stored) {
            if restore_previous_credential(&entry, previous_credential.as_ref()).is_err() {
                return Err(format!(
                    "{error} The previous OS credential could not be restored."
                ));
            }
            return Err(error);
        }

        let account_changed =
            previous_stored.active_account_id != Some(metadata.account_id.clone());
        *self
            .stored
            .write()
            .map_err(|_| "Account state is temporarily unavailable.".to_owned())? = next_stored;
        *self
            .startup_error
            .write()
            .map_err(|_| "Account state is temporarily unavailable.".to_owned())? = None;
        backend_state.replace_account(metadata.account_id, local_backend, network_backend)?;
        Ok((self.status(backend_state), account_changed))
    }

    pub(crate) async fn connect_google(
        &self,
        backend_state: &BackendState,
    ) -> Result<(AccountStatusDto, bool), String> {
        let client_id = google_client_id()?;
        let client_secret = google_client_secret()?;
        let oauth = authorize_google(&client_id, client_secret).await?;
        let mut metadata = AccountMetadata::google(oauth.email.clone())?;
        let previous_stored = self
            .stored
            .read()
            .map_err(|_| "Account state is temporarily unavailable.".to_owned())?
            .clone();
        if let Some(existing) = previous_stored
            .accounts
            .iter()
            .find(|existing| existing.same_identity(&metadata))
        {
            metadata.account_id = existing.account_id.clone();
        }

        let database_path = account_database_path(&self.app_data, &metadata);
        let local_backend = open_local_backend(&metadata, &database_path)?;
        let network_backend = open_backend(&metadata, &database_path, &oauth.tokens.access_token)?;
        verify_connections(&network_backend).await?;

        let entry = keyring_entry(&metadata)?;
        let previous_credential = read_previous_credential(&entry)?;
        let encoded = Zeroizing::new(
            serde_json::to_string(&oauth.tokens)
                .map_err(|_| "Google credentials could not be encoded.".to_owned())?,
        );
        entry.set_password(encoded.as_str()).map_err(|_| {
            "The OS credential store could not save Google authorization.".to_owned()
        })?;

        let mut next_stored = previous_stored.clone();
        if let Err(error) = next_stored.upsert_and_activate(metadata.clone()) {
            let _ = restore_previous_credential(&entry, previous_credential.as_ref());
            return Err(error);
        }
        if let Err(error) = self.store.save(&next_stored) {
            if restore_previous_credential(&entry, previous_credential.as_ref()).is_err() {
                return Err(format!(
                    "{error} The previous OS credential could not be restored."
                ));
            }
            return Err(error);
        }

        let account_changed =
            previous_stored.active_account_id != Some(metadata.account_id.clone());
        *self
            .stored
            .write()
            .map_err(|_| "Account state is temporarily unavailable.".to_owned())? = next_stored;
        *self
            .startup_error
            .write()
            .map_err(|_| "Account state is temporarily unavailable.".to_owned())? = None;
        backend_state.replace_account(metadata.account_id, local_backend, network_backend)?;
        Ok((self.status(backend_state), account_changed))
    }

    pub(crate) fn switch_account(
        &self,
        backend_state: &BackendState,
        account_id: &str,
    ) -> Result<AccountStatusDto, String> {
        let mut next_stored = self
            .stored
            .read()
            .map_err(|_| "Account state is temporarily unavailable.".to_owned())?
            .clone();
        if !next_stored
            .accounts
            .iter()
            .any(|metadata| metadata.account_id == account_id)
        {
            return Err("The selected account does not exist.".to_owned());
        }
        next_stored.active_account_id = Some(account_id.to_owned());
        self.store.save(&next_stored)?;
        backend_state.set_active(account_id)?;
        *self
            .stored
            .write()
            .map_err(|_| "Account state is temporarily unavailable.".to_owned())? = next_stored;
        Ok(self.status(backend_state))
    }

    pub(crate) fn remove_account(
        &self,
        backend_state: &BackendState,
        account_id: &str,
    ) -> Result<AccountStatusDto, String> {
        let previous_stored = self
            .stored
            .read()
            .map_err(|_| "Account state is temporarily unavailable.".to_owned())?
            .clone();
        let metadata = previous_stored
            .accounts
            .iter()
            .find(|metadata| metadata.account_id == account_id)
            .cloned()
            .ok_or_else(|| "The selected account does not exist.".to_owned())?;
        let mut next_stored = previous_stored.clone();
        next_stored
            .accounts
            .retain(|metadata| metadata.account_id != account_id);
        if next_stored.active_account_id.as_deref() == Some(account_id) {
            next_stored.active_account_id = next_stored
                .accounts
                .first()
                .map(|metadata| metadata.account_id.clone());
        }
        self.store.save(&next_stored)?;

        let entry = keyring_entry(&metadata)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(_) => {
                let _ = self.store.save(&previous_stored);
                return Err("The OS credential store could not remove this account.".to_owned());
            }
        }
        backend_state.remove(account_id, next_stored.active_account_id.clone())?;
        *self
            .stored
            .write()
            .map_err(|_| "Account state is temporarily unavailable.".to_owned())? = next_stored;
        Ok(self.status(backend_state))
    }

    pub(crate) async fn refresh_oauth_backends(
        &self,
        backend_state: &BackendState,
    ) -> Result<(), String> {
        let accounts = self
            .stored
            .read()
            .map_err(|_| "Account state is temporarily unavailable.".to_owned())?
            .accounts
            .clone();
        let mut first_error = None;
        for metadata in accounts
            .iter()
            .filter(|metadata| metadata.authentication == AccountAuthentication::GoogleOAuth)
        {
            if let Err(error) = self
                .refresh_google_backend(metadata, backend_state, false)
                .await
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub(crate) async fn refresh_active_oauth_backend(
        &self,
        backend_state: &BackendState,
    ) -> Result<(), String> {
        let account_id = backend_state
            .active_account_id()
            .ok_or_else(|| "No mail account is selected.".to_owned())?;
        let metadata = self
            .stored
            .read()
            .map_err(|_| "Account state is temporarily unavailable.".to_owned())?
            .accounts
            .iter()
            .find(|metadata| metadata.account_id == account_id)
            .cloned()
            .ok_or_else(|| "The selected account does not exist.".to_owned())?;
        if metadata.authentication == AccountAuthentication::GoogleOAuth {
            self.refresh_google_backend(&metadata, backend_state, false)
                .await?;
        }
        Ok(())
    }

    async fn refresh_google_backend(
        &self,
        metadata: &AccountMetadata,
        backend_state: &BackendState,
        force: bool,
    ) -> Result<(), String> {
        let entry = keyring_entry(metadata)?;
        let encoded = Zeroizing::new(entry.get_password().map_err(|error| match error {
            keyring::Error::NoEntry => "Google authorization is missing; sign in again.".to_owned(),
            _ => "The OS credential store is unavailable.".to_owned(),
        })?);
        let mut tokens: OAuthTokenBundle = serde_json::from_str(encoded.as_str())
            .map_err(|_| "Saved Google authorization is invalid; sign in again.".to_owned())?;
        let now = unix_timestamp();
        if !force
            && tokens.expires_at_unix > now.saturating_add(OAUTH_REFRESH_MARGIN_SECONDS)
            && backend_state.network_for(&metadata.account_id).is_ok()
        {
            return Ok(());
        }
        let started = Instant::now();
        diagnostics::info(
            "oauth_refresh_started",
            DiagnosticFields::default()
                .account(&metadata.account_id)
                .operation("google_oauth_refresh"),
        );
        let client_id = google_client_id().inspect_err(|_| {
            log_oauth_refresh_failure(&metadata.account_id, started);
        })?;
        let client_secret = google_client_secret().inspect_err(|_| {
            log_oauth_refresh_failure(&metadata.account_id, started);
        })?;
        let refreshed =
            match refresh_google_tokens(&client_id, client_secret, &tokens.refresh_token).await {
                Ok(refreshed) => refreshed,
                Err(error) => {
                    log_oauth_refresh_failure(&metadata.account_id, started);
                    return Err(error);
                }
            };
        tokens.access_token.zeroize();
        tokens.access_token = refreshed.access_token;
        tokens.expires_at_unix = now.saturating_add(refreshed.expires_in);
        let encoded = Zeroizing::new(serde_json::to_string(&tokens).map_err(|_| {
            log_oauth_refresh_failure(&metadata.account_id, started);
            "Google credentials could not be encoded.".to_owned()
        })?);
        entry.set_password(encoded.as_str()).map_err(|_| {
            log_oauth_refresh_failure(&metadata.account_id, started);
            "The OS credential store could not update Google authorization.".to_owned()
        })?;
        let database_path = account_database_path(&self.app_data, metadata);
        let network =
            open_backend(metadata, &database_path, &tokens.access_token).inspect_err(|_| {
                log_oauth_refresh_failure(&metadata.account_id, started);
            })?;
        match backend_state.replace_network(&metadata.account_id, network, true) {
            Ok(()) => {
                diagnostics::info(
                    "oauth_refresh_completed",
                    DiagnosticFields::default()
                        .account(&metadata.account_id)
                        .operation("google_oauth_refresh")
                        .outcome("completed")
                        .duration(started.elapsed()),
                );
                Ok(())
            }
            Err(error) => {
                log_oauth_refresh_failure(&metadata.account_id, started);
                Err(error)
            }
        }
    }
}

fn log_oauth_refresh_failure(account_id: &str, started: Instant) {
    diagnostics::error(
        "oauth_refresh_failed",
        DiagnosticFields::default()
            .account(account_id)
            .operation("google_oauth_refresh")
            .error(DiagnosticErrorKind::Runtime)
            .duration(started.elapsed()),
    );
}

async fn verify_connections(backend: &MailBackend) -> Result<(), String> {
    let connection = backend
        .check_connections()
        .await
        .map_err(crate::safe_mail_error)?;
    match (connection.imap_ok, connection.smtp_ok) {
        (true, true) => Ok(()),
        (false, false) => Err(
            "The account was not saved because both IMAP and SMTP authentication failed."
                .to_owned(),
        ),
        (false, true) => {
            Err("The account was not saved because IMAP authentication failed.".to_owned())
        }
        (true, false) => {
            Err("The account was not saved because SMTP authentication failed.".to_owned())
        }
    }
}

fn open_local_backend(
    metadata: &AccountMetadata,
    database_path: &Path,
) -> Result<MailBackend, String> {
    let mut local_metadata = metadata.clone();
    local_metadata.authentication = AccountAuthentication::Password;
    open_backend(
        &local_metadata,
        database_path,
        LOCAL_ONLY_PLACEHOLDER_SECRET,
    )
}

fn load_network_backend(
    metadata: &AccountMetadata,
    database_path: &Path,
) -> Result<(Option<MailBackend>, bool), String> {
    if metadata.provider == AccountProvider::Outlook {
        return Err(OUTLOOK_NOTICE.to_owned());
    }
    let entry = keyring_entry(metadata)?;
    let credential = match entry.get_password() {
        Ok(credential) => Zeroizing::new(credential),
        Err(keyring::Error::NoEntry) => {
            if metadata.account_id != LEGACY_KEYRING_USERNAME {
                return Ok((None, false));
            }
            let legacy = legacy_keyring_entry()?;
            let legacy_credential = match legacy.get_password() {
                Ok(credential) => Zeroizing::new(credential),
                Err(keyring::Error::NoEntry) => return Ok((None, false)),
                Err(_) => {
                    return Err(
                        "The OS credential store is unavailable; local mail remains available."
                            .to_owned(),
                    );
                }
            };
            entry.set_password(legacy_credential.as_str()).map_err(|_| {
                "The OS credential store could not migrate this account; local mail remains available."
                    .to_owned()
            })?;
            legacy_credential
        }
        Err(_) => {
            return Err(
                "The OS credential store is unavailable; local mail remains available.".to_owned(),
            );
        }
    };

    match metadata.authentication {
        AccountAuthentication::Password => {
            open_backend(metadata, database_path, credential.as_str())
                .map(|backend| (Some(backend), true))
        }
        AccountAuthentication::GoogleOAuth => {
            let tokens: OAuthTokenBundle = serde_json::from_str(credential.as_str())
                .map_err(|_| "Saved Google authorization is invalid; sign in again.".to_owned())?;
            if tokens.expires_at_unix <= unix_timestamp().saturating_add(60) {
                Ok((None, true))
            } else {
                open_backend(metadata, database_path, &tokens.access_token)
                    .map(|backend| (Some(backend), true))
            }
        }
    }
}

fn open_backend(
    metadata: &AccountMetadata,
    database_path: &Path,
    secret: &str,
) -> Result<MailBackend, String> {
    let config = metadata.account_config(secret)?;
    let backend = MailBackend::open(config, database_path)
        .map_err(|_| "The local mail database could not be opened.".to_owned())?;
    backend
        .initialize()
        .map_err(|_| "The local mail database could not be initialized.".to_owned())?;
    Ok(backend)
}

fn read_previous_credential(entry: &Entry) -> Result<Option<Zeroizing<String>>, String> {
    match entry.get_password() {
        Ok(password) => Ok(Some(Zeroizing::new(password))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err(
            "The existing OS credential could not be read; the account was not changed.".to_owned(),
        ),
    }
}

fn restore_previous_credential(
    entry: &Entry,
    previous: Option<&Zeroizing<String>>,
) -> Result<(), String> {
    match previous {
        Some(password) => entry.set_password(password.as_str()),
        None => match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error),
        },
    }
    .map_err(|_| "The previous OS credential could not be restored.".to_owned())
}

fn record_startup_error(slot: &mut Option<String>, error: String) {
    if slot.is_none() {
        *slot = Some(error);
    }
}

fn keyring_entry(metadata: &AccountMetadata) -> Result<Entry, String> {
    let username = keyring_username(metadata);
    Entry::new(KEYRING_SERVICE, &username)
        .map_err(|_| "The OS credential store is unavailable.".to_owned())
}

fn legacy_keyring_entry() -> Result<Entry, String> {
    Entry::new(KEYRING_SERVICE, LEGACY_KEYRING_USERNAME)
        .map_err(|_| "The OS credential store is unavailable.".to_owned())
}

fn keyring_username(metadata: &AccountMetadata) -> String {
    format!(
        "{KEYRING_USERNAME_PREFIX}{}",
        &account_identity_hash(metadata)[..24]
    )
}

fn account_database_path(app_data: &Path, metadata: &AccountMetadata) -> PathBuf {
    let hash = account_identity_hash(metadata);
    app_data.join(format!("mine-mail-{}.sqlite3", &hash[..24]))
}

fn generated_account_id(metadata: &AccountMetadata) -> String {
    format!("account-{}", &account_identity_hash(metadata)[..24])
}

fn account_identity_hash(metadata: &AccountMetadata) -> String {
    let mut digest = Sha256::new();
    digest.update(b"mine-mail-account-database-v1\0");
    digest.update(metadata.email.trim().to_ascii_lowercase().as_bytes());
    digest.update(b"\0");
    digest.update(metadata.imap.host.trim().to_ascii_lowercase().as_bytes());
    digest.update(metadata.imap.port.to_be_bytes());
    digest.update(b"\0");
    digest.update(metadata.smtp.host.trim().to_ascii_lowercase().as_bytes());
    digest.update(metadata.smtp.port.to_be_bytes());
    format!("{:x}", digest.finalize())
}

fn required_text(value: Option<&str>, field: &str) -> Result<String, String> {
    let value = value.unwrap_or_default().trim();
    if value.is_empty() {
        Err(format!("{field} is required."))
    } else {
        Ok(value.to_owned())
    }
}

fn server(host: impl Into<String>, port: u16) -> ServerConfig {
    ServerConfig {
        host: host.into(),
        port,
    }
}

#[derive(Deserialize, Serialize)]
struct OAuthTokenBundle {
    schema_version: u8,
    refresh_token: String,
    access_token: String,
    expires_at_unix: u64,
}

impl Drop for OAuthTokenBundle {
    fn drop(&mut self) {
        self.refresh_token.zeroize();
        self.access_token.zeroize();
    }
}

struct GoogleAuthorization {
    email: String,
    tokens: OAuthTokenBundle,
}

#[derive(Deserialize)]
struct GoogleTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
    #[serde(default)]
    scope: String,
}

#[derive(Deserialize)]
struct GoogleRefreshResponse {
    access_token: String,
    expires_in: u64,
}

#[derive(Deserialize)]
struct GoogleOAuthError {
    error: String,
    #[serde(default)]
    error_description: String,
}

#[derive(Deserialize)]
struct GoogleUserInfo {
    email: String,
    #[serde(default)]
    email_verified: bool,
}

fn google_client_id() -> Result<String, String> {
    if GOOGLE_CLIENT_ID.trim().is_empty() {
        Err("Google 登录尚未配置。".to_owned())
    } else {
        Ok(GOOGLE_CLIENT_ID.to_owned())
    }
}

fn google_client_secret() -> Result<&'static str, String> {
    if GOOGLE_CLIENT_SECRET.trim().is_empty() {
        Err("Google 登录配置不完整，缺少桌面 OAuth 客户端凭据。".to_owned())
    } else {
        Ok(GOOGLE_CLIENT_SECRET)
    }
}

fn google_oauth_configured() -> bool {
    google_client_id().is_ok() && google_client_secret().is_ok()
}

async fn authorize_google(
    client_id: &str,
    client_secret: &str,
) -> Result<GoogleAuthorization, String> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|_| "Google 登录本机回调无法启动。".to_owned())?;
    let port = listener
        .local_addr()
        .map_err(|_| "Google 登录本机回调地址不可用。".to_owned())?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}");
    let verifier = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = Uuid::new_v4().simple().to_string();
    let mut authorization_url =
        url::Url::parse(GOOGLE_AUTH_URL).expect("Google authorization URL is static and valid");
    authorization_url
        .query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", &format!("openid email {GOOGLE_MAIL_SCOPE}"))
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent")
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &state);

    open::that(authorization_url.as_str())
        .map_err(|_| "无法打开系统浏览器完成 Google 登录。".to_owned())?;
    let code = wait_for_oauth_callback(listener, &state).await?;
    let client = reqwest::Client::builder()
        .timeout(OAUTH_HTTP_TIMEOUT)
        .build()
        .map_err(|_| "Google 登录网络客户端无法初始化。".to_owned())?;
    let response = client
        .post(GOOGLE_TOKEN_URL)
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("code", code.as_str()),
            ("code_verifier", verifier.as_str()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect_uri.as_str()),
        ])
        .send()
        .await
        .map_err(|_| "无法连接 Google 完成登录。".to_owned())?;
    if !response.status().is_success() {
        let status = response.status().as_u16();
        let error = response.json::<GoogleOAuthError>().await.ok();
        return Err(describe_google_token_error(status, error.as_ref(), false));
    }
    let token: GoogleTokenResponse = response
        .json()
        .await
        .map_err(|_| "Google 返回了无法识别的登录结果。".to_owned())?;
    if !token
        .scope
        .split_whitespace()
        .any(|scope| scope == GOOGLE_MAIL_SCOPE)
    {
        return Err("Google 未授予 Gmail 邮件访问权限。".to_owned());
    }
    let refresh_token = token
        .refresh_token
        .ok_or_else(|| "Google 未返回离线刷新令牌，请重新授权。".to_owned())?;
    let user_info_response = client
        .get(GOOGLE_USERINFO_URL)
        .bearer_auth(&token.access_token)
        .send()
        .await
        .map_err(|_| "无法读取 Google 账户信息。".to_owned())?;
    if !user_info_response.status().is_success() {
        return Err("Google 账户信息验证失败。".to_owned());
    }
    let user_info: GoogleUserInfo = user_info_response
        .json()
        .await
        .map_err(|_| "Google 返回了无法识别的账户信息。".to_owned())?;
    if !user_info.email_verified || user_info.email.trim().is_empty() {
        return Err("Google 账户邮箱尚未验证。".to_owned());
    }
    Ok(GoogleAuthorization {
        email: user_info.email,
        tokens: OAuthTokenBundle {
            schema_version: 1,
            refresh_token,
            access_token: token.access_token,
            expires_at_unix: unix_timestamp().saturating_add(token.expires_in),
        },
    })
}

async fn wait_for_oauth_callback(
    listener: TcpListener,
    expected_state: &str,
) -> Result<String, String> {
    timeout(OAUTH_CALLBACK_TIMEOUT, async {
        let (mut stream, _) = listener
            .accept()
            .await
            .map_err(|_| "Google 登录回调连接失败。".to_owned())?;
        let mut request = Vec::with_capacity(2048);
        let mut buffer = [0_u8; 1024];
        loop {
            let count = stream
                .read(&mut buffer)
                .await
                .map_err(|_| "Google 登录回调读取失败。".to_owned())?;
            if count == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..count]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
            if request.len() > 16 * 1024 {
                return Err("Google 登录回调内容过大。".to_owned());
            }
        }
        let request = String::from_utf8(request)
            .map_err(|_| "Google 登录回调格式无效。".to_owned())?;
        let target = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .ok_or_else(|| "Google 登录回调格式无效。".to_owned())?;
        let callback = url::Url::parse(&format!("http://127.0.0.1{target}"))
            .map_err(|_| "Google 登录回调地址无效。".to_owned())?;
        let params: HashMap<String, String> = callback.query_pairs().into_owned().collect();
        let result = if params.get("state").map(String::as_str) != Some(expected_state) {
            Err("Google 登录安全校验失败，请重试。".to_owned())
        } else if let Some(error) = params.get("error") {
            Err(if error == "access_denied" {
                "你已取消 Google 登录。".to_owned()
            } else {
                "Google 登录未完成。".to_owned()
            })
        } else {
            params
                .get("code")
                .cloned()
                .ok_or_else(|| "Google 登录未返回授权码。".to_owned())
        };
        let successful = result.is_ok();
        let body = if successful {
            "<!doctype html><meta charset=\"utf-8\"><title>Mine Mail</title><p>Google 登录已完成，可以关闭此页面并返回 Mine Mail。</p>"
        } else {
            "<!doctype html><meta charset=\"utf-8\"><title>Mine Mail</title><p>Google 登录未完成，请返回 Mine Mail 重试。</p>"
        };
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{}",
            body.len(), body
        );
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.shutdown().await;
        result
    })
    .await
    .map_err(|_| "Google 登录等待超时，请重试。".to_owned())?
}

async fn refresh_google_tokens(
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> Result<GoogleRefreshResponse, String> {
    let client = reqwest::Client::builder()
        .timeout(OAUTH_HTTP_TIMEOUT)
        .build()
        .map_err(|_| "Google 登录网络客户端无法初始化。".to_owned())?;
    let response = client
        .post(GOOGLE_TOKEN_URL)
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .map_err(|_| "无法连接 Google 刷新登录。".to_owned())?;
    if !response.status().is_success() {
        let status = response.status().as_u16();
        let error = response.json::<GoogleOAuthError>().await.ok();
        return Err(describe_google_token_error(status, error.as_ref(), true));
    }
    response
        .json()
        .await
        .map_err(|_| "Google 返回了无法识别的刷新结果。".to_owned())
}

fn describe_google_token_error(
    status: u16,
    error: Option<&GoogleOAuthError>,
    refreshing: bool,
) -> String {
    let error_code = error.map(|error| error.error.as_str());
    let safe_detail = error.map(|error| error.error_description.to_ascii_lowercase());
    let explanation = match (refreshing, error_code, safe_detail.as_deref()) {
        (_, _, Some(detail)) if detail.contains("client_secret") => {
            "该 OAuth 客户端要求客户端密钥；请改用“桌面应用”类型的 Client ID"
        }
        (false, _, Some(detail)) if detail.contains("code_verifier") => {
            "Google 拒绝了 PKCE 校验；请重新发起登录"
        }
        (true, Some("invalid_grant"), _) => "登录授权已过期或被撤销，请重新登录",
        (_, Some("invalid_client"), _) => {
            "OAuth 客户端无效；请确认该 Client ID 的应用类型是“桌面应用”"
        }
        (false, Some("invalid_grant"), _) => {
            "授权码、回调地址或 PKCE 校验不匹配；请重试，并确认使用“桌面应用”类型的 Client ID"
        }
        (_, Some("redirect_uri_mismatch"), _) => {
            "本机回调地址不适用于该 OAuth 客户端；请使用“桌面应用”类型的 Client ID"
        }
        (_, Some("unauthorized_client"), _) => "该 OAuth 客户端未获准执行桌面应用登录",
        (_, Some("access_denied"), _) => "Google 账户拒绝了此次授权",
        (_, Some("invalid_request"), _) => "Google 认为 OAuth 请求参数无效",
        (true, _, _) => "登录授权已失效，请重新登录",
        (false, _, _) => "Google 拒绝了授权码交换",
    };
    let code = error_code
        .map(|code| format!("，错误码 {code}"))
        .unwrap_or_default();
    format!("{explanation}（HTTP {status}{code}）。")
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use mine_mail::{ComposeRequest, SmtpSecurity};
    use tempfile::tempdir;

    use super::{
        AccountAuthentication, AccountMetadata, AccountProvider, AccountStore, BackendState,
        GoogleOAuthError, MAX_ACCOUNTS, StoredAccounts, account_database_path,
        describe_google_token_error, google_client_id, keyring_username, open_local_backend,
    };

    #[test]
    fn google_desktop_client_id_is_embedded_for_click_to_sign_in() {
        let client_id = google_client_id().expect("embedded Google client ID");
        assert!(client_id.ends_with(".apps.googleusercontent.com"));
        assert!(!client_id.contains(char::is_whitespace));
    }

    #[test]
    fn google_token_errors_explain_misconfigured_desktop_clients_without_echoing_payloads() {
        let invalid_client = GoogleOAuthError {
            error: "invalid_client".to_owned(),
            error_description: "do not echo this server-controlled text".to_owned(),
        };
        let message = describe_google_token_error(400, Some(&invalid_client), false);
        assert!(message.contains("桌面应用"));
        assert!(message.contains("invalid_client"));

        let invalid_grant = GoogleOAuthError {
            error: "invalid_grant".to_owned(),
            error_description: String::new(),
        };
        let message = describe_google_token_error(400, Some(&invalid_grant), false);
        assert!(message.contains("PKCE"));
        assert!(message.contains("invalid_grant"));

        let secret_required = GoogleOAuthError {
            error: "invalid_request".to_owned(),
            error_description: "client_secret is missing".to_owned(),
        };
        let message = describe_google_token_error(400, Some(&secret_required), false);
        assert!(message.contains("桌面应用"));
        assert!(!message.contains("client_secret is missing"));
    }

    #[test]
    fn built_in_presets_match_the_mvp_contract() {
        let gmail = AccountMetadata::preset(AccountProvider::Gmail, "demo@gmail.com".to_owned())
            .expect("Gmail preset");
        assert_eq!(gmail.imap.host, "imap.gmail.com");
        assert_eq!(gmail.smtp.port, 465);
        assert_eq!(gmail.smtp_security, SmtpSecurity::ImplicitTls);
        assert_eq!(gmail.authentication, AccountAuthentication::Password);

        let oauth = AccountMetadata::google("demo@gmail.com".to_owned()).expect("Google OAuth");
        assert_eq!(oauth.authentication, AccountAuthentication::GoogleOAuth);
    }

    #[test]
    fn account_store_migrates_single_account_and_contains_no_secrets() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("account.json");
        let store = AccountStore::new(path.clone());
        let mut metadata =
            AccountMetadata::preset(AccountProvider::NetEase163, "demo@163.com".to_owned())
                .expect("163 preset");
        metadata.account_id = "primary".to_owned();
        std::fs::write(&path, serde_json::to_vec(&metadata).unwrap()).unwrap();

        let migrated = store.load().expect("load legacy metadata");
        assert_eq!(migrated.accounts, vec![metadata]);
        store.save(&migrated).expect("save collection");
        let contents = std::fs::read_to_string(path).expect("metadata contents");
        assert!(!contents.contains("authorization_password"));
        assert!(!contents.contains("not-a-real-secret"));
        assert!(!contents.contains("access_token"));
        assert!(!contents.contains("refresh_token"));
    }

    #[test]
    fn stored_accounts_enforce_three_account_limit_and_keep_stable_ids() {
        let mut stored = StoredAccounts::default();
        for index in 0..MAX_ACCOUNTS {
            stored
                .upsert_and_activate(
                    AccountMetadata::preset(
                        AccountProvider::Gmail,
                        format!("user{index}@gmail.com"),
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        assert!(
            stored
                .upsert_and_activate(
                    AccountMetadata::preset(AccountProvider::Gmail, "fourth@gmail.com".to_owned(),)
                        .unwrap(),
                )
                .is_err()
        );
        let first_id = stored.accounts[0].account_id.clone();
        stored
            .upsert_and_activate(AccountMetadata::google("user0@gmail.com".to_owned()).unwrap())
            .unwrap();
        assert_eq!(stored.accounts[0].account_id, first_id);
        assert_eq!(stored.accounts.len(), MAX_ACCOUNTS);
    }

    #[test]
    fn account_database_and_credentials_use_one_way_identifiers() {
        let first =
            AccountMetadata::preset(AccountProvider::NetEase163, "first@163.com".to_owned())
                .expect("first preset");
        let same = AccountMetadata::preset(AccountProvider::NetEase163, "FIRST@163.COM".to_owned())
            .expect("same preset");
        let second = AccountMetadata::preset(AccountProvider::Gmail, "second@gmail.com".to_owned())
            .expect("second preset");
        assert_eq!(keyring_username(&first), keyring_username(&same));
        assert_ne!(keyring_username(&first), keyring_username(&second));

        let path = account_database_path(std::path::Path::new("data"), &first);
        let filename = path.file_name().unwrap().to_str().unwrap();
        assert!(filename.starts_with("mine-mail-"));
        assert!(!filename.contains("first"));
    }

    #[test]
    fn local_cache_remains_writable_without_a_network_credential() {
        let directory = tempdir().expect("temporary directory");
        let metadata =
            AccountMetadata::preset(AccountProvider::NetEase163, "demo@163.com".to_owned())
                .expect("163 preset");
        let database_path = account_database_path(directory.path(), &metadata);
        let local_backend =
            open_local_backend(&metadata, &database_path).expect("local cache backend");
        let account_id = metadata.account_id.clone();
        let state = BackendState::new(
            vec![(account_id.clone(), local_backend, None, false)],
            Some(account_id),
        );

        assert!(state.is_local_ready());
        assert!(!state.is_network_ready());
        assert!(!state.credential_available());
        assert!(state.network().is_err());

        let backend = state.local().expect("local backend remains available");
        let draft = backend
            .upsert_draft(
                None,
                ComposeRequest {
                    to: vec!["recipient@example.com".to_owned()],
                    cc: vec![],
                    bcc: vec![],
                    subject: "Offline draft".to_owned(),
                    body_text: "Saved without a credential".to_owned(),
                },
            )
            .expect("save local draft");
        assert_eq!(backend.list_drafts().expect("list drafts").len(), 1);
        backend.delete_draft(&draft.id).expect("delete local draft");
    }
}
