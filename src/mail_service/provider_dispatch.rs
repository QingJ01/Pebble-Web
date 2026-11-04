use pebble_core::{PebbleError, ProviderType, Result};
use pebble_mail::{GmailProvider, ImapProvider, OutlookProvider};

use crate::state::AppState;

use super::{connect_gmail, connect_imap, connect_outlook};

/// A connected provider, ready for operations.
///
/// Wraps the three provider types so callers can match once instead of
/// duplicating the connect / disconnect boilerplate everywhere.
pub(crate) enum ConnectedProvider {
    Gmail(GmailProvider),
    Outlook(OutlookProvider),
    Imap(ImapProvider),
}

impl ConnectedProvider {
    /// Connect to the appropriate provider for the given account.
    pub async fn connect(
        state: &AppState,
        account_id: &str,
        provider_type: &ProviderType,
    ) -> Result<Self> {
        match provider_type {
            ProviderType::Gmail => {
                let p = connect_gmail(state, account_id).await?;
                Ok(Self::Gmail(p))
            }
            ProviderType::Outlook => {
                let p = connect_outlook(state, account_id).await?;
                Ok(Self::Outlook(p))
            }
            ProviderType::Imap => {
                let p = connect_imap(state, account_id).await?;
                Ok(Self::Imap(p))
            }
            ProviderType::Pop3 => Err(PebbleError::UnsupportedProvider(
                "POP3 does not support remote message mutations".to_string(),
            )),
        }
    }

    /// Disconnect the provider (only meaningful for IMAP).
    pub async fn disconnect(&self) {
        if let Self::Imap(imap) = self {
            let _ = imap.disconnect().await;
        }
    }
}

/// Parse a remote_id as an IMAP UID.
pub(crate) fn parse_imap_uid(remote_id: &str) -> Result<u32> {
    remote_id
        .parse::<u32>()
        .map_err(|_| PebbleError::Internal(format!("Invalid IMAP UID: {remote_id}")))
}
