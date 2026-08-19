use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
#[cfg(target_os = "linux")]
use gio::prelude::ProxyResolverExt;
use keyring::Entry;
use mine_mail::{
    AccountConfig, ConnectionFailure, ConnectionFailureKind, ConnectionProtocol, ConnectionReport,
    MailBackend, ServerConfig, SmtpSecurity,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Mutex as AsyncMutex,
    time::timeout,
};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

use crate::diagnostics::{
    self, ErrorKind as DiagnosticErrorKind, Fields as DiagnosticFields,
    OperationId as DiagnosticOperationId,
};

include!(concat!(env!("OUT_DIR"), "/google_oauth_config.rs"));

const ACCOUNT_METADATA_FILE: &str = "account.json";
const ACCOUNT_STORE_VERSION: u8 = 2;
const ACCOUNT_METADATA_VERSION: u8 = 1;
const MAX_ACCOUNTS: usize = 3;
const ACCOUNT_REMARK_MAX_CHARACTERS: usize = 40;
const ACCOUNT_EMAIL_MAX_CHARACTERS: usize = 254;
const ACCOUNT_SECRET_MAX_BYTES: usize = 16 * 1024;
const MAIL_SERVER_HOST_MAX_CHARACTERS: usize = 253;
const KEYRING_SERVICE: &str = "com.minemail.desktop";
const LEGACY_KEYRING_USERNAME: &str = "primary";
const KEYRING_USERNAME_PREFIX: &str = "account-";
const LOCAL_ONLY_PLACEHOLDER_SECRET: &str = "mine-mail-local-cache-only";
const OUTLOOK_NOTICE: &str =
    "Outlook 现代登录尚未支持；已缓存邮件仍可阅读，但当前不能重新连接或新建 Outlook 账户。";
const GOOGLE_CLIENT_ID: &str =
    "609932488435-4h4fffcvl0hcpe0u9svc8k610tstvia7.apps.googleusercontent.com";
const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_REVOCATION_URL: &str = "https://oauth2.googleapis.com/revoke";
const GOOGLE_USERINFO_URL: &str = "https://openidconnect.googleapis.com/v1/userinfo";
const GOOGLE_MAIL_SCOPE: &str = "https://mail.google.com/";
const OAUTH_CALLBACK_TIMEOUT: Duration = Duration::from_secs(180);
const OAUTH_HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const OAUTH_REVOCATION_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(5);
const OAUTH_REVOCATION_MAX_ATTEMPTS: u64 = 2;
#[cfg(target_os = "linux")]
const SYSTEM_PROXY_RESOLUTION_TIMEOUT: Duration = Duration::from_secs(3);
const OAUTH_REFRESH_MARGIN_SECONDS: u64 = 300;
const ARCHIVE_FOLDER_SELECTION_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_ARCHIVE_FOLDER_SELECTIONS: usize = 3 * 128;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AccountProvider {
    #[serde(rename = "163")]
    NetEase163,
    Qq,
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
    #[serde(default)]
    remark: Option<String>,
    imap: ServerConfig,
    smtp: ServerConfig,
    smtp_security: SmtpSecurity,
}

