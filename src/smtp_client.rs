use std::time::Duration;

use lettre::{
    AsyncSmtpTransport, AsyncTransport, Tokio1Executor,
    address::Envelope,
    transport::smtp::{
        Error as SmtpError,
        authentication::{Credentials, Mechanism},
    },
};

use crate::{
    AccountConfig, AuthenticationKind, ConnectionFailure, ConnectionFailureKind,
    ConnectionProtocol, MailError, OutboxStatus, Result, SmtpSecurity,
};

const SMTP_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub(crate) struct DeliveryFailure {
    pub status: OutboxStatus,
    pub safe_reason: String,
}

pub(crate) struct SmtpClient {
    transport: AsyncSmtpTransport<Tokio1Executor>,
}

impl SmtpClient {
    pub fn new(config: &AccountConfig) -> Result<Self> {
        let credentials = Credentials::new(
            config.email.clone(),
            config.authorization_secret().to_owned(),
        );
        let builder = match config.smtp_security {
            SmtpSecurity::ImplicitTls => {
                AsyncSmtpTransport::<Tokio1Executor>::relay(&config.smtp.host)
            }
            SmtpSecurity::StartTls => {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.smtp.host)
            }
        }
        .map_err(|error| MailError::Smtp(format!("cannot configure TLS: {error}")))?
        .port(config.smtp.port)
        .credentials(credentials);
        let builder = if config.authentication_kind() == AuthenticationKind::OAuth2 {
            builder.authentication(vec![Mechanism::Xoauth2])
        } else {
            builder
        };
        let transport = builder.timeout(Some(SMTP_TIMEOUT)).build();
        Ok(Self { transport })
    }

    pub async fn probe(&self) -> std::result::Result<(), ConnectionFailure> {
        match self.transport.test_connection().await {
            Ok(true) => Ok(()),
            Ok(false) => Err(ConnectionFailure::new(
                ConnectionProtocol::Smtp,
                ConnectionFailureKind::Server,
            )),
            Err(error) => Err(smtp_probe_failure(&error)),
        }
    }

    pub async fn send_raw(
        &self,
        envelope: &Envelope,
        raw_rfc822: &[u8],
    ) -> std::result::Result<(), DeliveryFailure> {
        self.transport
            .send_raw(envelope, raw_rfc822)
            .await
            .map(|_| ())
            .map_err(classify_smtp_error)
    }
}

fn smtp_probe_failure(error: &SmtpError) -> ConnectionFailure {
    let status_code = error.status().map(u16::from);
    let kind = if let Some(status_code) = status_code {
        smtp_response_failure_kind(status_code)
    } else if error.is_tls() {
        ConnectionFailureKind::Tls
    } else if error.is_timeout() || error.is_transport_shutdown() {
        ConnectionFailureKind::Network
    } else if error.is_client() {
        ConnectionFailureKind::Authentication
    } else if error.is_response() {
        ConnectionFailureKind::Server
    } else {
        ConnectionFailureKind::Network
    };
    ConnectionFailure::new(ConnectionProtocol::Smtp, kind).with_status_code(status_code)
}

fn smtp_response_failure_kind(status_code: u16) -> ConnectionFailureKind {
    if matches!(status_code, 432 | 454 | 530 | 534 | 535 | 538) {
        ConnectionFailureKind::Authentication
    } else {
        ConnectionFailureKind::Server
    }
}

fn classify_smtp_error(error: SmtpError) -> DeliveryFailure {
    let status = if error.is_permanent() {
        OutboxStatus::Rejected
    } else if error.is_transient() {
        OutboxStatus::Retryable
    } else {
        // For timeouts and transport failures we cannot prove whether the
        // server accepted DATA before the connection was lost. Automatic retry
        // could duplicate a message, so the item requires manual review.
        OutboxStatus::DeliveryUnknown
    };
    DeliveryFailure {
        status,
        safe_reason: safe_smtp_error(&error),
    }
}

fn safe_smtp_error(error: &SmtpError) -> String {
    if let Some(status) = error.status() {
        return format!("SMTP server response {status}");
    }
    if error.is_timeout() {
        "SMTP timeout; delivery state is unknown".to_owned()
    } else if error.is_tls() {
        "SMTP TLS negotiation failed".to_owned()
    } else if error.is_transport_shutdown() {
        "SMTP transport was unavailable".to_owned()
    } else if error.is_client() {
        "SMTP client rejected the message".to_owned()
    } else {
        "SMTP transport failed; delivery state is unknown".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::{ConnectionFailureKind, smtp_response_failure_kind};

    #[test]
    fn smtp_authentication_statuses_are_not_reported_as_network_failures() {
        for status_code in [432, 454, 530, 534, 535, 538] {
            assert_eq!(
                smtp_response_failure_kind(status_code),
                ConnectionFailureKind::Authentication
            );
        }
    }

    #[test]
    fn other_smtp_statuses_are_reported_as_server_failures() {
        for status_code in [421, 450, 550] {
            assert_eq!(
                smtp_response_failure_kind(status_code),
                ConnectionFailureKind::Server
            );
        }
    }
}
