use std::time::Duration;

use lettre::{
    AsyncSmtpTransport, AsyncTransport, Tokio1Executor,
    address::Envelope,
    transport::smtp::{Error as SmtpError, authentication::Credentials},
};

use crate::{AccountConfig, MailError, OutboxStatus, Result};

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
            config.authorization_password().to_owned(),
        );
        let transport = AsyncSmtpTransport::<Tokio1Executor>::relay(&config.smtp.host)
            .map_err(|error| MailError::Smtp(format!("cannot configure TLS: {error}")))?
            .port(config.smtp.port)
            .credentials(credentials)
            .timeout(Some(SMTP_TIMEOUT))
            .build();
        Ok(Self { transport })
    }

    pub async fn probe(&self) -> Result<()> {
        match self.transport.test_connection().await {
            Ok(true) => Ok(()),
            Ok(false) => Err(MailError::Smtp(
                "server rejected the SMTP connection test".to_owned(),
            )),
            Err(error) => Err(MailError::Smtp(safe_smtp_error(&error))),
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
