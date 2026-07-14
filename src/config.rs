use std::{fmt, fs, path::Path};

use zeroize::Zeroizing;

use crate::{MailError, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

/// Runtime account configuration. The authorization password is zeroized on
/// drop and deliberately omitted from `Debug` output.
pub struct AccountConfig {
    pub account_id: String,
    pub email: String,
    authorization_password: Zeroizing<String>,
    pub imap: ServerConfig,
    pub smtp: ServerConfig,
}

impl AccountConfig {
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

        Ok(Self {
            account_id: "primary".to_owned(),
            email: email.to_owned(),
            authorization_password: Zeroizing::new(values[1].to_owned()),
            imap: ServerConfig {
                host: "imap.163.com".to_owned(),
                port: 993,
            },
            smtp: ServerConfig {
                host: "smtp.163.com".to_owned(),
                port: 465,
            },
        })
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
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::AccountConfig;

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
}