impl AccountMetadata {
    fn preset(provider: AccountProvider, email: String) -> Result<Self, String> {
        let email = email.trim();
        if email.chars().count() > ACCOUNT_EMAIL_MAX_CHARACTERS {
            return Err(format!(
                "请检查输入：邮箱地址最多可输入 {ACCOUNT_EMAIL_MAX_CHARACTERS} 个字符。"
            ));
        }
        let (imap, smtp, smtp_security) = match provider {
            AccountProvider::NetEase163 => (
                server("imap.163.com", 993),
                server("smtp.163.com", 465),
                SmtpSecurity::ImplicitTls,
            ),
            AccountProvider::Qq => (
                server("imap.qq.com", 993),
                server("smtp.qq.com", 465),
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
                return Err("请检查输入：自定义邮箱需要填写完整的 IMAP 和 SMTP 设置。".to_owned());
            }
        };
        let mut metadata = Self {
            schema_version: ACCOUNT_METADATA_VERSION,
            account_id: String::new(),
            provider,
            authentication: AccountAuthentication::Password,
            email: email.to_owned(),
            remark: None,
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
        // The authentication kind is part of the account identity, so the id
        // must be regenerated after the switch.
        metadata.account_id = generated_account_id(&metadata);
        Ok(metadata)
    }

    fn from_input(input: &ConfigureAccountRequest) -> Result<Self, String> {
        if input.provider == AccountProvider::Outlook {
            return Err(OUTLOOK_NOTICE.to_owned());
        }
        if input.email.trim().chars().count() > ACCOUNT_EMAIL_MAX_CHARACTERS {
            return Err(format!(
                "请检查输入：邮箱地址最多可输入 {ACCOUNT_EMAIL_MAX_CHARACTERS} 个字符。"
            ));
        }
        if input.provider == AccountProvider::NetEase163
            && !input
                .email
                .trim()
                .to_ascii_lowercase()
                .ends_with("@163.com")
        {
            return Err("请检查输入：163 邮箱地址必须以 @163.com 结尾。".to_owned());
        }
        if input.provider == AccountProvider::Qq
            && !input.email.trim().to_ascii_lowercase().ends_with("@qq.com")
        {
            return Err("请检查输入：QQ 邮箱地址必须以 @qq.com 结尾。".to_owned());
        }

        if input.provider != AccountProvider::Custom {
            return Self::preset(input.provider, input.email.trim().to_owned());
        }

        let imap_host = required_text(input.imap_host.as_deref(), "IMAP 服务器地址")?;
        let smtp_host = required_text(input.smtp_host.as_deref(), "SMTP 服务器地址")?;
        let imap_port = input
            .imap_port
            .filter(|port| *port > 0)
            .ok_or_else(|| "请检查输入：请输入有效的 IMAP 端口。".to_owned())?;
        let smtp_port = input
            .smtp_port
            .filter(|port| *port > 0)
            .ok_or_else(|| "请检查输入：请输入有效的 SMTP 端口。".to_owned())?;
        let smtp_security = input
            .smtp_security
            .map(Into::into)
            .ok_or_else(|| "请检查输入：SMTP 安全方式只能选择 TLS 或 STARTTLS。".to_owned())?;

        let mut metadata = Self {
            schema_version: ACCOUNT_METADATA_VERSION,
            account_id: String::new(),
            provider: AccountProvider::Custom,
            authentication: AccountAuthentication::Password,
            email: input.email.trim().to_owned(),
            remark: None,
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
        result.map_err(|_| "请检查输入：邮箱账户设置无效。".to_owned())
    }

    fn same_identity(&self, other: &Self) -> bool {
        // A password account and a Google OAuth account for the same mailbox
        // are different credentials and must never overwrite each other.
        self.authentication == other.authentication
            && account_identity_hash(self) == account_identity_hash(other)
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
            metadata.remark = existing.remark.clone();
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
        // Trim surrounding whitespace before validation and storage so an
        // accidentally pasted space does not silently break authentication.
        let password = Zeroizing::new(std::mem::take(&mut self.secret).trim().to_owned());
        if password.trim().is_empty() {
            return Err("An authorization password or app password is required.".to_owned());
        }
        if password.len() > ACCOUNT_SECRET_MAX_BYTES {
            return Err("请检查输入：授权信息过长。".to_owned());
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
    remark: Option<String>,
    authentication: AccountAuthentication,
    backend_ready: bool,
    network_ready: bool,
    credential_available: bool,
    credential_invalid: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountStatusDto {
    pub(crate) configured: bool,
    backend_ready: bool,
    network_ready: bool,
    credential_available: bool,
    credential_invalid: bool,
    account_id: Option<String>,
    provider: Option<AccountProvider>,
    email: Option<String>,
    remark: Option<String>,
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

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoveAccountRequest {
    pub(crate) account_id: String,
    #[serde(default)]
    pub(crate) revoke_google_authorization: bool,
    #[serde(default)]
    pub(crate) delete_local_data: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoveAccountResultDto {
    pub(crate) status: AccountStatusDto,
    google_authorization_revoked: bool,
    pub(crate) local_data_deleted: bool,
    pub(crate) warning: Option<String>,
    #[serde(skip)]
    pub(crate) removed_email: String,
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
            AccountProvider::Qq,
            "QQ 邮箱",
            true,
            "请使用 QQ 邮箱生成的授权码，而不是 QQ 登录密码。",
            "QQ 邮箱授权码",
            false,
        ),
        preset_dto(
            AccountProvider::Gmail,
            "Gmail",
            true,
            "请使用 Google 账户生成的应用专用密码通过 IMAP / SMTP 连接；也可继续使用 Google OAuth。",
            "Google 应用专用密码",
            true,
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
    credential_invalid: bool,
}

struct BackendSlots {
    active_account_id: Option<String>,
    accounts: HashMap<String, BackendAccountSlots>,
}

struct ArchiveFolderSelection {
    account_id: String,
    mailbox_name: String,
    expires_at: Instant,
}

pub(crate) struct BackendState {
    slots: RwLock<BackendSlots>,
    archive_folder_selections: Mutex<HashMap<String, ArchiveFolderSelection>>,
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
                        credential_invalid: false,
                    },
                )
            })
            .collect();
        let slots = BackendSlots {
            active_account_id,
            accounts,
        };
        Self::rebalance_body_cache_budgets(&slots);
        Self {
            slots: RwLock::new(slots),
            archive_folder_selections: Mutex::new(HashMap::new()),
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

    pub(crate) fn clear_archive_folder_selections(&self, account_id: &str) -> Result<(), String> {
        let mut selections = self
            .archive_folder_selections
            .lock()
            .map_err(|_| "Archive folder choices are temporarily unavailable.".to_owned())?;
        selections.retain(|_, selection| selection.account_id != account_id);
        Ok(())
    }

    pub(crate) fn register_archive_folder_selection(
        &self,
        account_id: &str,
        mailbox_name: String,
    ) -> Result<String, String> {
        let now = Instant::now();
        let mut selections = self
            .archive_folder_selections
            .lock()
            .map_err(|_| "Archive folder choices are temporarily unavailable.".to_owned())?;
        selections.retain(|_, selection| selection.expires_at > now);
        if selections.len() >= MAX_ARCHIVE_FOLDER_SELECTIONS {
            return Err("Too many Archive folder choices are pending. Please retry.".to_owned());
        }
        let selection_id = Uuid::new_v4().to_string();
        selections.insert(
            selection_id.clone(),
            ArchiveFolderSelection {
                account_id: account_id.to_owned(),
                mailbox_name,
                expires_at: now + ARCHIVE_FOLDER_SELECTION_TTL,
            },
        );
        Ok(selection_id)
    }

    pub(crate) fn resolve_archive_folder_selection(
        &self,
        account_id: &str,
        selection_id: &str,
    ) -> Result<String, String> {
        let now = Instant::now();
        let mut selections = self
            .archive_folder_selections
            .lock()
            .map_err(|_| "Archive folder choices are temporarily unavailable.".to_owned())?;
        selections.retain(|_, selection| selection.expires_at > now);
        selections
            .get(selection_id)
            .filter(|selection| selection.account_id == account_id)
            .map(|selection| selection.mailbox_name.clone())
            .ok_or_else(|| "The Archive folder choice is invalid or expired.".to_owned())
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
                credential_invalid: false,
            },
        );
        slots.active_account_id = Some(account_id);
        Self::rebalance_body_cache_budgets(&slots);
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
        let network = Arc::new(network);
        network.set_body_cache_budget_bytes(account.local.body_cache_budget_bytes());
        account.network = Some(network);
        account.credential_available = credential_available;
        account.credential_invalid = false;
        Ok(())
    }

    fn invalidate_credential(&self, account_id: &str) -> Result<(), String> {
        let mut slots = self
            .slots
            .write()
            .map_err(|_| "The mail backend is temporarily unavailable.".to_owned())?;
        let account = slots
            .accounts
            .get_mut(account_id)
            .ok_or_else(|| "The selected account is unavailable.".to_owned())?;
        account.network = None;
        account.credential_available = false;
        account.credential_invalid = true;
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
        Self::rebalance_body_cache_budgets(&slots);
        Ok(())
    }

    fn rebalance_body_cache_budgets(slots: &BackendSlots) {
        let account_count = u64::try_from(slots.accounts.len())
            .unwrap_or(u64::MAX)
            .max(1);
        let per_account_budget = crate::BODY_CACHE_TOTAL_BYTES / account_count;
        for (account_id, account) in &slots.accounts {
            account
                .local
                .set_body_cache_budget_bytes(per_account_budget);
            match account.local.enforce_body_cache_budget(None) {
                Ok(_) => diagnostics::limited_recovery(
                    "body_cache_budget_enforcement_failed",
                    "body_cache_budget_enforcement_recovered",
                    "body_cache_rebalance",
                    Some(account_id),
                ),
                Err(error) => diagnostics::limited_failure(
                    "body_cache_budget_enforcement_failed",
                    "body_cache_rebalance",
                    Some(account_id),
                    diagnostics::mail_error_kind(&error),
                ),
            }
            if let Some(network) = &account.network {
                network.set_body_cache_budget_bytes(per_account_budget);
            }
        }
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

    pub(crate) fn network_ready_for(&self, account_id: &str) -> bool {
        self.readiness(account_id).1
    }

    fn credential_invalid_for(&self, account_id: &str) -> bool {
        self.slots
            .read()
            .ok()
            .and_then(|slots| {
                slots
                    .accounts
                    .get(account_id)
                    .map(|account| account.credential_invalid)
            })
            .unwrap_or(false)
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

    fn credential_invalid(&self) -> bool {
        self.active_account_id()
            .is_some_and(|account_id| self.credential_invalid_for(&account_id))
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
    credential_gates: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
}

impl AccountRuntime {
    pub(crate) fn open(app_data: &Path) -> Result<(Self, BackendState), String> {
        fs::create_dir_all(app_data)
            .map_err(|_| "The application data directory is unavailable.".to_owned())?;
        let store = AccountStore::new(app_data.join(ACCOUNT_METADATA_FILE));
        let (stored, mut startup_error) = match store.load() {
            Ok(stored) => (stored, None),
            Err(error) => {
                diagnostics::error(
                    "account_metadata_load_failed",
                    DiagnosticFields::default()
                        .operation("account_runtime_open")
                        .error(DiagnosticErrorKind::Serialization),
                );
                (StoredAccounts::default(), Some(error))
            }
        };

        let mut backends = Vec::new();
        for metadata in &stored.accounts {
            let database_path = account_database_path(app_data, metadata);
            match open_local_backend(metadata, &database_path) {
                Ok(local) => {
                    let (network, credential_available) =
                        match load_network_backend(metadata, &database_path) {
                            Ok(result) => {
                                if !result.1 {
                                    diagnostics::warn(
                                        "account_credential_unavailable",
                                        DiagnosticFields::default()
                                            .account(&metadata.account_id)
                                            .operation("account_runtime_open")
                                            .outcome("cached_mail_only"),
                                    );
                                }
                                result
                            }
                            Err(error) => {
                                diagnostics::error(
                                    "account_network_backend_open_failed",
                                    DiagnosticFields::default()
                                        .account(&metadata.account_id)
                                        .operation("account_runtime_open")
                                        .error(DiagnosticErrorKind::Runtime),
                                );
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
                Err(error) => {
                    diagnostics::error(
                        "account_local_backend_open_failed",
                        DiagnosticFields::default()
                            .account(&metadata.account_id)
                            .operation("account_runtime_open")
                            .error(DiagnosticErrorKind::Database),
                    );
                    record_startup_error(&mut startup_error, error);
                }
            }
        }
        let backend_state = BackendState::new(backends, stored.active_account_id.clone());
        let runtime = Self {
            store,
            app_data: app_data.to_path_buf(),
            stored: RwLock::new(stored),
            startup_error: RwLock::new(startup_error),
            credential_gates: Mutex::new(HashMap::new()),
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
                credential_gates: Mutex::new(HashMap::new()),
            },
            BackendState::empty(),
        )
    }

    fn credential_gate(&self, account_id: &str) -> Result<Arc<AsyncMutex<()>>, String> {
        let mut gates = self
            .credential_gates
            .lock()
            .map_err(|_| "Credential coordination is temporarily unavailable.".to_owned())?;
        Ok(gates
            .entry(account_id.to_owned())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone())
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
                    remark: metadata.remark.clone(),
                    authentication: metadata.authentication,
                    backend_ready,
                    network_ready,
                    credential_available,
                    credential_invalid: backend.credential_invalid_for(&metadata.account_id),
                }
            })
            .collect();

        AccountStatusDto {
            configured: !stored.accounts.is_empty(),
            backend_ready: backend.is_local_ready(),
            network_ready: backend.is_network_ready(),
            credential_available: backend.credential_available(),
            credential_invalid: backend.credential_invalid(),
            account_id: active.map(|metadata| metadata.account_id.clone()),
            provider: active.map(|metadata| metadata.provider),
            email: active.map(|metadata| metadata.email.clone()),
            remark: active.and_then(|metadata| metadata.remark.clone()),
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

    pub(crate) fn account_email_and_remark(
        &self,
        account_id: &str,
    ) -> Option<(String, Option<String>)> {
        self.stored.read().ok().and_then(|stored| {
            stored
                .accounts
                .iter()
                .find(|metadata| metadata.account_id == account_id)
                .map(|metadata| (metadata.email.clone(), metadata.remark.clone()))
        })
    }

    pub(crate) fn set_remark(
        &self,
        backend_state: &BackendState,
        account_id: &str,
        remark: &str,
    ) -> Result<AccountStatusDto, String> {
        let remark = normalize_account_remark(remark)?;
        let mut next_stored = self
            .stored
            .read()
            .map_err(|_| "Account state is temporarily unavailable.".to_owned())?
            .clone();
        let account = next_stored
            .accounts
            .iter_mut()
            .find(|metadata| metadata.account_id == account_id)
            .ok_or_else(|| "The selected account does not exist.".to_owned())?;
        account.remark = remark;
        self.store.save(&next_stored)?;
        *self
            .stored
            .write()
            .map_err(|_| "Account state is temporarily unavailable.".to_owned())? = next_stored;
        Ok(self.status(backend_state))
    }

    pub(crate) async fn configure(
        &self,
        backend_state: &BackendState,
        mut input: ConfigureAccountRequest,
    ) -> Result<(AccountStatusDto, bool), String> {
        let password = input.take_password().inspect_err(|_| {
            log_account_operation_failure(
                "account_configuration_failed",
                "configure_account",
                "input_validation",
                None,
                DiagnosticErrorKind::Validation,
            );
        })?;
        let mut metadata = AccountMetadata::from_input(&input).inspect_err(|_| {
            log_account_operation_failure(
                "account_configuration_failed",
                "configure_account",
                "account_validation",
                None,
                DiagnosticErrorKind::Validation,
            );
        })?;
        let previous_stored = self
            .stored
            .read()
            .map_err(|_| "Account state is temporarily unavailable.".to_owned())
            .inspect_err(|_| {
                log_account_operation_failure(
                    "account_configuration_failed",
                    "configure_account",
                    "account_state_read",
                    Some(&metadata.account_id),
                    DiagnosticErrorKind::Runtime,
                );
            })?
            .clone();
        let account_added = !previous_stored
            .accounts
            .iter()
            .any(|existing| existing.same_identity(&metadata));
        if let Some(existing) = previous_stored
            .accounts
            .iter()
            .find(|existing| existing.same_identity(&metadata))
        {
            metadata.account_id = existing.account_id.clone();
        }
        let database_path = account_database_path(&self.app_data, &metadata);
        let local_backend_result = if account_added {
            open_local_backend(&metadata, &database_path)
        } else {
            open_local_backend_without_outbox_recovery(&metadata, &database_path)
        };
        let local_backend = local_backend_result.inspect_err(|_| {
            log_account_operation_failure(
                "account_configuration_failed",
                "configure_account",
                "local_database_open",
                Some(&metadata.account_id),
                DiagnosticErrorKind::Database,
            );
        })?;
        let network_backend =
            open_backend_without_outbox_recovery(&metadata, &database_path, password.as_str())
                .inspect_err(|_| {
                    log_account_operation_failure(
                        "account_configuration_failed",
                        "configure_account",
                        "network_backend_open",
                        Some(&metadata.account_id),
                        DiagnosticErrorKind::Config,
                    );
                })?;
        verify_connections(&network_backend, metadata.authentication)
            .await
            .inspect_err(|_| {
                log_account_operation_failure(
                    "account_configuration_failed",
                    "configure_account",
                    "connection_verification",
                    Some(&metadata.account_id),
                    DiagnosticErrorKind::Runtime,
                );
            })?;

        let credential_gate = self
            .credential_gate(&metadata.account_id)
            .inspect_err(|_| {
                log_account_operation_failure(
                    "account_configuration_failed",
                    "configure_account",
                    "credential_gate_open",
                    Some(&metadata.account_id),
                    DiagnosticErrorKind::Runtime,
                );
            })?;
        let _credential_guard = credential_gate.lock().await;
        let entry = keyring_entry(&metadata).inspect_err(|_| {
            log_account_operation_failure(
                "account_configuration_failed",
                "configure_account",
                "credential_store_open",
                Some(&metadata.account_id),
                DiagnosticErrorKind::Runtime,
            );
        })?;
        let previous_credential = read_previous_credential(&entry).inspect_err(|_| {
            log_account_operation_failure(
                "account_configuration_failed",
                "configure_account",
                "credential_store_read",
                Some(&metadata.account_id),
                DiagnosticErrorKind::Runtime,
            );
        })?;
        entry
            .set_password(password.as_str())
            .map_err(|_| "The OS credential store could not save this account.".to_owned())
            .inspect_err(|_| {
                log_account_operation_failure(
                    "account_configuration_failed",
                    "configure_account",
                    "credential_store_write",
                    Some(&metadata.account_id),
                    DiagnosticErrorKind::Runtime,
                );
            })?;

        let mut next_stored = previous_stored.clone();
        if let Err(error) = next_stored.upsert_and_activate(metadata.clone()) {
            log_account_operation_failure(
                "account_configuration_failed",
                "configure_account",
                "account_limit_or_state",
                Some(&metadata.account_id),
                DiagnosticErrorKind::Validation,
            );
            if restore_previous_credential(&entry, previous_credential.as_ref()).is_err() {
                log_account_operation_failure(
                    "account_configuration_failed",
                    "configure_account",
                    "credential_rollback",
                    Some(&metadata.account_id),
                    DiagnosticErrorKind::Runtime,
                );
            }
            return Err(error);
        }
        if let Err(error) = self.store.save(&next_stored) {
            log_account_operation_failure(
                "account_configuration_failed",
                "configure_account",
                "account_metadata_save",
                Some(&metadata.account_id),
                DiagnosticErrorKind::Io,
            );
            if restore_previous_credential(&entry, previous_credential.as_ref()).is_err() {
                log_account_operation_failure(
                    "account_configuration_failed",
                    "configure_account",
                    "credential_rollback",
                    Some(&metadata.account_id),
                    DiagnosticErrorKind::Runtime,
                );
                return Err(format!(
                    "{error} The previous OS credential could not be restored."
                ));
            }
            return Err(error);
        }

        *self
            .stored
            .write()
            .map_err(|_| "Account state is temporarily unavailable.".to_owned())
            .inspect_err(|_| {
                log_account_operation_failure(
                    "account_configuration_failed",
                    "configure_account",
                    "account_state_commit",
                    Some(&metadata.account_id),
                    DiagnosticErrorKind::Runtime,
                );
            })? = next_stored;
        *self
            .startup_error
            .write()
            .map_err(|_| "Account state is temporarily unavailable.".to_owned())? = None;
        backend_state
            .replace_account(metadata.account_id, local_backend, network_backend)
            .inspect_err(|_| {
                log_account_operation_failure(
                    "account_configuration_failed",
                    "configure_account",
                    "backend_runtime_replace",
                    None,
                    DiagnosticErrorKind::Runtime,
                );
            })?;
        Ok((self.status(backend_state), account_added))
    }

    pub(crate) async fn begin_google_authorization(
        &self,
        operation_id: &DiagnosticOperationId,
    ) -> Result<GoogleAuthorization, String> {
        let client_id = google_client_id()?;
        let client_secret = google_client_secret()?;
        authorize_google(&client_id, client_secret, operation_id).await
    }

    pub(crate) fn google_authorization_adds_account(
        &self,
        oauth: &GoogleAuthorization,
    ) -> Result<bool, String> {
        let metadata = AccountMetadata::google(oauth.email.clone())?;
        let stored = self
            .stored
            .read()
            .map_err(|_| "Account state is temporarily unavailable.".to_owned())?;
        Ok(!stored
            .accounts
            .iter()
            .any(|existing| existing.same_identity(&metadata)))
    }

    pub(crate) async fn connect_google(
        &self,
        backend_state: &BackendState,
        oauth: GoogleAuthorization,
    ) -> Result<(AccountStatusDto, bool), String> {
        let mut metadata = AccountMetadata::google(oauth.email.clone()).inspect_err(|_| {
            log_account_operation_failure(
                "google_account_connection_failed",
                "connect_google_account",
                "account_validation",
                None,
                DiagnosticErrorKind::Validation,
            );
        })?;
        let previous_stored = self
            .stored
            .read()
            .map_err(|_| "Account state is temporarily unavailable.".to_owned())
            .inspect_err(|_| {
                log_account_operation_failure(
                    "google_account_connection_failed",
                    "connect_google_account",
                    "account_state_read",
                    Some(&metadata.account_id),
                    DiagnosticErrorKind::Runtime,
                );
            })?
            .clone();
        let account_added = !previous_stored
            .accounts
            .iter()
            .any(|existing| existing.same_identity(&metadata));
        if let Some(existing) = previous_stored
            .accounts
            .iter()
            .find(|existing| existing.same_identity(&metadata))
        {
            metadata.account_id = existing.account_id.clone();
        }

        let database_path = account_database_path(&self.app_data, &metadata);
        let local_backend_result = if account_added {
            open_local_backend(&metadata, &database_path)
        } else {
            open_local_backend_without_outbox_recovery(&metadata, &database_path)
        };
        let local_backend = local_backend_result.inspect_err(|_| {
            log_account_operation_failure(
                "google_account_connection_failed",
                "connect_google_account",
                "local_database_open",
                Some(&metadata.account_id),
                DiagnosticErrorKind::Database,
            );
        })?;
        let network_backend = open_backend_without_outbox_recovery(
            &metadata,
            &database_path,
            &oauth.tokens.access_token,
        )
        .inspect_err(|_| {
            log_account_operation_failure(
                "google_account_connection_failed",
                "connect_google_account",
                "network_backend_open",
                Some(&metadata.account_id),
                DiagnosticErrorKind::Config,
            );
        })?;
        let credential_gate = self
            .credential_gate(&metadata.account_id)
            .inspect_err(|_| {
                log_account_operation_failure(
                    "google_account_connection_failed",
                    "connect_google_account",
                    "credential_gate_open",
                    Some(&metadata.account_id),
                    DiagnosticErrorKind::Runtime,
                );
            })?;
        let _credential_guard = credential_gate.lock().await;
        let entry = keyring_entry(&metadata).inspect_err(|_| {
            log_account_operation_failure(
                "google_account_connection_failed",
                "connect_google_account",
                "credential_store_open",
                Some(&metadata.account_id),
                DiagnosticErrorKind::Runtime,
            );
        })?;
        let previous_credential = read_previous_credential(&entry).inspect_err(|_| {
            log_account_operation_failure(
                "google_account_connection_failed",
                "connect_google_account",
                "credential_store_read",
                Some(&metadata.account_id),
                DiagnosticErrorKind::Runtime,
            );
        })?;
        let encoded = Zeroizing::new(
            serde_json::to_string(&oauth.tokens)
                .map_err(|_| "Google credentials could not be encoded.".to_owned())
                .inspect_err(|_| {
                    log_account_operation_failure(
                        "google_account_connection_failed",
                        "connect_google_account",
                        "credential_encoding",
                        Some(&metadata.account_id),
                        DiagnosticErrorKind::Serialization,
                    );
                })?,
        );
        entry
            .set_password(encoded.as_str())
            .map_err(|_| "The OS credential store could not save Google authorization.".to_owned())
            .inspect_err(|_| {
                log_account_operation_failure(
                    "google_account_connection_failed",
                    "connect_google_account",
                    "credential_store_write",
                    Some(&metadata.account_id),
                    DiagnosticErrorKind::Runtime,
                );
            })?;

        let mut next_stored = previous_stored.clone();
        if let Err(error) = next_stored.upsert_and_activate(metadata.clone()) {
            log_account_operation_failure(
                "google_account_connection_failed",
                "connect_google_account",
                "account_limit_or_state",
                Some(&metadata.account_id),
                DiagnosticErrorKind::Validation,
            );
            if restore_previous_credential(&entry, previous_credential.as_ref()).is_err() {
                log_account_operation_failure(
                    "google_account_connection_failed",
                    "connect_google_account",
                    "credential_rollback",
                    Some(&metadata.account_id),
                    DiagnosticErrorKind::Runtime,
                );
            }
            return Err(error);
        }
        if let Err(error) = self.store.save(&next_stored) {
            log_account_operation_failure(
                "google_account_connection_failed",
                "connect_google_account",
                "account_metadata_save",
                Some(&metadata.account_id),
                DiagnosticErrorKind::Io,
            );
            if restore_previous_credential(&entry, previous_credential.as_ref()).is_err() {
                log_account_operation_failure(
                    "google_account_connection_failed",
                    "connect_google_account",
                    "credential_rollback",
                    Some(&metadata.account_id),
                    DiagnosticErrorKind::Runtime,
                );
                return Err(format!(
                    "{error} The previous OS credential could not be restored."
                ));
            }
            return Err(error);
        }

        *self
            .stored
            .write()
            .map_err(|_| "Account state is temporarily unavailable.".to_owned())? = next_stored;
        *self
            .startup_error
            .write()
            .map_err(|_| "Account state is temporarily unavailable.".to_owned())? = None;
        backend_state
            .replace_account(metadata.account_id, local_backend, network_backend)
            .inspect_err(|_| {
                log_account_operation_failure(
                    "google_account_connection_failed",
                    "connect_google_account",
                    "backend_runtime_replace",
                    None,
                    DiagnosticErrorKind::Runtime,
                );
            })?;
        Ok((self.status(backend_state), account_added))
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

    pub(crate) async fn revoke_google_authorization_for_removal(
        &self,
        request: &RemoveAccountRequest,
        operation_id: &DiagnosticOperationId,
    ) -> Result<bool, String> {
        if !request.revoke_google_authorization {
            return Ok(false);
        }
        let account_id = request.account_id.trim();
        let metadata = self
            .stored
            .read()
            .map_err(|_| "Account state is temporarily unavailable.".to_owned())?
            .accounts
            .iter()
            .find(|metadata| metadata.account_id == account_id)
            .cloned()
            .ok_or_else(|| "The selected account does not exist.".to_owned())?;
        if metadata.authentication != AccountAuthentication::GoogleOAuth {
            return Err(
                "Google authorization can only be revoked for a Google OAuth account.".to_owned(),
            );
        }
        let credential_gate = self.credential_gate(&metadata.account_id)?;
        let _credential_guard = credential_gate.lock().await;
        let entry = keyring_entry(&metadata)?;
        let encoded = Zeroizing::new(entry.get_password().map_err(|error| match error {
            keyring::Error::NoEntry => {
                "Google authorization is not available locally, so Mine Mail cannot revoke it. Disconnect the account here and remove Mine Mail from your Google Account permissions."
                    .to_owned()
            }
            _ => "The OS credential store could not read Google authorization; nothing was removed."
                .to_owned(),
        })?);
        let tokens: OAuthTokenBundle = serde_json::from_str(encoded.as_str()).map_err(|_| {
            "Saved Google authorization is invalid, so Mine Mail cannot revoke it. Nothing was removed."
                .to_owned()
        })?;
        revoke_google_authorization(&tokens, operation_id).await?;
        Ok(true)
    }

    pub(crate) async fn remove_account(
        &self,
        backend_state: &BackendState,
        request: &RemoveAccountRequest,
        google_authorization_revoked: bool,
    ) -> Result<RemoveAccountResultDto, String> {
        let account_id = request.account_id.trim();
        let previous_stored = self
            .stored
            .read()
            .map_err(|_| "Account state is temporarily unavailable.".to_owned())
            .inspect_err(|_| {
                log_account_operation_failure(
                    "account_removal_failed",
                    "remove_account",
                    "account_state_read",
                    Some(account_id),
                    DiagnosticErrorKind::Runtime,
                );
            })?
            .clone();
        let metadata = previous_stored
            .accounts
            .iter()
            .find(|metadata| metadata.account_id == account_id)
            .cloned()
            .ok_or_else(|| "The selected account does not exist.".to_owned())
            .inspect_err(|_| {
                log_account_operation_failure(
                    "account_removal_failed",
                    "remove_account",
                    "account_validation",
                    Some(account_id),
                    DiagnosticErrorKind::Validation,
                );
            })?;
        let is_google_oauth = metadata.authentication == AccountAuthentication::GoogleOAuth;
        if request.revoke_google_authorization && !is_google_oauth {
            log_account_operation_failure(
                "account_removal_failed",
                "remove_account",
                "authorization_validation",
                Some(account_id),
                DiagnosticErrorKind::Validation,
            );
            return Err(
                "Google authorization can only be revoked for a Google OAuth account.".to_owned(),
            );
        }
        if request.revoke_google_authorization != google_authorization_revoked {
            log_account_operation_failure(
                "account_removal_failed",
                "remove_account",
                "authorization_confirmation",
                Some(account_id),
                DiagnosticErrorKind::Validation,
            );
            return Err(
                "Google authorization must be confirmed before the account can be removed."
                    .to_owned(),
            );
        }
        let credential_gate = self
            .credential_gate(&metadata.account_id)
            .inspect_err(|_| {
                log_account_operation_failure(
                    "account_removal_failed",
                    "remove_account",
                    "credential_gate_open",
                    Some(account_id),
                    DiagnosticErrorKind::Runtime,
                );
            })?;
        let _credential_guard = credential_gate.lock().await;
        let entry = keyring_entry(&metadata).inspect_err(|_| {
            log_account_operation_failure(
                "account_removal_failed",
                "remove_account",
                "credential_store_open",
                Some(account_id),
                DiagnosticErrorKind::Runtime,
            );
        })?;

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
        if let Err(error) = self.store.save(&next_stored) {
            log_account_operation_failure(
                "account_removal_failed",
                "remove_account",
                "account_metadata_save",
                Some(account_id),
                DiagnosticErrorKind::Io,
            );
            return Err(if google_authorization_revoked {
                format!(
                    "Google authorization was revoked, but Mine Mail could not update its local account list: {error} Restart the app and retry local removal."
                )
            } else {
                error
            });
        }

        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {
                // Only legacy account metadata owns an ambiguous predecessor.
                // A new authentication-qualified account may share the same
                // identity with a legacy account using another authentication
                // kind, so deleting the bare identity entry would remove the
                // other account's credential.
                if let Some(legacy_username) = legacy_keyring_username_for_account(&metadata) {
                    let _ = Entry::new(KEYRING_SERVICE, &legacy_username)
                        .and_then(|entry| entry.delete_credential());
                }
            }
            Err(_) => {
                log_account_operation_failure(
                    "account_removal_failed",
                    "remove_account",
                    "credential_delete",
                    Some(account_id),
                    DiagnosticErrorKind::Runtime,
                );
                if self.store.save(&previous_stored).is_err() {
                    log_account_operation_failure(
                        "account_removal_failed",
                        "remove_account",
                        "account_metadata_rollback",
                        Some(account_id),
                        DiagnosticErrorKind::Io,
                    );
                }
                return Err(if google_authorization_revoked {
                    "Google authorization was revoked, but the OS credential could not be removed. The account remains listed so you can retry the local cleanup."
                        .to_owned()
                } else {
                    "The OS credential store could not remove this account.".to_owned()
                });
            }
        }
        let managed_attachment_warning = remove_managed_attachment_data_if_requested(
            backend_state,
            account_id,
            request.delete_local_data,
        );
        if managed_attachment_warning.is_some() {
            log_account_operation_failure(
                "account_removal_cleanup_failed",
                "remove_account",
                "managed_attachment_cleanup",
                Some(account_id),
                DiagnosticErrorKind::Io,
            );
        }
        if backend_state
            .remove(account_id, next_stored.active_account_id.clone())
            .is_err()
        {
            log_account_operation_failure(
                "account_removal_failed",
                "remove_account",
                "backend_runtime_remove",
                Some(account_id),
                DiagnosticErrorKind::Runtime,
            );
            return Err(
                "The account credential and saved account entry were removed, but the running interface could not finish the update. Restart Mine Mail."
                    .to_owned(),
            );
        }
        let mut stored = self
            .stored
            .write()
            .map_err(|_| {
                "The account credential and saved account entry were removed, but the running interface could not refresh. Restart Mine Mail."
                    .to_owned()
            })
            .inspect_err(|_| {
                log_account_operation_failure(
                    "account_removal_failed",
                    "remove_account",
                    "account_state_commit",
                    Some(account_id),
                    DiagnosticErrorKind::Runtime,
                );
            })?;
        *stored = next_stored;
        drop(stored);

        let (local_data_deleted, warning) = if request.delete_local_data {
            let mut warnings = managed_attachment_warning.into_iter().collect::<Vec<_>>();
            let database_path = account_database_path(&self.app_data, &metadata);
            if remove_sqlite_cache_files(&database_path).is_err() {
                log_account_operation_failure(
                    "account_removal_cleanup_failed",
                    "remove_account",
                    "local_cache_cleanup",
                    Some(account_id),
                    DiagnosticErrorKind::Io,
                );
                warnings.push("The account mail cache could not be deleted.".to_owned());
            }
            (
                warnings.is_empty(),
                (!warnings.is_empty()).then(|| warnings.join(" ")),
            )
        } else {
            (false, None)
        };
        Ok(RemoveAccountResultDto {
            status: self.status(backend_state),
            google_authorization_revoked,
            local_data_deleted,
            warning,
            removed_email: metadata.email,
        })
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
        self.refresh_oauth_backend_for(backend_state, &account_id)
            .await
    }

    pub(crate) async fn refresh_oauth_backend_for(
        &self,
        backend_state: &BackendState,
        account_id: &str,
    ) -> Result<(), String> {
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
        if !force && backend_state.credential_invalid_for(&metadata.account_id) {
            return Ok(());
        }
        let started = Instant::now();
        let credential_gate = self
            .credential_gate(&metadata.account_id)
            .inspect_err(|_| {
                log_oauth_refresh_failure(&metadata.account_id, started);
            })?;
        let _credential_guard = credential_gate.lock().await;
        let entry = keyring_entry(metadata).inspect_err(|_| {
            log_oauth_refresh_failure(&metadata.account_id, started);
        })?;
        let encoded = Zeroizing::new(entry.get_password().map_err(|error| {
            log_oauth_refresh_failure(&metadata.account_id, started);
            match error {
                keyring::Error::NoEntry => {
                    "Google authorization is missing; sign in again.".to_owned()
                }
                _ => "The OS credential store is unavailable.".to_owned(),
            }
        })?);
        let mut tokens: OAuthTokenBundle =
            serde_json::from_str(encoded.as_str()).map_err(|_| {
                log_oauth_refresh_failure(&metadata.account_id, started);
                "Saved Google authorization is invalid; sign in again.".to_owned()
            })?;
        let now = unix_timestamp();
        if !force
            && tokens.expires_at_unix > now.saturating_add(OAUTH_REFRESH_MARGIN_SECONDS)
            && backend_state.network_for(&metadata.account_id).is_ok()
        {
            diagnostics::limited_recovery(
                "oauth_refresh_failed",
                "oauth_refresh_recovered",
                "google_oauth_refresh",
                Some(&metadata.account_id),
            );
            return Ok(());
        }
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
                    if error.credential_invalid {
                        match backend_state.invalidate_credential(&metadata.account_id) {
                            Ok(()) => diagnostics::limited_recovery(
                                "credential_invalidation_failed",
                                "credential_invalidation_recovered",
                                "google_oauth_refresh",
                                Some(&metadata.account_id),
                            ),
                            Err(_) => diagnostics::limited_failure(
                                "credential_invalidation_failed",
                                "google_oauth_refresh",
                                Some(&metadata.account_id),
                                DiagnosticErrorKind::Runtime,
                            ),
                        }
                    }
                    log_oauth_refresh_failure(&metadata.account_id, started);
                    return Err(error.message);
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
            open_backend_without_outbox_recovery(metadata, &database_path, &tokens.access_token)
                .inspect_err(|_| {
                    log_oauth_refresh_failure(&metadata.account_id, started);
                })?;
        match backend_state.replace_network(&metadata.account_id, network, true) {
            Ok(()) => {
                diagnostics::limited_recovery(
                    "oauth_refresh_failed",
                    "oauth_refresh_recovered",
                    "google_oauth_refresh",
                    Some(&metadata.account_id),
                );
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

fn remove_managed_attachment_data_if_requested(
    backend_state: &BackendState,
    account_id: &str,
    delete_local_data: bool,
) -> Option<String> {
    if !delete_local_data {
        return None;
    }
    // Keep this Arc scoped to the managed-directory call. It must be dropped
    // before BackendState removes its slots and the SQLite files are deleted
    // on Windows.
    let result = backend_state
        .local_for(account_id)
        .map_err(|_| ())
        .and_then(|backend| {
            backend
                .delete_managed_attachment_data()
                .map(|_| ())
                .map_err(|_| ())
        })
        .map_err(|_| "The account managed attachments could not be deleted.".to_owned());
    result.err()
}

fn log_oauth_refresh_failure(account_id: &str, started: Instant) {
    diagnostics::limited_failure_with_fields(
        "oauth_refresh_failed",
        "google_oauth_refresh",
        DiagnosticErrorKind::Runtime,
        DiagnosticFields::default()
            .account(account_id)
            .duration(started.elapsed()),
    );
}

fn log_account_operation_failure(
    event: &'static str,
    operation: &'static str,
    stage: &'static str,
    account_id: Option<&str>,
    error_kind: DiagnosticErrorKind,
) {
    let mut fields = DiagnosticFields::default()
        .operation(operation)
        .outcome(stage)
        .error(error_kind);
    if let Some(account_id) = account_id {
        fields = fields.account(account_id);
    }
    diagnostics::error(event, fields);
}

async fn verify_connections(
    backend: &MailBackend,
    authentication: AccountAuthentication,
) -> Result<(), String> {
    let connection = backend
        .check_connections()
        .await
        .map_err(crate::safe_mail_error)?;
    connection_report_result(&connection, authentication)
}

fn connection_report_result(
    connection: &ConnectionReport,
    authentication: AccountAuthentication,
) -> Result<(), String> {
    let mut failures = Vec::new();
    if !connection.imap_ok {
        failures.push(connection_failure_message(
            ConnectionProtocol::Imap,
            connection.imap_failure.as_ref(),
            authentication,
        ));
    }
    if !connection.smtp_ok {
        failures.push(connection_failure_message(
            ConnectionProtocol::Smtp,
            connection.smtp_failure.as_ref(),
            authentication,
        ));
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!("账户未保存：{}。", failures.join("；")))
    }
}

fn connection_failure_message(
    protocol: ConnectionProtocol,
    failure: Option<&ConnectionFailure>,
    authentication: AccountAuthentication,
) -> String {
    let label = match protocol {
        ConnectionProtocol::Imap => "IMAP",
        ConnectionProtocol::Smtp => "SMTP",
    };
    let fallback = ConnectionFailure::new(protocol, ConnectionFailureKind::Server);
    let failure = failure.unwrap_or(&fallback);
    match failure.kind {
        ConnectionFailureKind::Configuration => {
            format!("{label} 配置无效，请检查服务器地址、端口和安全方式")
        }
        ConnectionFailureKind::Network => {
            format!("无法连接 {label} 服务器，请检查网络、代理或 TUN 设置后重试")
        }
        ConnectionFailureKind::Tls => {
            format!(
                "{label} TLS 安全连接失败，请检查安全方式、代理/TUN（含 Fake-IP）或安全软件后重试"
            )
        }
        ConnectionFailureKind::Authentication => {
            let status = failure
                .status_code
                .map(|code| format!("（状态码 {code}）"))
                .unwrap_or_default();
            if authentication == AccountAuthentication::GoogleOAuth {
                format!(
                    "Gmail {label} 拒绝了 OAuth 登录{status}，请检查 Google 账户安全状态后重新授权"
                )
            } else {
                format!("{label} 身份验证失败{status}，请检查邮箱地址和授权密码")
            }
        }
        ConnectionFailureKind::Server => {
            if let Some(status_code) = failure.status_code {
                format!(
                    "{label} 服务器拒绝了连接检查（状态码 {status_code}），请稍后重试或检查服务商限制"
                )
            } else {
                format!("{label} 服务器未能完成连接检查，请稍后重试")
            }
        }
    }
}

fn open_local_backend(
    metadata: &AccountMetadata,
    database_path: &Path,
) -> Result<MailBackend, String> {
    open_local_backend_with_recovery(metadata, database_path, true)
}

fn open_local_backend_without_outbox_recovery(
    metadata: &AccountMetadata,
    database_path: &Path,
) -> Result<MailBackend, String> {
    open_local_backend_with_recovery(metadata, database_path, false)
}

fn open_local_backend_with_recovery(
    metadata: &AccountMetadata,
    database_path: &Path,
    recover_outbox: bool,
) -> Result<MailBackend, String> {
    let mut local_metadata = metadata.clone();
    local_metadata.authentication = AccountAuthentication::Password;
    open_backend_with_recovery(
        &local_metadata,
        database_path,
        LOCAL_ONLY_PLACEHOLDER_SECRET,
        recover_outbox,
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
            if metadata.account_id == LEGACY_KEYRING_USERNAME {
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
                // The pre-account metadata store keeps a single shared
                // "primary" entry; after migration it is stale and must not
                // linger in the OS credential store.
                let _ = legacy.delete_credential();
                legacy_credential
            } else if uses_legacy_identity_account_id(metadata) {
                // Versions before authentication-kind separation stored the
                // credential under the bare identity name. Fall back to that
                // entry and migrate it to the kind-qualified name so existing
                // accounts keep working after upgrade.
                let legacy_username = legacy_identity_keyring_username(metadata);
                let legacy = Entry::new(KEYRING_SERVICE, &legacy_username)
                    .map_err(|_| "The OS credential store is unavailable.".to_owned())?;
                match legacy.get_password() {
                    Ok(legacy_credential) => {
                        let legacy_credential = Zeroizing::new(legacy_credential);
                        entry
                            .set_password(legacy_credential.as_str())
                            .map_err(|_| {
                                "The OS credential store could not migrate this account; local mail remains available."
                                    .to_owned()
                            })?;
                        // Once the credential is safely written under the
                        // authentication-qualified name, remove the ambiguous
                        // predecessor so another authentication kind cannot
                        // mistake the token bundle for a password (or vice versa).
                        let _ = legacy.delete_credential();
                        legacy_credential
                    }
                    Err(keyring::Error::NoEntry) => return Ok((None, false)),
                    Err(_) => {
                        return Err(
                            "The OS credential store is unavailable; local mail remains available."
                                .to_owned(),
                        );
                    }
                }
            } else {
                return Ok((None, false));
            }
        }
        Err(_) => {
            return Err(
                "The OS credential store is unavailable; local mail remains available.".to_owned(),
            );
        }
    };

    match metadata.authentication {
        AccountAuthentication::Password => {
            open_backend_without_outbox_recovery(metadata, database_path, credential.as_str())
                .map(|backend| (Some(backend), true))
        }
        AccountAuthentication::GoogleOAuth => {
            let tokens: OAuthTokenBundle = serde_json::from_str(credential.as_str())
                .map_err(|_| "Saved Google authorization is invalid; sign in again.".to_owned())?;
            if tokens.expires_at_unix <= unix_timestamp().saturating_add(60) {
                Ok((None, true))
            } else {
                open_backend_without_outbox_recovery(metadata, database_path, &tokens.access_token)
                    .map(|backend| (Some(backend), true))
            }
        }
    }
}

fn open_backend_without_outbox_recovery(
    metadata: &AccountMetadata,
    database_path: &Path,
    secret: &str,
) -> Result<MailBackend, String> {
    open_backend_with_recovery(metadata, database_path, secret, false)
}

fn open_backend_with_recovery(
    metadata: &AccountMetadata,
    database_path: &Path,
    secret: &str,
    recover_outbox: bool,
) -> Result<MailBackend, String> {
    let config = metadata.account_config(secret)?;
    let backend = MailBackend::open(config, database_path)
        .map_err(|_| "The local mail database could not be opened.".to_owned())?;
    let initialized = if recover_outbox {
        backend.initialize()
    } else {
        backend.initialize_without_outbox_recovery()
    };
    initialized.map_err(|_| "The local mail database could not be initialized.".to_owned())?;
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

fn authentication_suffix(metadata: &AccountMetadata) -> &'static str {
    match metadata.authentication {
        AccountAuthentication::Password => "pwd",
        AccountAuthentication::GoogleOAuth => "oauth",
    }
}

fn keyring_username(metadata: &AccountMetadata) -> String {
    // The stored value differs by authentication kind: a plain password versus
    // an OAuth token JSON bundle. Including the kind in the entry name keeps a
    // Password account and a Google OAuth account for the same identity from
    // overwriting each other's credentials.
    format!(
        "{KEYRING_USERNAME_PREFIX}{}-{}",
        &account_identity_hash(metadata)[..24],
        authentication_suffix(metadata),
    )
}

/// Entry name used before authentication-kind separation. Existing accounts
/// keep their credential under this name until the next successful read
/// migrates it to the kind-qualified name.
fn legacy_identity_keyring_username(metadata: &AccountMetadata) -> String {
    format!(
        "{KEYRING_USERNAME_PREFIX}{}",
        &account_identity_hash(metadata)[..24]
    )
}

fn uses_legacy_identity_account_id(metadata: &AccountMetadata) -> bool {
    metadata.account_id == legacy_identity_keyring_username(metadata)
}

fn legacy_keyring_username_for_account(metadata: &AccountMetadata) -> Option<String> {
    if metadata.account_id == LEGACY_KEYRING_USERNAME {
        Some(LEGACY_KEYRING_USERNAME.to_owned())
    } else if uses_legacy_identity_account_id(metadata) {
        Some(legacy_identity_keyring_username(metadata))
    } else {
        None
    }
}

fn account_database_path(app_data: &Path, metadata: &AccountMetadata) -> PathBuf {
    // Versions through v1.1.1 derived the cache filename only from the
    // identity hash, including the original `primary` account. Preserve that
    // exact filename for legacy metadata. New authentication-qualified
    // accounts use a distinct suffix. Never place the persisted account id
    // itself into a path because local metadata may be malformed or tampered.
    let identity_hash = &account_identity_hash(metadata)[..24];
    let key = if metadata.account_id == LEGACY_KEYRING_USERNAME
        || uses_legacy_identity_account_id(metadata)
    {
        identity_hash.to_owned()
    } else {
        format!("{identity_hash}-{}", authentication_suffix(metadata))
    };
    app_data.join(format!("mine-mail-{key}.sqlite3"))
}

fn sqlite_sidecar_path(database_path: &Path, suffix: &str) -> PathBuf {
    let mut path = database_path.as_os_str().to_os_string();
    path.push(suffix);
    PathBuf::from(path)
}

fn remove_sqlite_cache_files(database_path: &Path) -> Result<(), String> {
    let paths = [
        sqlite_sidecar_path(database_path, "-wal"),
        sqlite_sidecar_path(database_path, "-shm"),
        database_path.to_path_buf(),
    ];
    for path in paths {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                return Err(
                    "The account was removed, but its local mail cache could not be completely deleted. Close Mine Mail and follow the data-deletion instructions."
                        .to_owned(),
                );
            }
        }
    }
    Ok(())
}

fn generated_account_id(metadata: &AccountMetadata) -> String {
    format!(
        "{KEYRING_USERNAME_PREFIX}{}-{}",
        &account_identity_hash(metadata)[..24],
        authentication_suffix(metadata),
    )
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
        return Err(format!("请检查输入：{field}不能为空。"));
    }
    if value.chars().count() > MAIL_SERVER_HOST_MAX_CHARACTERS {
        return Err(format!(
            "请检查输入：{field}最多可输入 {MAIL_SERVER_HOST_MAX_CHARACTERS} 个字符。"
        ));
    }
    Ok(value.to_owned())
}

fn normalize_account_remark(value: &str) -> Result<Option<String>, String> {
    let remark = value.trim();
    if remark.is_empty() {
        return Ok(None);
    }
    if remark.chars().count() > ACCOUNT_REMARK_MAX_CHARACTERS {
        return Err(format!(
            "请检查输入：邮箱备注最多可输入 {ACCOUNT_REMARK_MAX_CHARACTERS} 个字符。"
        ));
    }
    if remark.chars().any(char::is_control) {
        return Err("请检查输入：邮箱备注不能包含控制字符。".to_owned());
    }
    Ok(Some(remark.to_owned()))
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

pub(crate) struct GoogleAuthorization {
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

struct GoogleRefreshFailure {
    message: String,
    credential_invalid: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GoogleTokenRevocation {
    Revoked,
    AlreadyInvalid,
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
    operation_id: &DiagnosticOperationId,
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

    let (client, proxy_mode) = google_http_client(GOOGLE_TOKEN_URL)
        .await
        .map_err(|_| "Google 登录网络客户端无法初始化。".to_owned())?;
    diagnostics::info(
        "oauth_http_client_ready",
        DiagnosticFields::default()
            .operation_id(operation_id.clone())
            .operation("google_oauth_authorization")
            .mode(proxy_mode),
    );

    let callback_started = Instant::now();
    open::that(authorization_url.as_str())
        .map_err(|_| "无法打开系统浏览器完成 Google 登录。".to_owned())?;
    let code = wait_for_oauth_callback(listener, &state).await?;
    diagnostics::info(
        "oauth_authorization_stage_completed",
        DiagnosticFields::default()
            .operation_id(operation_id.clone())
            .operation("loopback_callback")
            .outcome("completed")
            .duration(callback_started.elapsed()),
    );
    let token_exchange_started = Instant::now();
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
    diagnostics::info(
        "oauth_authorization_stage_completed",
        DiagnosticFields::default()
            .operation_id(operation_id.clone())
            .operation("token_exchange")
            .outcome("completed")
            .duration(token_exchange_started.elapsed()),
    );
    let user_info_started = Instant::now();
    let (user_info_client, user_info_proxy_mode) = google_http_client(GOOGLE_USERINFO_URL)
        .await
        .map_err(|_| "Google 登录网络客户端无法初始化。".to_owned())?;
    diagnostics::info(
        "oauth_http_client_ready",
        DiagnosticFields::default()
            .operation_id(operation_id.clone())
            .operation("google_oauth_userinfo")
            .mode(user_info_proxy_mode),
    );
    let user_info_response = user_info_client
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
    diagnostics::info(
        "oauth_authorization_stage_completed",
        DiagnosticFields::default()
            .operation_id(operation_id.clone())
            .operation("user_info")
            .outcome("completed")
            .duration(user_info_started.elapsed()),
    );
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
        match stream.write_all(response.as_bytes()).await {
            Ok(()) => diagnostics::limited_recovery(
                "oauth_callback_response_failed",
                "oauth_callback_response_recovered",
                "google_oauth_callback",
                None,
            ),
            Err(_) => diagnostics::limited_failure(
                "oauth_callback_response_failed",
                "google_oauth_callback",
                None,
                DiagnosticErrorKind::Io,
            ),
        }
        match stream.shutdown().await {
            Ok(()) => diagnostics::limited_recovery(
                "oauth_callback_shutdown_failed",
                "oauth_callback_shutdown_recovered",
                "google_oauth_callback",
                None,
            ),
            Err(_) => diagnostics::limited_failure(
                "oauth_callback_shutdown_failed",
                "google_oauth_callback",
                None,
                DiagnosticErrorKind::Io,
            ),
        }
        result
    })
    .await
    .map_err(|_| "Google 登录等待超时，请重试。".to_owned())?
}

async fn refresh_google_tokens(
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> Result<GoogleRefreshResponse, GoogleRefreshFailure> {
    let (client, proxy_mode) =
        google_http_client(GOOGLE_TOKEN_URL)
            .await
            .map_err(|_| GoogleRefreshFailure {
                message: "Google 登录网络客户端无法初始化。".to_owned(),
                credential_invalid: false,
            })?;
    diagnostics::info(
        "oauth_http_client_ready",
        DiagnosticFields::default()
            .operation("google_oauth_refresh")
            .mode(proxy_mode),
    );
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
        .map_err(|_| GoogleRefreshFailure {
            message: "无法连接 Google 刷新登录。".to_owned(),
            credential_invalid: false,
        })?;
    if !response.status().is_success() {
        let status = response.status().as_u16();
        let error = response.json::<GoogleOAuthError>().await.ok();
        return Err(google_refresh_failure(status, error.as_ref()));
    }
    response.json().await.map_err(|_| GoogleRefreshFailure {
        message: "Google 返回了无法识别的刷新结果。".to_owned(),
        credential_invalid: false,
    })
}

fn google_refresh_failure(status: u16, error: Option<&GoogleOAuthError>) -> GoogleRefreshFailure {
    GoogleRefreshFailure {
        message: describe_google_token_error(status, error, true),
        credential_invalid: error.is_some_and(|error| error.error == "invalid_grant"),
    }
}

async fn revoke_google_authorization(
    tokens: &OAuthTokenBundle,
    operation_id: &DiagnosticOperationId,
) -> Result<(), String> {
    if tokens.refresh_token.trim().is_empty() {
        return Err(
            "Saved Google authorization has no refresh token, so Mine Mail cannot revoke it. Nothing was removed."
                .to_owned(),
        );
    }

    match revoke_google_token(&tokens.refresh_token, "refresh_token", operation_id).await? {
        GoogleTokenRevocation::Revoked => Ok(()),
        GoogleTokenRevocation::AlreadyInvalid if tokens.access_token.trim().is_empty() => Ok(()),
        GoogleTokenRevocation::AlreadyInvalid => {
            match revoke_google_token(&tokens.access_token, "access_token", operation_id).await? {
                GoogleTokenRevocation::Revoked | GoogleTokenRevocation::AlreadyInvalid => Ok(()),
            }
        }
    }
}

async fn revoke_google_token(
    token: &str,
    token_kind: &'static str,
    operation_id: &DiagnosticOperationId,
) -> Result<GoogleTokenRevocation, String> {
    for attempt in 1..=OAUTH_REVOCATION_MAX_ATTEMPTS {
        let attempt_started = Instant::now();
        let (client, proxy_mode) = google_http_client(GOOGLE_REVOCATION_URL)
            .await
            .map_err(|_| "Google authorization revocation could not be initialized.".to_owned())?;
        diagnostics::info(
            "oauth_http_client_ready",
            DiagnosticFields::default()
                .operation_id(operation_id.clone())
                .operation("google_oauth_revocation")
                .trigger(token_kind)
                .mode(proxy_mode)
                .attempt(attempt),
        );
        let response = timeout(
            OAUTH_REVOCATION_ATTEMPT_TIMEOUT,
            client
                .post(GOOGLE_REVOCATION_URL)
                .form(&[("token", token)])
                .send(),
        )
        .await;
        let response = match response {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => {
                let (outcome, error_kind) = reqwest_failure_diagnostic(&error);
                diagnostics::error(
                    "oauth_revocation_attempt_failed",
                    DiagnosticFields::default()
                        .operation_id(operation_id.clone())
                        .operation("google_oauth_revocation")
                        .trigger(token_kind)
                        .mode(proxy_mode)
                        .outcome(outcome)
                        .error(error_kind)
                        .attempt(attempt)
                        .duration(attempt_started.elapsed()),
                );
                if attempt < OAUTH_REVOCATION_MAX_ATTEMPTS {
                    continue;
                }
                return Err(
                    "Mine Mail could not reach Google to revoke authorization after two attempts. Nothing was removed; check the proxy or network and retry."
                        .to_owned(),
                );
            }
            Err(_) => {
                diagnostics::error(
                    "oauth_revocation_attempt_failed",
                    DiagnosticFields::default()
                        .operation_id(operation_id.clone())
                        .operation("google_oauth_revocation")
                        .trigger(token_kind)
                        .mode(proxy_mode)
                        .outcome("timeout")
                        .error(DiagnosticErrorKind::Timeout)
                        .attempt(attempt)
                        .duration(attempt_started.elapsed()),
                );
                if attempt < OAUTH_REVOCATION_MAX_ATTEMPTS {
                    continue;
                }
                return Err(
                    "Google authorization revocation timed out after two attempts. Nothing was removed; check the proxy or network and retry."
                        .to_owned(),
                );
            }
        };
        let status = response.status().as_u16();
        let error = if response.status().is_success() {
            None
        } else {
            response.json::<GoogleOAuthError>().await.ok()
        };
        let outcome = google_revocation_response(status, error.as_ref())?;
        diagnostics::info(
            "oauth_revocation_completed",
            DiagnosticFields::default()
                .operation_id(operation_id.clone())
                .operation("google_oauth_revocation")
                .trigger(token_kind)
                .mode(proxy_mode)
                .outcome(match outcome {
                    GoogleTokenRevocation::Revoked => "revoked",
                    GoogleTokenRevocation::AlreadyInvalid => "already_invalid",
                })
                .attempt(attempt)
                .duration(attempt_started.elapsed()),
        );
        return Ok(outcome);
    }
    unreachable!("the bounded Google revocation loop always returns")
}

fn google_revocation_response(
    status: u16,
    error: Option<&GoogleOAuthError>,
) -> Result<GoogleTokenRevocation, String> {
    if (200..300).contains(&status) {
        return Ok(GoogleTokenRevocation::Revoked);
    }
    if error.is_some_and(|error| error.error == "invalid_token") {
        return Ok(GoogleTokenRevocation::AlreadyInvalid);
    }
    let error_code = error
        .map(|error| format!(", error code {}", error.error))
        .unwrap_or_default();
    Err(format!(
        "Google did not confirm authorization revocation (HTTP {status}{error_code}). Nothing was removed; retry or remove Mine Mail from your Google Account permissions."
    ))
}

fn reqwest_failure_diagnostic(error: &reqwest::Error) -> (&'static str, DiagnosticErrorKind) {
    if error.is_timeout() {
        ("timeout", DiagnosticErrorKind::Timeout)
    } else if error.is_connect() {
        ("connect_error", DiagnosticErrorKind::Runtime)
    } else if error.is_request() {
        ("request_error", DiagnosticErrorKind::Runtime)
    } else if error.is_body() {
        ("body_error", DiagnosticErrorKind::Runtime)
    } else {
        ("transport_error", DiagnosticErrorKind::Runtime)
    }
}

async fn google_http_client(
    _target_url: &'static str,
) -> Result<(reqwest::Client, &'static str), reqwest::Error> {
    let builder = reqwest::Client::builder().timeout(OAUTH_HTTP_TIMEOUT);

    #[cfg(target_os = "linux")]
    let (builder, proxy_mode) = if linux_https_proxy_environment_configured() {
        (builder, "environment_proxy")
    } else if let Some(proxy_uri) = linux_system_proxy_uri(_target_url).await {
        (
            builder.proxy(reqwest::Proxy::https(proxy_uri)?),
            "system_proxy",
        )
    } else {
        (builder, "direct")
    };

    #[cfg(not(target_os = "linux"))]
    let (builder, proxy_mode) = (builder, "automatic");

    builder.build().map(|client| (client, proxy_mode))
}

#[cfg(target_os = "linux")]
fn linux_https_proxy_environment_configured() -> bool {
    ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]
        .into_iter()
        .any(|name| std::env::var(name).is_ok_and(|value| !value.trim().is_empty()))
}

#[cfg(target_os = "linux")]
async fn linux_system_proxy_uri(target_url: &'static str) -> Option<String> {
    timeout(
        SYSTEM_PROXY_RESOLUTION_TIMEOUT,
        tokio::task::spawn_blocking(move || resolve_linux_system_proxy(target_url)),
    )
    .await
    .ok()
    .and_then(Result::ok)
    .flatten()
}

#[cfg(target_os = "linux")]
fn resolve_linux_system_proxy(target_url: &str) -> Option<String> {
    let resolver = gio::ProxyResolver::default();
    if !resolver.is_supported() {
        return None;
    }
    let candidates = resolver
        .lookup(target_url, None::<&gio::Cancellable>)
        .ok()?;
    select_linux_system_proxy(candidates)
}

#[cfg(target_os = "linux")]
fn select_linux_system_proxy<I, S>(candidates: I) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    for candidate in candidates {
        let candidate = candidate.as_ref().trim();
        if candidate.eq_ignore_ascii_case("direct://") {
            return None;
        }
        if let Some(proxy_uri) = normalize_linux_proxy_uri(candidate) {
            return Some(proxy_uri);
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn normalize_linux_proxy_uri(candidate: &str) -> Option<String> {
    let mut proxy = url::Url::parse(candidate).ok()?;
    match proxy.scheme() {
        "http" | "https" | "socks4" | "socks4a" | "socks5" | "socks5h" => {}
        "socks" => proxy.set_scheme("socks5h").ok()?,
        _ => return None,
    }
    proxy.host_str()?;
    Some(proxy.to_string())
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
    use std::time::Duration;

    use mine_mail::{
        ComposeRequest, ConnectionFailure, ConnectionFailureKind, ConnectionProtocol,
        ConnectionReport, SmtpSecurity,
    };
    use tempfile::tempdir;

    use super::{
        AccountAuthentication, AccountMetadata, AccountProvider, AccountRuntime, AccountStore,
        BackendState, ConfigureAccountRequest, GoogleAuthorization, GoogleOAuthError,
        GoogleTokenRevocation, LEGACY_KEYRING_USERNAME, MAX_ACCOUNTS, OAuthTokenBundle,
        SmtpSecurityInput, StoredAccounts, account_database_path, account_presets,
        connection_report_result, describe_google_token_error, google_client_id,
        google_refresh_failure, google_revocation_response, keyring_username,
        legacy_identity_keyring_username, legacy_keyring_username_for_account,
        normalize_account_remark, open_local_backend, remove_managed_attachment_data_if_requested,
        remove_sqlite_cache_files, sqlite_sidecar_path, uses_legacy_identity_account_id,
    };
    #[cfg(target_os = "linux")]
    use super::{normalize_linux_proxy_uri, select_linux_system_proxy};

    #[test]
    fn archive_folder_choices_are_opaque_account_bound_and_clearable() {
        let backend = BackendState::empty();
        let selection_id = backend
            .register_archive_folder_selection(
                "account-a",
                "&UXZO1mWHTvZZOQ-/MineArchive".to_owned(),
            )
            .expect("register Archive folder choice");

        assert_ne!(selection_id, "&UXZO1mWHTvZZOQ-/MineArchive");
        assert_eq!(
            backend
                .resolve_archive_folder_selection("account-a", &selection_id)
                .expect("resolve same-account choice"),
            "&UXZO1mWHTvZZOQ-/MineArchive"
        );
        assert!(
            backend
                .resolve_archive_folder_selection("account-b", &selection_id)
                .is_err()
        );
        backend
            .clear_archive_folder_selections("account-a")
            .expect("clear Archive folder choices");
        assert!(
            backend
                .resolve_archive_folder_selection("account-a", &selection_id)
                .is_err()
        );
    }

    #[test]
    fn connection_report_accepts_both_successful_protocols() {
        let report = ConnectionReport {
            imap_ok: true,
            smtp_ok: true,
            imap_failure: None,
            smtp_failure: None,
        };

        assert!(connection_report_result(&report, AccountAuthentication::GoogleOAuth).is_ok());
    }

    #[test]
    fn connection_report_identifies_smtp_tls_and_tun_failures() {
        let report = ConnectionReport {
            imap_ok: true,
            smtp_ok: false,
            imap_failure: None,
            smtp_failure: Some(ConnectionFailure::new(
                ConnectionProtocol::Smtp,
                ConnectionFailureKind::Tls,
            )),
        };

        assert_eq!(
            connection_report_result(&report, AccountAuthentication::GoogleOAuth)
                .expect_err("SMTP TLS failure must reject account setup"),
            "账户未保存：SMTP TLS 安全连接失败，请检查安全方式、代理/TUN（含 Fake-IP）或安全软件后重试。"
        );
    }

    #[test]
    fn connection_report_identifies_oauth_rejection_without_server_text() {
        let report = ConnectionReport {
            imap_ok: true,
            smtp_ok: false,
            imap_failure: None,
            smtp_failure: Some(
                ConnectionFailure::new(
                    ConnectionProtocol::Smtp,
                    ConnectionFailureKind::Authentication,
                )
                .with_status_code(Some(535)),
            ),
        };

        assert_eq!(
            connection_report_result(&report, AccountAuthentication::GoogleOAuth)
                .expect_err("OAuth rejection must reject account setup"),
            "账户未保存：Gmail SMTP 拒绝了 OAuth 登录（状态码 535），请检查 Google 账户安全状态后重新授权。"
        );
    }

    #[test]
    fn connection_report_lists_each_failed_protocol() {
        let report = ConnectionReport {
            imap_ok: false,
            smtp_ok: false,
            imap_failure: Some(ConnectionFailure::new(
                ConnectionProtocol::Imap,
                ConnectionFailureKind::Network,
            )),
            smtp_failure: Some(ConnectionFailure::new(
                ConnectionProtocol::Smtp,
                ConnectionFailureKind::Configuration,
            )),
        };

        assert_eq!(
            connection_report_result(&report, AccountAuthentication::Password)
                .expect_err("failed protocols must reject account setup"),
            "账户未保存：无法连接 IMAP 服务器，请检查网络、代理或 TUN 设置后重试；SMTP 配置无效，请检查服务器地址、端口和安全方式。"
        );
    }

    #[test]
    fn google_desktop_client_id_is_embedded_for_click_to_sign_in() {
        let client_id = google_client_id().expect("embedded Google client ID");
        assert!(client_id.ends_with(".apps.googleusercontent.com"));
        assert!(!client_id.contains(char::is_whitespace));
    }

    #[test]
    fn google_binding_distinguishes_new_accounts_from_reauthorization() {
        let directory = tempdir().expect("temporary directory");
        let (runtime, _backend) =
            AccountRuntime::fallback(directory.path(), "test fallback".to_owned());
        let authorization = GoogleAuthorization {
            email: "demo@gmail.com".to_owned(),
            tokens: OAuthTokenBundle {
                schema_version: 1,
                refresh_token: "refresh-token".to_owned(),
                access_token: "access-token".to_owned(),
                expires_at_unix: 1,
            },
        };

        assert!(
            runtime
                .google_authorization_adds_account(&authorization)
                .expect("new Google identity")
        );
        runtime
            .stored
            .write()
            .expect("account state")
            .upsert_and_activate(
                AccountMetadata::google(authorization.email.clone()).expect("Google metadata"),
            )
            .expect("store Google identity");
        assert!(
            !runtime
                .google_authorization_adds_account(&authorization)
                .expect("existing Google identity")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn credential_mutations_serialize_only_within_one_account() {
        let directory = tempdir().expect("temporary directory");
        let (runtime, _backend) =
            AccountRuntime::fallback(directory.path(), "test fallback".to_owned());
        let first = runtime.credential_gate("account-a").expect("first gate");
        let same = runtime.credential_gate("account-a").expect("same gate");
        let other = runtime.credential_gate("account-b").expect("other gate");
        let first_guard = first.lock().await;

        let mut waiting_same = Box::pin(same.lock());
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut waiting_same)
                .await
                .is_err(),
            "the same account must serialize credential writes"
        );
        let other_guard = tokio::time::timeout(Duration::from_millis(20), other.lock())
            .await
            .expect("another account must remain independent");
        drop(other_guard);
        drop(first_guard);
        let _same_guard = tokio::time::timeout(Duration::from_millis(100), waiting_same)
            .await
            .expect("the waiting credential write resumes after release");
    }

    #[test]
    fn google_revocation_is_idempotent_for_tokens_google_already_invalidated() {
        assert_eq!(
            google_revocation_response(200, None).expect("successful revocation"),
            GoogleTokenRevocation::Revoked
        );
        let invalid_token = GoogleOAuthError {
            error: "invalid_token".to_owned(),
            error_description: "server detail must stay private".to_owned(),
        };
        assert_eq!(
            google_revocation_response(400, Some(&invalid_token))
                .expect("already invalid is idempotent"),
            GoogleTokenRevocation::AlreadyInvalid
        );
        let invalid_request = GoogleOAuthError {
            error: "invalid_request".to_owned(),
            error_description: "server detail must stay private".to_owned(),
        };
        let error = google_revocation_response(400, Some(&invalid_request))
            .expect_err("malformed revocation remains an error");
        assert!(error.contains("invalid_request"));
        assert!(!error.contains("server detail"));
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

        let expired = GoogleOAuthError {
            error: "invalid_grant".to_owned(),
            error_description: "expired or revoked".to_owned(),
        };
        let failure = google_refresh_failure(400, Some(&expired));
        assert!(failure.credential_invalid);
        assert!(failure.message.contains("登录授权已过期或被撤销"));

        let transient = google_refresh_failure(503, None);
        assert!(!transient.credential_invalid);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_system_proxy_candidates_are_safely_normalized() {
        assert_eq!(
            select_linux_system_proxy(["http://127.0.0.1:7897"]),
            Some("http://127.0.0.1:7897/".to_owned())
        );
        assert_eq!(
            select_linux_system_proxy([
                "direct://",
                "http://proxy-that-must-not-be-used.example:8080",
            ]),
            None
        );
        assert_eq!(
            normalize_linux_proxy_uri("socks://127.0.0.1:1080"),
            Some("socks5h://127.0.0.1:1080".to_owned())
        );
        assert_eq!(normalize_linux_proxy_uri("file:///tmp/proxy"), None);
        assert_eq!(normalize_linux_proxy_uri("not a proxy"), None);
    }

    #[test]
    fn built_in_presets_match_the_mvp_contract() {
        let presets = account_presets();
        let providers = presets.iter().map(|preset| preset.id).collect::<Vec<_>>();
        assert_eq!(
            providers,
            vec![
                AccountProvider::NetEase163,
                AccountProvider::Qq,
                AccountProvider::Gmail,
                AccountProvider::Custom,
            ]
        );
        assert!(!providers.contains(&AccountProvider::Outlook));

        let outlook = ConfigureAccountRequest {
            provider: AccountProvider::Outlook,
            email: "legacy@outlook.com".to_owned(),
            secret: "unused".to_owned(),
            imap_host: None,
            imap_port: None,
            smtp_host: None,
            smtp_port: None,
            smtp_security: None,
        };
        let outlook_error =
            AccountMetadata::from_input(&outlook).expect_err("Outlook cannot be newly configured");
        assert!(outlook_error.contains("已缓存邮件仍可阅读"));
        assert!(outlook_error.contains("不能重新连接或新建"));

        let gmail = AccountMetadata::preset(AccountProvider::Gmail, "demo@gmail.com".to_owned())
            .expect("Gmail preset");
        assert_eq!(gmail.imap.host, "imap.gmail.com");
        assert_eq!(gmail.smtp.port, 465);
        assert_eq!(gmail.smtp_security, SmtpSecurity::ImplicitTls);
        assert_eq!(gmail.authentication, AccountAuthentication::Password);
        let gmail_preset = presets
            .iter()
            .find(|preset| preset.id == AccountProvider::Gmail)
            .expect("Gmail preset remains available");
        assert!(gmail_preset.oauth);
        assert_eq!(gmail_preset.secret_label, "Google 应用专用密码");
        assert!(gmail_preset.authentication_note.contains("IMAP / SMTP"));

        let qq = AccountMetadata::preset(AccountProvider::Qq, "demo@qq.com".to_owned())
            .expect("QQ preset");
        assert_eq!(qq.imap.host, "imap.qq.com");
        assert_eq!(qq.imap.port, 993);
        assert_eq!(qq.smtp.host, "smtp.qq.com");
        assert_eq!(qq.smtp.port, 465);
        assert_eq!(qq.smtp_security, SmtpSecurity::ImplicitTls);

        let invalid_qq = ConfigureAccountRequest {
            provider: AccountProvider::Qq,
            email: "demo@example.com".to_owned(),
            secret: "unused".to_owned(),
            imap_host: None,
            imap_port: None,
            smtp_host: None,
            smtp_port: None,
            smtp_security: None,
        };
        assert_eq!(
            AccountMetadata::from_input(&invalid_qq).expect_err("QQ requires its own domain"),
            "请检查输入：QQ 邮箱地址必须以 @qq.com 结尾。"
        );

        let oauth = AccountMetadata::google("demo@gmail.com".to_owned()).expect("Google OAuth");
        assert_eq!(oauth.authentication, AccountAuthentication::GoogleOAuth);
    }

    #[test]
    fn account_setup_rejects_oversized_text_at_the_rust_boundary() {
        let oversized_email = ConfigureAccountRequest {
            provider: AccountProvider::Gmail,
            email: format!("{}@gmail.com", "a".repeat(245)),
            secret: "unused".to_owned(),
            imap_host: None,
            imap_port: None,
            smtp_host: None,
            smtp_port: None,
            smtp_security: None,
        };
        assert!(
            AccountMetadata::from_input(&oversized_email)
                .expect_err("oversized email")
                .contains("254")
        );

        let oversized_host = ConfigureAccountRequest {
            provider: AccountProvider::Custom,
            email: "demo@example.com".to_owned(),
            secret: "unused".to_owned(),
            imap_host: Some("h".repeat(254)),
            imap_port: Some(993),
            smtp_host: Some("smtp.example.com".to_owned()),
            smtp_port: Some(465),
            smtp_security: Some(SmtpSecurityInput::ImplicitTls),
        };
        assert!(
            AccountMetadata::from_input(&oversized_host)
                .expect_err("oversized host")
                .contains("253")
        );

        let mut oversized_secret = ConfigureAccountRequest {
            provider: AccountProvider::Gmail,
            email: "demo@gmail.com".to_owned(),
            secret: "s".repeat(16 * 1024 + 1),
            imap_host: None,
            imap_port: None,
            smtp_host: None,
            smtp_port: None,
            smtp_security: None,
        };
        assert!(oversized_secret.take_password().is_err());
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
        stored.accounts[0].remark = Some("工作邮箱".to_owned());
        stored
            .upsert_and_activate(
                AccountMetadata::preset(AccountProvider::Gmail, "user0@gmail.com".to_owned())
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(stored.accounts[0].account_id, first_id);
        assert_eq!(stored.accounts[0].remark.as_deref(), Some("工作邮箱"));
        assert_eq!(stored.accounts.len(), MAX_ACCOUNTS);
    }

    #[test]
    fn google_oauth_and_password_identity_are_distinct_accounts() {
        // Same mailbox and server, different authentication kind: credentials
        // are stored separately and must never overwrite each other.
        let mut stored = StoredAccounts::default();
        stored
            .upsert_and_activate(
                AccountMetadata::preset(AccountProvider::Gmail, "user0@gmail.com".to_owned())
                    .unwrap(),
            )
            .unwrap();
        stored
            .upsert_and_activate(AccountMetadata::google("user0@gmail.com".to_owned()).unwrap())
            .unwrap();
        assert_eq!(stored.accounts.len(), 2);
        assert_ne!(stored.accounts[0].account_id, stored.accounts[1].account_id);
    }

    #[test]
    fn account_remarks_are_trimmed_bounded_and_clearable() {
        assert_eq!(
            normalize_account_remark("  工作邮箱  ").unwrap().as_deref(),
            Some("工作邮箱")
        );
        assert_eq!(normalize_account_remark("   ").unwrap(), None);
        assert!(normalize_account_remark(&"邮".repeat(41)).is_err());
        assert!(normalize_account_remark("工作\n邮箱").is_err());
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

        let password_gmail =
            AccountMetadata::preset(AccountProvider::Gmail, "same@gmail.com".to_owned())
                .expect("password Gmail metadata");
        let oauth = AccountMetadata::google("same@gmail.com".to_owned()).expect("OAuth metadata");
        assert_eq!(
            legacy_identity_keyring_username(&password_gmail),
            legacy_identity_keyring_username(&oauth)
        );
        assert_ne!(keyring_username(&password_gmail), keyring_username(&oauth));

        let mut primary = password_gmail.clone();
        primary.account_id = LEGACY_KEYRING_USERNAME.to_owned();
        assert_eq!(
            legacy_keyring_username_for_account(&primary).as_deref(),
            Some(LEGACY_KEYRING_USERNAME)
        );
        assert_eq!(legacy_keyring_username_for_account(&password_gmail), None);

        let path = account_database_path(std::path::Path::new("data"), &first);
        let filename = path.file_name().unwrap().to_str().unwrap();
        assert!(filename.starts_with("mine-mail-"));
        assert!(!filename.contains("first"));

        let mut legacy_identity = first.clone();
        legacy_identity.account_id = legacy_identity_keyring_username(&legacy_identity);
        assert!(uses_legacy_identity_account_id(&legacy_identity));
        assert_eq!(
            legacy_keyring_username_for_account(&legacy_identity).as_deref(),
            Some(legacy_identity.account_id.as_str())
        );
        let legacy_path = account_database_path(std::path::Path::new("data"), &legacy_identity);
        assert_ne!(legacy_path, path);
        assert!(
            !legacy_path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .contains("-pwd")
        );

        let mut legacy_primary = first.clone();
        legacy_primary.account_id = "primary".to_owned();
        assert_eq!(
            account_database_path(std::path::Path::new("data"), &legacy_primary),
            legacy_path
        );

        let mut malformed = first.clone();
        malformed.account_id = "account-..\\outside".to_owned();
        let malformed_path = account_database_path(std::path::Path::new("data"), &malformed);
        assert_eq!(malformed_path.parent(), Some(std::path::Path::new("data")));
        assert!(!malformed_path.to_string_lossy().contains("outside"));
    }

    #[test]
    fn deleting_local_cache_removes_sqlite_database_and_sidecars() {
        let directory = tempdir().expect("temporary directory");
        let database_path = directory.path().join("mail.sqlite3");
        let wal_path = sqlite_sidecar_path(&database_path, "-wal");
        let shm_path = sqlite_sidecar_path(&database_path, "-shm");
        std::fs::write(&database_path, b"database").expect("database");
        std::fs::write(&wal_path, b"wal").expect("wal");
        std::fs::write(&shm_path, b"shm").expect("shm");

        remove_sqlite_cache_files(&database_path).expect("remove cache");

        assert!(!database_path.exists());
        assert!(!wal_path.exists());
        assert!(!shm_path.exists());
        remove_sqlite_cache_files(&database_path).expect("missing cache is already deleted");
    }

    #[test]
    fn managed_attachment_cleanup_is_opt_in_account_scoped_and_privacy_safe_on_failure() {
        let directory = tempdir().expect("temporary directory");
        let metadata = [
            AccountMetadata::preset(AccountProvider::NetEase163, "first@163.com".to_owned())
                .expect("first account"),
            AccountMetadata::preset(AccountProvider::Gmail, "second@gmail.com".to_owned())
                .expect("second account"),
            AccountMetadata::preset(AccountProvider::Gmail, "third@gmail.com".to_owned())
                .expect("third account"),
        ];
        let accounts = metadata
            .iter()
            .map(|metadata| {
                let database_path = account_database_path(directory.path(), metadata);
                let backend =
                    open_local_backend(metadata, &database_path).expect("local cache backend");
                (metadata.account_id.clone(), backend, None, false)
            })
            .collect();
        let state = BackendState::new(accounts, Some(metadata[0].account_id.clone()));

        assert!(
            remove_managed_attachment_data_if_requested(&state, &metadata[0].account_id, false)
                .is_none()
        );
        assert!(
            state
                .local_for(&metadata[0].account_id)
                .unwrap()
                .delete_managed_attachment_data()
                .unwrap(),
            "delete=false must leave the first account directory intact"
        );

        assert!(
            remove_managed_attachment_data_if_requested(&state, &metadata[1].account_id, true)
                .is_none()
        );
        assert!(
            !state
                .local_for(&metadata[1].account_id)
                .unwrap()
                .delete_managed_attachment_data()
                .unwrap(),
            "the requested account directory was deleted exactly once"
        );
        assert!(
            state
                .local_for(&metadata[2].account_id)
                .unwrap()
                .delete_managed_attachment_data()
                .unwrap(),
            "deleting the second account must not touch the third account"
        );

        let warning =
            remove_managed_attachment_data_if_requested(&BackendState::empty(), "missing", true)
                .expect("missing backend reports a bounded warning");
        assert_eq!(
            warning,
            "The account managed attachments could not be deleted."
        );
        assert!(!warning.contains(directory.path().to_string_lossy().as_ref()));
    }

    #[test]
    fn body_cache_budget_is_shared_evenly_across_connected_accounts() {
        let directory = tempdir().expect("temporary directory");
        let metadata = [
            AccountMetadata::preset(AccountProvider::NetEase163, "first@163.com".to_owned())
                .expect("first account"),
            AccountMetadata::preset(AccountProvider::Gmail, "second@gmail.com".to_owned())
                .expect("second account"),
            AccountMetadata::preset(AccountProvider::Gmail, "third@gmail.com".to_owned())
                .expect("third account"),
        ];
        let accounts = metadata
            .iter()
            .map(|metadata| {
                let database_path = account_database_path(directory.path(), metadata);
                let backend =
                    open_local_backend(metadata, &database_path).expect("local cache backend");
                (metadata.account_id.clone(), backend, None, false)
            })
            .collect();
        let state = BackendState::new(accounts, Some(metadata[0].account_id.clone()));

        for account in &metadata {
            assert_eq!(
                state
                    .local_for(&account.account_id)
                    .expect("account cache")
                    .body_cache_budget_bytes(),
                crate::BODY_CACHE_TOTAL_BYTES / 3
            );
        }

        state
            .remove(
                &metadata[2].account_id,
                Some(metadata[0].account_id.clone()),
            )
            .expect("remove third account");
        for account in &metadata[..2] {
            assert_eq!(
                state
                    .local_for(&account.account_id)
                    .expect("remaining account cache")
                    .body_cache_budget_bytes(),
                crate::BODY_CACHE_TOTAL_BYTES / 2
            );
        }
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
                    format: Default::default(),
                    reply_context: None,
                },
            )
            .expect("save local draft");
        assert_eq!(backend.list_drafts().expect("list drafts").len(), 1);
        backend.delete_draft(&draft.id).expect("delete local draft");
    }
}
