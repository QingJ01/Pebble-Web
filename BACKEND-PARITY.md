# Backend compatibility status

This page describes the current Web backend contract, not a claim of desktop feature parity. Desktop-only OS integration remains intentionally unavailable to the browser.

## Web-specific contracts

- Typed commands are authenticated `POST /api/v1/commands/{command}` requests. Unknown commands remain errors.
- OAuth starts at `POST /api/v1/oauth/{provider}/start`, reports status at `GET /api/v1/oauth/{provider}/status/{flow_id}`, and exposes configured provider availability at `GET /api/v1/oauth/providers`. Configure fixed provider callback URLs under the public Web origin; callbacks do not trust the request `Host` header.
- `get_server_info` returns the Web package version. `get_realtime_preference` and `set_realtime_preference` use `realtime`, `balanced`, `battery`, and `manual`; the saved server preference controls polling. `PEBBLE_SYNC_INTERVAL` is deprecated, ignored, and warned about at startup.
- `get_auto_backup_status` returns persisted scheduler status with Unix-second timestamps. Automatic backup configuration and status are encrypted in application data. Passphrase-encrypted secret exports are not supported yet and fail explicitly.
- Compose attachment staging returns an opaque app-owned token. Attachments are limited to 25 MiB total, filenames are sanitized, and access resolves inside the app attachment directory. The authenticated `/api/v1/attachments/{attachment_id}/download` route remains the browser download mechanism; `get_attachment_path` returns that relative route, never a host path.
- Sync events use `mail:sync-progress`, `mail:sync-complete`, `mail:error`, `mail:realtime-status`, `mail:new`, and `mail:unsnoozed`, wrapped as `{type,payload}`.

## Current limitations

The command names below are present in the frontend contract but are not yet implemented by the Web command bridge: `update_message_flags`, `archive_message`, `delete_message`, `restore_message`, `move_to_folder`, `empty_trash`, `batch_archive`, `batch_delete`, `batch_mark_read`, and `batch_star`. Existing REST routes do not substitute for these commands' required durable, provider-aware pending-operation behavior.

Other remaining gaps include provider-aware remote mutation and retry processing for pending mail operations, execution of rules on newly stored messages, identity-verified provider compose/send with durable Outbox/Sent handling, encrypted secret-passphrase backup/restore semantics, and full background task/service lifecycle and behavioral tests. The remote-CSS privacy-rendering behavior is an explicit hold and is unchanged.

## OAuth environment

OAuth is disabled unless provider credentials and `PEBBLE_PUBLIC_BASE_URL` are configured. Use a fixed HTTPS public origin (localhost HTTP is accepted for development), and register:

- `https://<public-origin>/api/v1/oauth/gmail/callback`
- `https://<public-origin>/api/v1/oauth/outlook/callback`

Set `PEBBLE_GOOGLE_CLIENT_ID`, `PEBBLE_GOOGLE_CLIENT_SECRET`, `PEBBLE_MICROSOFT_CLIENT_ID`, and `PEBBLE_MICROSOFT_CLIENT_SECRET` on the server. Availability indicates configuration only, not verified live-mail access.
