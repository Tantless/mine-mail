use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use keyring::Entry;
use mine_mail::{AccountConfig, MailBackend, ServerConfig, SmtpSecurity};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

const ACCOUNT_METADATA_FILE: &str = "account.json";
const KEYRING_SERVICE: &str = "com.minemail.desktop";
const LEGACY_KEYRING_USERNAME: &str = "primary";
const KEYRING_USERNAME_PREFIX: &str = "account-";
const LOCAL_ONLY_PLACEHOLDER_SECRET: &str = "mine-mail-local-cache-only";
const OUTLOOK_NOTICE: &str = "Outlook 需要 OAuth / Modern Auth；当前 MVP 尚未实现，暂不支持登录。";

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AccountProvider {
    #[serde(rename = "163")]
    NetEase163,
    Gmail,
    Outlook,
    Custom,
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
        Ok(Self {
            schema_version: 1,
            account_id: "primary".to_owned(),
            provider,
            email,
            imap,
            smtp,
            smtp_security,
        })
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

        Ok(Self {
            schema_version: 1,
            account_id: "primary".to_owned(),
            provider: AccountProvider::Custom,
            email: input.email.trim().to_owned(),
            imap: server(imap_host, imap_port),
            smtp: server(smtp_host, smtp_port),
            smtp_security,
        })
    }

    fn account_config(&self, password: &str) -> Result<AccountConfig, String> {
        AccountConfig::new(
            self.account_id.clone(),
            self.email.clone(),
            password,
            self.imap.clone(),
            self.smtp.clone(),
            self.smtp_security,
        )
        .map_err(|_| "The account settings are invalid.".to_owned())
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
pub(crate) struct AccountStatusDto {
    configured: bool,
    backend_ready: bool,
    network_ready: bool,
    credential_available: bool,
    provider: Option<AccountProvider>,
    email: Option<String>,
    imap: Option<ServerConfig>,
    smtp: Option<ServerConfig>,
    smtp_security: Option<SmtpSecurity>,
    authentication_notice: Option<&'static str>,
    startup_error: Option<String>,
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
}

pub(crate) fn account_presets() -> Vec<AccountPresetDto> {
    vec![
        preset_dto(
            AccountProvider::NetEase163,
            "163 邮箱",
            true,
            "请使用 163 邮箱生成的 IMAP / SMTP 客户端授权密码。",
            "客户端授权密码",
        ),
        preset_dto(
            AccountProvider::Gmail,
            "Gmail",
            true,
            "当前 MVP 请使用 Google 应用专用密码；OAuth 尚未接入。",
            "应用专用密码",
        ),
        preset_dto(
            AccountProvider::Outlook,
            "Outlook",
            false,
            OUTLOOK_NOTICE,
            "Modern Auth",
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
        },
    ]
}

fn preset_dto(
    provider: AccountProvider,
    label: &'static str,
    available_in_mvp: bool,
    authentication_note: &'static str,
    secret_label: &'static str,
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
    }
}

struct BackendSlots {
    local: Option<Arc<MailBackend>>,
    network: Option<Arc<MailBackend>>,
    credential_available: bool,
}

pub(crate) struct BackendState {
    slots: RwLock<BackendSlots>,
}

impl BackendState {
    fn new(
        local: Option<MailBackend>,
        network: Option<MailBackend>,
        credential_available: bool,
    ) -> Self {
        Self {
            slots: RwLock::new(BackendSlots {
                local: local.map(Arc::new),
                network: network.map(Arc::new),
                credential_available,
            }),
        }
    }

    pub(crate) fn local(&self) -> Result<Arc<MailBackend>, String> {
        self.slots
            .read()
            .map_err(|_| "The mail backend is temporarily unavailable.".to_owned())?
            .local
            .clone()
            .ok_or_else(|| "The local mail database is unavailable.".to_owned())
    }

    pub(crate) fn network(&self) -> Result<Arc<MailBackend>, String> {
        self.slots
            .read()
            .map_err(|_| "The mail backend is temporarily unavailable.".to_owned())?
            .network
            .clone()
            .ok_or_else(|| {
                "Network mail features are unavailable until the account credential is restored."
                    .to_owned()
            })
    }

    fn replace(&self, local: MailBackend, network: MailBackend) -> Result<(), String> {
        *self
            .slots
            .write()
            .map_err(|_| "The mail backend is temporarily unavailable.".to_owned())? =
            BackendSlots {
                local: Some(Arc::new(local)),
                network: Some(Arc::new(network)),
                credential_available: true,
            };
        Ok(())
    }

    pub(crate) fn is_local_ready(&self) -> bool {
        self.slots
            .read()
            .map(|slots| slots.local.is_some())
            .unwrap_or(false)
    }

