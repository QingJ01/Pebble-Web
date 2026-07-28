# Pebble Web

[中文](./README.md) | English

Pebble Web is a self-hosted web email client derived from the desktop [Pebble](https://github.com/QingJ01/Pebble) project. The default project documentation is Chinese, with this English version provided as a companion guide.

## Features

- Multi-account email management: inbox, folders, threads, starred mail, archive, trash
- Compose workflow: rich text, Markdown, HTML, attachments, replies, forwards, drafts
- Mail sync: IMAP/SMTP support with configurable background synchronization
- Full-text search powered by Tantivy
- Batch actions: archive, delete, mark read/unread, star
- Labels and Kanban: custom labels, mail task board, context notes
- Snooze workflow for deferred messages
- Translation integrations for DeepLX, DeepL, generic translation services, and LLM-based translation
- Privacy protection: remote images are blocked by default, with trusted sender support
- Attachment upload staging and authenticated downloads
- WebDAV settings backup and restore
- Bilingual UI: Chinese and English

## Quick Start

### One-Command Deploy

On a Linux server with Docker and Docker Compose installed:

```bash
curl -fsSL https://raw.githubusercontent.com/QingJ01/Pebble-Web/master/scripts/deploy.sh | bash
```

The script clones or updates the project, creates `.env`, builds the image, and starts the service. The default install directory is `~/pebble-web`, and the default port is `8080`.

To customize the directory, port, or login password:

```bash
curl -fsSL https://raw.githubusercontent.com/QingJ01/Pebble-Web/master/scripts/deploy.sh | env PEBBLE_INSTALL_DIR=/opt/pebble-web PEBBLE_PORT=8080 PEBBLE_PASSWORD='change-me' bash
```

### Docker Compose

Docker Compose is the recommended deployment method:

```bash
git clone https://github.com/QingJ01/Pebble-Web.git
cd Pebble-Web
cp .env.example .env
```

Edit `.env` and set at least:

```env
PEBBLE_PASSWORD=replace-with-your-login-password
PEBBLE_JWT_SECRET=replace-with-a-random-secret-at-least-32-chars
```

Start the service:

```bash
docker-compose up -d
```

Open:

```text
http://localhost:8080
```

### Manual Build

Requirements:

- Rust 1.80+
- Node.js 20+
- npm

```bash
cd frontend
npm install
npm run build
cd ..

cargo build --release
```

Run on Linux/macOS:

```bash
export PEBBLE_PASSWORD=your-password
export PEBBLE_JWT_SECRET=your-random-secret-at-least-32-chars
export PEBBLE_DATA_DIR=./data
export PEBBLE_STATIC_DIR=./frontend/dist

./target/release/pebble-web
```

Run on Windows PowerShell:

```powershell
$env:PEBBLE_PASSWORD="your-password"
$env:PEBBLE_JWT_SECRET="your-random-secret-at-least-32-chars"
$env:PEBBLE_DATA_DIR="./data"
$env:PEBBLE_STATIC_DIR="./frontend/dist"

.\target\release\pebble-web.exe
```

## Development

Backend:

```bash
cargo run
```

Frontend:

```bash
cd frontend
npm install
npm run dev
```

The frontend development server proxies application requests to the backend.

## Configuration

| Variable | Required | Default | Description |
| --- | --- | --- | --- |
| `PEBBLE_PASSWORD` | Yes | none | Web login password |
| `PEBBLE_JWT_SECRET` | Yes | none | Token signing secret, at least 32 characters |
| `PEBBLE_PORT` | No | `8080` | Server port |
| `PEBBLE_DATA_DIR` | No | `/data` | Data directory |
| `PEBBLE_STATIC_DIR` | No | `./frontend/dist` | Frontend static files directory |
| `PEBBLE_SYNC_INTERVAL` | No | `300` | Mail sync interval in seconds |
| `PEBBLE_ENCRYPTION_KEY` | No | auto-generated | 32-byte hex-encoded encryption key |
| `PEBBLE_PUBLIC_URL` | No | `http://localhost:8080` | Public base URL used to build OAuth redirect URIs |
| `GOOGLE_CLIENT_ID` | No | unset | Enables Gmail OAuth when set |
| `GOOGLE_CLIENT_SECRET` | No | unset | Usually required for Google Web clients |
| `MICROSOFT_CLIENT_ID` | No | unset | Enables Outlook OAuth when set |
| `MICROSOFT_CLIENT_SECRET` | No | unset | Required for confidential Microsoft clients |

### OAuth mailbox login (Gmail / Outlook)

Web supports OAuth 2.0 Authorization Code + PKCE. Register this redirect URI with Google / Microsoft:

```text
{PEBBLE_PUBLIC_URL}/api/v1/oauth/callback
```

When the corresponding `*_CLIENT_ID` is unset, the OAuth button for that provider is hidden; IMAP app-password login remains available.

Security notes:

- Do not use placeholder values from `.env.example` in production.
- Put the service behind HTTPS for production deployments.
- Back up `PEBBLE_DATA_DIR` regularly.

## Data Directory

Default layout:

```text
data/
├─ pebble.db
├─ index/
├─ attachments/
│  └─ staging/
└─ encryption.key
```

Notes:

- `pebble.db` stores accounts, message metadata, folders, labels, rules, and settings.
- `index/` stores the full-text search index.
- `attachments/` stores downloaded and staged attachment files.
- `encryption.key` protects local sensitive data. Losing it can make existing credentials unrecoverable.

## Tech Stack

| Area | Technology |
| --- | --- |
| Frontend | React 19, TypeScript, Vite, Tailwind CSS, Zustand, TanStack Query |
| Editor | TipTap with Markdown support |
| Backend | Rust, Axum, Tokio |
| Storage | SQLite, rusqlite |
| Search | Tantivy |
| Mail | IMAP, SMTP |
| Security | AES-256-GCM, Argon2, JWT |
| Deployment | Docker, Docker Compose |

## Project Structure

```text
Pebble-Web/
├─ src/                 # Rust web backend
├─ crates/              # Rust workspace crates
├─ frontend/            # React frontend
├─ Dockerfile
├─ docker-compose.yml
├─ .env.example
├─ README.md
├─ README_EN.md
└─ LICENSE
```

## Links

- [LINUX DO](https://linux.do/) — A sincere, friendly, united, and professional community

## License

This project is licensed under the [GNU Affero General Public License v3.0](./LICENSE), SPDX identifier `AGPL-3.0-only`.
