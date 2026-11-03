# Backend parity status

This migration uses the settled desktop/Web audit and implementation contract. This matrix records the current implementation state, not runtime acceptance. Desktop-only OS integration is intentionally not a server feature.

| Domain | Current status | Remaining backend work |
| --- | --- | --- |
| Sync and providers | Partial | Imported portable IMAP/Gmail/Outlook/POP3 engines and compatible store APIs. The Web `SyncManager` still dispatches only IMAP; provider token refresh, POP3 dispatch, per-cycle WS events, and rules-trigger integration remain incomplete. |
| Accounts and identity | Partial | Added account label/provider display fields, sync-range state, OAuth start/status/callback protocol, and encrypted OAuth token persistence. Account REST creation/update, OAuth reauthorization, refresh coordination, proxy/TLS settings, identity repair, and provider worker startup remain incomplete. |
| Contacts | Partial | Portable contact and vCard store services are present. Command bridge and HTTP/provider synchronization wiring remain incomplete. |
| Display and privacy | Partial | Portable privacy crate is present. Web command adapters and server-side privacy behavior remain incomplete. |
| Compose and drafts | Partial | Portable sender and store functionality is present. Web API command coverage, durable attachment/draft orchestration, provider-specific send, and ambiguous-send recovery remain incomplete. |
| Settings and services | Partial | Encrypted command-backed signatures/templates, realtime preference, server info, and backup-status response shape are present. Scheduler-backed backups, rules execution, due-snooze service, diagnostics, translation/proxy settings, and full command coverage remain incomplete. |

The authenticated attachment download route and existing REST authentication remain in place. Native tray, keychain, window management, autostart, and OS notification APIs are not exposed through the browser command bridge.

## OAuth configuration

OAuth is disabled unless the server has provider client credentials and `PEBBLE_PUBLIC_BASE_URL`. Configure a fixed HTTPS public origin (localhost HTTP is accepted for development), then register:

- `https://<public-origin>/api/v1/oauth/gmail/callback`
- `https://<public-origin>/api/v1/oauth/outlook/callback`

Set `PEBBLE_GOOGLE_CLIENT_ID`, `PEBBLE_GOOGLE_CLIENT_SECRET`, `PEBBLE_MICROSOFT_CLIENT_ID`, and `PEBBLE_MICROSOFT_CLIENT_SECRET` in the server environment. Provider availability reports configuration only; it does not establish that consent or live mailbox access has been tested.