    fn is_network_ready(&self) -> bool {
        self.slots
            .read()
            .map(|slots| slots.network.is_some())
            .unwrap_or(false)
    }

    fn credential_available(&self) -> bool {
        self.slots
            .read()
            .map(|slots| slots.credential_available)
            .unwrap_or(false)
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

    fn load(&self) -> Result<Option<AccountMetadata>, String> {
        match fs::read(&self.path) {
            Ok(contents) => {
                let metadata: AccountMetadata = serde_json::from_slice(&contents)
                    .map_err(|_| "The saved account metadata is invalid.".to_owned())?;
                if metadata.schema_version != 1 {
                    return Err("The saved account metadata version is unsupported.".to_owned());
                }
                Ok(Some(metadata))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err("The saved account metadata could not be read.".to_owned()),
        }
    }

    fn save(&self, metadata: &AccountMetadata) -> Result<(), String> {
        let contents = serde_json::to_vec_pretty(metadata)
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
    metadata: RwLock<Option<AccountMetadata>>,
    startup_error: RwLock<Option<String>>,
}

impl AccountRuntime {
    pub(crate) fn open(
        app_data: &Path,
        legacy_credentials: Option<&Path>,
    ) -> Result<(Self, BackendState), String> {
        fs::create_dir_all(app_data)
            .map_err(|_| "The application data directory is unavailable.".to_owned())?;
        let store = AccountStore::new(app_data.join(ACCOUNT_METADATA_FILE));
        let (mut metadata, mut startup_error) = match store.load() {
            Ok(metadata) => (metadata, None),
            Err(error) => (None, Some(error)),
        };

        if startup_error.is_none()
            && metadata.is_none()
            && let Some(path) = legacy_credentials
            && path.is_file()
        {
            match migrate_legacy_163(&store, path) {
                Ok(migrated) => metadata = Some(migrated),
                Err(error) => startup_error = Some(error),
            }
        }

        let mut local_backend = None;
        let mut network_backend = None;
        let mut credential_available = false;
        if let Some(metadata) = metadata.as_ref() {
            let database_path = account_database_path(app_data, metadata);
            match open_local_backend(metadata, &database_path) {
                Ok(backend) => {
                    local_backend = Some(backend);
                    match load_network_backend(metadata, &database_path) {
                        Ok(Some(backend)) => {
                            network_backend = Some(backend);
                            credential_available = true;
                        }
                        Ok(None) => record_startup_error(
                            &mut startup_error,
                            "The saved account credential is missing; local mail remains available."
                                .to_owned(),
                        ),
                        Err(error) => record_startup_error(&mut startup_error, error),
                    }
                }
                Err(error) => record_startup_error(&mut startup_error, error),
            }
        }
        let backend_state = BackendState::new(local_backend, network_backend, credential_available);
        let runtime = Self {
            store,
            app_data: app_data.to_path_buf(),
            metadata: RwLock::new(metadata),
            startup_error: RwLock::new(startup_error),
        };
        Ok((runtime, backend_state))
    }

    pub(crate) fn fallback(app_data: &Path, error: String) -> (Self, BackendState) {
        (
            Self {
                store: AccountStore::new(app_data.join(ACCOUNT_METADATA_FILE)),
                app_data: app_data.to_path_buf(),
                metadata: RwLock::new(None),
                startup_error: RwLock::new(Some(error)),
            },
            BackendState::new(None, None, false),
        )
    }

    pub(crate) fn status(&self, backend: &BackendState) -> AccountStatusDto {
        let metadata = self.metadata.read().ok().and_then(|value| value.clone());
        let startup_error = self
            .startup_error
            .read()
            .ok()
            .and_then(|value| value.clone());

        AccountStatusDto {
            configured: metadata.is_some(),
            backend_ready: backend.is_local_ready(),
            network_ready: backend.is_network_ready(),
            credential_available: backend.credential_available(),
            provider: metadata.as_ref().map(|metadata| metadata.provider),
            email: metadata.as_ref().map(|metadata| metadata.email.clone()),
            imap: metadata.as_ref().map(|metadata| metadata.imap.clone()),
            smtp: metadata.as_ref().map(|metadata| metadata.smtp.clone()),
            smtp_security: metadata.as_ref().map(|metadata| metadata.smtp_security),
            authentication_notice: metadata.as_ref().and_then(|metadata| {
                (metadata.provider == AccountProvider::Outlook).then_some(OUTLOOK_NOTICE)
            }),
            startup_error,
        }
    }

    pub(crate) async fn configure(
        &self,
        backend_state: &BackendState,
        mut input: ConfigureAccountRequest,
    ) -> Result<(AccountStatusDto, bool), String> {
        let password = input.take_password()?;
        let metadata = AccountMetadata::from_input(&input)?;
        let account_changed = self
            .metadata
            .read()
            .map_err(|_| "Account state is temporarily unavailable.".to_owned())?
            .as_ref()
            != Some(&metadata);
        let database_path = account_database_path(&self.app_data, &metadata);
        let local_backend = open_local_backend(&metadata, &database_path)?;
        let config = metadata.account_config(password.as_str())?;
        let network_backend = MailBackend::open(config, &database_path)
            .map_err(|_| "The local mail database could not be opened.".to_owned())?;
        network_backend
            .initialize()
            .map_err(|_| "The local mail database could not be initialized.".to_owned())?;
        let connection = network_backend
            .check_connections()
            .await
            .map_err(crate::safe_mail_error)?;
        match (connection.imap_ok, connection.smtp_ok) {
            (true, true) => {}
            (false, false) => {
                return Err(
                    "The account was not saved because both IMAP and SMTP authentication failed."
                        .to_owned(),
                );
            }
            (false, true) => {
                return Err(
                    "The account was not saved because IMAP authentication failed.".to_owned(),
                );
            }
            (true, false) => {
                return Err(
                    "The account was not saved because SMTP authentication failed.".to_owned(),
                );
            }
        }

        // Credentials are namespaced by the public account identity. Writing
        // the new entry before atomically replacing account.json means a
        // crash can never replace the old account's only credential.
        let entry = keyring_entry(&metadata)?;
        let previous_password = read_previous_credential(&entry)?;
        entry
            .set_password(password.as_str())
            .map_err(|_| "The OS credential store could not save this account.".to_owned())?;

        if let Err(error) = self.store.save(&metadata) {
            if restore_previous_credential(&entry, previous_password.as_ref()).is_err() {
                return Err(format!(
                    "{error} The previous OS credential could not be restored."
                ));
            }
            return Err(error);
        }

        *self
            .metadata
            .write()
            .map_err(|_| "Account state is temporarily unavailable.".to_owned())? = Some(metadata);
        *self
            .startup_error
            .write()
            .map_err(|_| "Account state is temporarily unavailable.".to_owned())? = None;
        backend_state.replace(local_backend, network_backend)?;
        Ok((self.status(backend_state), account_changed))
    }
}

fn open_local_backend(
    metadata: &AccountMetadata,
    database_path: &Path,
) -> Result<MailBackend, String> {
    open_backend(metadata, database_path, LOCAL_ONLY_PLACEHOLDER_SECRET)
}

fn load_network_backend(
    metadata: &AccountMetadata,
    database_path: &Path,
) -> Result<Option<MailBackend>, String> {
    if metadata.provider == AccountProvider::Outlook {
        return Err(OUTLOOK_NOTICE.to_owned());
    }
    let entry = keyring_entry(metadata)?;
    let password = match entry.get_password() {
        Ok(password) => Zeroizing::new(password),
        Err(keyring::Error::NoEntry) => {
            // One-time compatibility path for builds that stored the active
            // account under the fixed "primary" key. Keep the legacy entry so
            // an interrupted migration remains recoverable.
            let legacy = legacy_keyring_entry()?;
            let legacy_password = match legacy.get_password() {
                Ok(password) => Zeroizing::new(password),
                Err(keyring::Error::NoEntry) => return Ok(None),
                Err(_) => {
                    return Err(
                        "The OS credential store is unavailable; local mail remains available."
                            .to_owned(),
                    );
                }
            };
            entry.set_password(legacy_password.as_str()).map_err(|_| {
                "The OS credential store could not migrate this account; local mail remains available."
                    .to_owned()
            })?;
            legacy_password
        }
        Err(_) => {
            return Err(
                "The OS credential store is unavailable; local mail remains available.".to_owned(),
            );
        }
    };
    open_backend(metadata, database_path, password.as_str()).map(Some)
}

fn open_backend(
    metadata: &AccountMetadata,
    database_path: &Path,
    password: &str,
) -> Result<MailBackend, String> {
    let config = metadata.account_config(password)?;
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

fn migrate_legacy_163(store: &AccountStore, path: &Path) -> Result<AccountMetadata, String> {
    let contents = Zeroizing::new(
        fs::read_to_string(path)
            .map_err(|_| "The legacy 163 credential file could not be read.".to_owned())?,
    );
    let values: Vec<&str> = contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if values.len() != 2 {
        return Err(
            "The legacy credential file must contain one email and one authorization password."
                .to_owned(),
        );
    }
    if !values[0].to_ascii_lowercase().ends_with("@163.com") {
        return Err("The legacy credential file does not contain a 163 address.".to_owned());
    }

    let metadata = AccountMetadata::preset(AccountProvider::NetEase163, values[0].to_owned())?;
    let password = Zeroizing::new(values[1].to_owned());
    metadata.account_config(password.as_str())?;
    let entry = keyring_entry(&metadata)?;
    let previous_password = read_previous_credential(&entry)?;
    entry
        .set_password(password.as_str())
        .map_err(|_| "The OS credential store could not import the legacy account.".to_owned())?;
    if let Err(error) = store.save(&metadata) {
        if restore_previous_credential(&entry, previous_password.as_ref()).is_err() {
            return Err(format!(
                "{error} The previous OS credential could not be restored."
            ));
        }
        return Err(error);
    }
    Ok(metadata)
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

#[cfg(test)]
mod tests {
    use mine_mail::ComposeRequest;
    use tempfile::tempdir;

    use super::{
        AccountMetadata, AccountProvider, AccountStore, BackendState, SmtpSecurity,
        account_database_path, keyring_username, open_local_backend,
    };

    #[test]
    fn built_in_presets_match_the_mvp_contract() {
        let gmail = AccountMetadata::preset(AccountProvider::Gmail, "demo@gmail.com".to_owned())
            .expect("Gmail preset");
        assert_eq!(gmail.imap.host, "imap.gmail.com");
        assert_eq!(gmail.smtp.port, 465);
        assert_eq!(gmail.smtp_security, SmtpSecurity::ImplicitTls);

        let outlook =
            AccountMetadata::preset(AccountProvider::Outlook, "demo@outlook.com".to_owned())
                .expect("Outlook preset");
        assert_eq!(outlook.smtp.host, "smtp-mail.outlook.com");
        assert_eq!(outlook.smtp.port, 587);
        assert_eq!(outlook.smtp_security, SmtpSecurity::StartTls);
    }

    #[test]
    fn account_store_contains_only_nonsecret_metadata() {
        let directory = tempdir().expect("temporary directory");
        let store = AccountStore::new(directory.path().join("account.json"));
        let metadata =
            AccountMetadata::preset(AccountProvider::NetEase163, "demo@163.com".to_owned())
                .expect("163 preset");
        store.save(&metadata).expect("save account metadata");
        assert_eq!(store.load().expect("load metadata"), Some(metadata));

        let replacement =
            AccountMetadata::preset(AccountProvider::Gmail, "demo@gmail.com".to_owned())
                .expect("Gmail preset");
        store
            .save(&replacement)
            .expect("atomically replace account metadata");
        assert_eq!(
            store.load().expect("load replacement metadata"),
            Some(replacement)
        );

        let contents = std::fs::read_to_string(directory.path().join("account.json"))
            .expect("metadata contents");
        assert!(!contents.contains("password"));
        assert!(!contents.contains("secret"));
    }

    #[test]
    fn account_database_filename_uses_a_one_way_identifier() {
        let metadata =
            AccountMetadata::preset(AccountProvider::NetEase163, "demo@163.com".to_owned())
                .expect("163 preset");
        let path = account_database_path(std::path::Path::new("data"), &metadata);
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("database filename");

        assert!(filename.starts_with("mine-mail-"));
        assert!(!filename.contains("demo"));
        assert!(!filename.contains("163.com"));
    }

    #[test]
    fn keyring_entries_are_stable_and_isolated_per_account() {
        let first =
            AccountMetadata::preset(AccountProvider::NetEase163, "first@163.com".to_owned())
                .expect("first preset");
        let same = AccountMetadata::preset(AccountProvider::NetEase163, "FIRST@163.COM".to_owned())
            .expect("same preset");
        let second = AccountMetadata::preset(AccountProvider::Gmail, "second@gmail.com".to_owned())
            .expect("second preset");

        let first_key = keyring_username(&first);
        assert_eq!(first_key, keyring_username(&same));
        assert_ne!(first_key, keyring_username(&second));
        assert!(first_key.starts_with("account-"));
        assert!(!first_key.contains("first"));
        assert!(!first_key.contains("163.com"));
    }

    #[test]
    fn custom_smtp_security_accepts_the_frontend_tls_value() {
        let request: super::ConfigureAccountRequest = serde_json::from_value(serde_json::json!({
            "provider": "custom",
            "email": "demo@example.com",
            "secret": "not-a-real-secret",
            "imap_host": "imap.example.com",
            "imap_port": 993,
            "smtp_host": "smtp.example.com",
            "smtp_port": 465,
            "smtp_security": "tls"
        }))
        .expect("custom account request");
        let metadata = AccountMetadata::from_input(&request).expect("custom metadata");
        assert_eq!(metadata.smtp_security, SmtpSecurity::ImplicitTls);
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
        let state = BackendState::new(Some(local_backend), None, false);

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
        assert!(backend.list_drafts().expect("list drafts").is_empty());
        assert!(backend.list_inbox(10).expect("list inbox").is_empty());
        assert!(backend.list_outbox().expect("list outbox").is_empty());
    }
}
