use std::{collections::BTreeSet, time::Duration};

use reqwest::{Client, StatusCode, redirect::Policy};
use serde::Deserialize;

const GMAIL_API_ROOT: &str = "https://gmail.googleapis.com/gmail/v1/users/me";
const GMAIL_API_TIMEOUT: Duration = Duration::from_secs(30);
const HISTORY_PAGE_SIZE: usize = 500;
const MAX_HISTORY_PAGES: usize = 100;
const MAX_CHANGED_MESSAGES: usize = 500;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GmailHistoryDelta {
    pub(crate) next_history_id: String,
    pub(crate) gmail_message_ids: Vec<u64>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum GmailHistoryError {
    CursorExpired,
    Unavailable,
    InvalidResponse,
    TooLarge,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GmailProfileResponse {
    history_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GmailHistoryResponse {
    #[serde(default)]
    history: Vec<GmailHistoryRecord>,
    next_page_token: Option<String>,
    history_id: String,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GmailHistoryRecord {
    #[serde(default)]
    messages_added: Vec<GmailMessageChange>,
    #[serde(default)]
    messages_deleted: Vec<GmailMessageChange>,
    #[serde(default)]
    labels_added: Vec<GmailLabelChange>,
    #[serde(default)]
    labels_removed: Vec<GmailLabelChange>,
}

#[derive(Deserialize)]
struct GmailMessageChange {
    message: GmailMessageIdentity,
}

#[derive(Deserialize)]
struct GmailLabelChange {
    message: GmailMessageIdentity,
}

#[derive(Deserialize)]
struct GmailMessageIdentity {
    id: String,
}

pub(crate) struct GmailClient {
    client: Client,
}

impl GmailClient {
    pub(crate) fn new() -> Result<Self, GmailHistoryError> {
        Client::builder()
            .timeout(GMAIL_API_TIMEOUT)
            .redirect(Policy::none())
            .build()
            .map(|client| Self { client })
            .map_err(|_| GmailHistoryError::Unavailable)
    }

    pub(crate) async fn current_history_id(
        &self,
        access_token: &str,
    ) -> Result<String, GmailHistoryError> {
        let response = self
            .client
            .get(format!("{GMAIL_API_ROOT}/profile"))
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|_| GmailHistoryError::Unavailable)?;
        if !response.status().is_success() {
            return Err(GmailHistoryError::Unavailable);
        }
        let profile = response
            .json::<GmailProfileResponse>()
            .await
            .map_err(|_| GmailHistoryError::InvalidResponse)?;
        validate_history_id(&profile.history_id)?;
        Ok(profile.history_id)
    }

    pub(crate) async fn list_history(
        &self,
        access_token: &str,
        start_history_id: &str,
    ) -> Result<GmailHistoryDelta, GmailHistoryError> {
        validate_history_id(start_history_id)?;
        let mut page_token: Option<String> = None;
        let mut message_ids = BTreeSet::new();
        let max_results = HISTORY_PAGE_SIZE.to_string();
        for _ in 0..MAX_HISTORY_PAGES {
            let mut request = self
                .client
                .get(format!("{GMAIL_API_ROOT}/history"))
                .query(&[
                    ("startHistoryId", start_history_id),
                    ("maxResults", max_results.as_str()),
                ]);
            if let Some(token) = page_token.as_deref() {
                request = request.query(&[("pageToken", token)]);
            }
            let response = request
                .bearer_auth(access_token)
                .send()
                .await
                .map_err(|_| GmailHistoryError::Unavailable)?;
            if response.status() == StatusCode::NOT_FOUND {
                return Err(GmailHistoryError::CursorExpired);
            }
            if !response.status().is_success() {
                return Err(GmailHistoryError::Unavailable);
            }
            let page = response
                .json::<GmailHistoryResponse>()
                .await
                .map_err(|_| GmailHistoryError::InvalidResponse)?;
            validate_history_id(&page.history_id)?;
            let next_history_id = page.history_id;
            collect_changed_message_ids(page.history, &mut message_ids)?;
            page_token = page.next_page_token;
            if page_token.is_none() {
                return Ok(GmailHistoryDelta {
                    next_history_id,
                    gmail_message_ids: message_ids.into_iter().collect(),
                });
            }
        }

        Err(GmailHistoryError::TooLarge)
    }
}

fn collect_changed_message_ids(
    history: Vec<GmailHistoryRecord>,
    message_ids: &mut BTreeSet<u64>,
) -> Result<(), GmailHistoryError> {
    for record in history {
        for id in record
            .messages_added
            .into_iter()
            .map(|change| change.message.id)
            .chain(
                record
                    .messages_deleted
                    .into_iter()
                    .map(|change| change.message.id),
            )
            .chain(
                record
                    .labels_added
                    .into_iter()
                    .map(|change| change.message.id),
            )
            .chain(
                record
                    .labels_removed
                    .into_iter()
                    .map(|change| change.message.id),
            )
        {
            record_changed_message_id(message_ids, gmail_api_id_to_imap_id(&id)?)?;
        }
    }
    Ok(())
}

fn record_changed_message_id(
    message_ids: &mut BTreeSet<u64>,
    gmail_message_id: u64,
) -> Result<(), GmailHistoryError> {
    message_ids.insert(gmail_message_id);
    if message_ids.len() > MAX_CHANGED_MESSAGES {
        return Err(GmailHistoryError::TooLarge);
    }
    Ok(())
}

fn validate_history_id(value: &str) -> Result<(), GmailHistoryError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(GmailHistoryError::InvalidResponse);
    }
    Ok(())
}

/// Gmail API message IDs are hexadecimal renderings of the same unsigned
/// 64-bit identity exposed by IMAP as X-GM-MSGID.
fn gmail_api_id_to_imap_id(value: &str) -> Result<u64, GmailHistoryError> {
    if value.is_empty() || value.len() > 16 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(GmailHistoryError::InvalidResponse);
    }
    u64::from_str_radix(value, 16).map_err(|_| GmailHistoryError::InvalidResponse)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        GmailHistoryError, GmailHistoryResponse, MAX_CHANGED_MESSAGES, collect_changed_message_ids,
        gmail_api_id_to_imap_id, record_changed_message_id, validate_history_id,
    };

    #[test]
    fn converts_gmail_api_hex_identity_to_imap_decimal_identity() {
        assert_eq!(gmail_api_id_to_imap_id("17c7a8b9").unwrap(), 0x17c7a8b9);
        assert_eq!(
            gmail_api_id_to_imap_id("FFFFFFFFFFFFFFFF").unwrap(),
            u64::MAX
        );
        assert_eq!(
            gmail_api_id_to_imap_id("").unwrap_err(),
            GmailHistoryError::InvalidResponse
        );
        assert_eq!(
            gmail_api_id_to_imap_id("not-a-message").unwrap_err(),
            GmailHistoryError::InvalidResponse
        );
    }

    #[test]
    fn history_cursor_is_an_opaque_decimal_sequence() {
        assert!(validate_history_id("123456789").is_ok());
        assert_eq!(
            validate_history_id("12-34").unwrap_err(),
            GmailHistoryError::InvalidResponse
        );
    }

    #[test]
    fn every_history_change_shape_contributes_one_deduplicated_message_identity() {
        let page: GmailHistoryResponse = serde_json::from_str(
            r#"{
                "history": [{
                    "messagesAdded": [{"message": {"id": "10"}}],
                    "messagesDeleted": [{"message": {"id": "11"}}],
                    "labelsAdded": [{"message": {"id": "12"}}],
                    "labelsRemoved": [
                        {"message": {"id": "13"}},
                        {"message": {"id": "10"}}
                    ]
                }],
                "historyId": "123"
            }"#,
        )
        .expect("history response");
        let mut ids = BTreeSet::new();
        collect_changed_message_ids(page.history, &mut ids).expect("changed message ids");

        assert_eq!(ids, BTreeSet::from([0x10, 0x11, 0x12, 0x13]));
    }

    #[test]
    fn an_oversized_change_set_falls_back_before_unbounded_imap_work() {
        let mut ids = (0..MAX_CHANGED_MESSAGES as u64).collect::<BTreeSet<_>>();
        assert_eq!(
            record_changed_message_id(&mut ids, MAX_CHANGED_MESSAGES as u64).unwrap_err(),
            GmailHistoryError::TooLarge
        );
    }
}
