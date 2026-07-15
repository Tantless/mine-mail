use std::{fmt, fs, path::Path};

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::{MailError, Result};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SmtpSecurity {
    ImplicitTls,
    StartTls,
}

/// Runtime account configuration. The authorization password is zeroized on
/// drop and deliberately omitted from `Debug` output.
pub struct AccountConfig {
    pub account_id: String,
    pub email: String,
    authorization_password: Zeroizing<String>,
    pub imap: ServerConfig,
    pub smtp: ServerConfig,
    pub smtp_security: SmtpSecurity,
}

impl AccountConfig {
    pub fn new(
        account_id: impl Into<String>,
        email: impl Into<String>,
        authorization_password: impl Into<String>,
        imap: ServerConfig,
        smtp: ServerConfig,
        smtp_security: SmtpSecurity,
    ) -> Result<Self> {
        let account_id = account_id.into().trim().to_owned();
        let email = email.into().trim().to_owned();
        let authorization_password = Zeroizing::new(authorization_password.into());

        if account_id.is_empty() {
            return Err(MailError::Config("account id cannot be empty".to_owned()));
        }
        if email.matches('@').count() != 1 || email.starts_with('@') || email.ends_with('@') {
            return Err(MailError::Config(
                "account email address is invalid".to_owned(),
            ));
        }
        if authorization_password.trim().is_empty() {
            return Err(MailError::Config(
                "the authorization password cannot be empty".to_owned(),
            ));
        }
        if imap.host.trim().is_empty() || smtp.host.trim().is_empty() {
            return Err(MailError::Config(
                "mail server host names cannot be empty".to_owned(),
            ));
        }
        if imap.port == 0 || smtp.port == 0 {
            return Err(MailError::Config(
                "mail server ports must be greater than zero".to_owned(),
            ));
        }

        Ok(Self {
            account_id,
            email,
            authorization_password,
            imap,
            smtp,
            smtp_security,
        })
    }

    pub fn from_163_password_file(path: impl AsRef<Path>) -> Result<Self> {
        let contents = Zeroizing::new(fs::read_to_string(path.as_ref()).map_err(|error| {
            MailError::Config(format!(
                "cannot read credentials file {}: {error}",
                path.as_ref().display()
            ))
        })?);

        Self::from_163_lines(contents.lines())
    }

    pub fn from_163_lines<'a>(lines: impl IntoIterator<Item = &'a str>) -> Result<Self> {
        let values: Vec<&str> = lines
            .into_iter()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();

        if values.len() != 2 {
            return Err(MailError::Config(
                "credentials file must contain exactly two non-empty lines: email and authorization password"
                    .to_owned(),
            ));
        }

        let email = values[0];
        if !email.ends_with("@163.com") || email.matches('@').count() != 1 {
            return Err(MailError::Config(
                "the first line must be a valid @163.com address".to_owned(),
            ));
        }
        if values[1].is_empty() {
            return Err(MailError::Config(
                "the authorization password cannot be empty".to_owned(),
            ));
        }

        Self::new(
            "primary",
            email,
            values[1],
            ServerConfig {
                host: "imap.163.com".to_owned(),
                port: 993,
            },
            ServerConfig {
                host: "smtp.163.com".to_owned(),
                port: 465,
            },
            SmtpSecurity::ImplicitTls,
        )
    }

    pub(crate) fn authorization_password(&self) -> &str {
        self.authorization_password.as_str()
    }
}

impl fmt::Debug for AccountConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AccountConfig")
            .field("account_id", &self.account_id)
            .field("email", &self.email)
            .field("authorization_password", &"[REDACTED]")
            .field("imap", &self.imap)
            .field("smtp", &self.smtp)
            .field("smtp_security", &self.smtp_security)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{AccountConfig, ServerConfig, SmtpSecurity};

    #[test]
    fn parses_two_line_163_credentials_without_debugging_secret() {
        let config = AccountConfig::from_163_lines(["demo@163.com", "not-a-real-secret"])
            .expect("valid config");

        assert_eq!(config.email, "demo@163.com");
        assert_eq!(config.imap.port, 993);
        assert!(!format!("{config:?}").contains("not-a-real-secret"));
        assert!(format!("{config:?}").contains("[REDACTED]"));
    }

    #[test]
    fn rejects_ambiguous_credentials_file() {
        assert!(AccountConfig::from_163_lines(["demo@163.com"]).is_err());
        assert!(AccountConfig::from_163_lines(["demo@example.com", "secret"]).is_err());
    }

    #[test]
    fn accepts_a_generic_starttls_account_without_exposing_the_secret() {
        let config = AccountConfig::new(
            "primary",
            "demo@example.com",
            "app-secret",
            ServerConfig {
                host: "imap.example.com".to_owned(),
                port: 993,
            },
            ServerConfig {
                host: "smtp.example.com".to_owned(),
                port: 587,
            },
            SmtpSecurity::StartTls,
        )
        .expect("valid generic account");

        assert_eq!(config.smtp_security, SmtpSecurity::StartTls);
        assert!(!format!("{config:?}").contains("app-secret"));
    }
}
