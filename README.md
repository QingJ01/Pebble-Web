# Pebble Web

中文 | [English](./README_EN.md)

Pebble Web 是一个可自托管的 Web 邮件客户端，基于桌面端 [Pebble](https://github.com/QingJ01/Pebble) 改造而来，默认面向中文用户编写文档，支持 Docker 部署和浏览器访问。

## 主要功能

- 多账户邮件管理：收件箱、文件夹、会话、星标、归档、回收站
- 邮件账号：IMAP/SMTP 密码登录，以及 Gmail / Outlook OAuth 2.0（PKCE）授权登录
- 邮件撰写：富文本、Markdown、HTML、附件、回复、转发、草稿
- 邮件同步：支持 IMAP/SMTP 与 Gmail/Outlook API，后台自动同步，可配置同步间隔
- 全文搜索：基于 Tantivy，支持常用邮件检索条件
- 批量操作：归档、删除、标记已读/未读、星标
- 标签与看板：自定义标签、邮件任务看板、上下文备注
- 稍后处理：邮件延后提醒和待处理列表
- 邮件翻译：支持 DeepLX、DeepL、通用翻译服务和 LLM 翻译配置
- 隐私保护：默认阻止远程图片，可维护可信发件人
- 附件管理：上传暂存、下载、内联附件展示
- 云端配置备份：通过 WebDAV 备份和恢复设置
- 双语界面：中文和 English

## 快速开始

### 一键部署

在已安装 Docker 和 Docker Compose 的 Linux 服务器上运行：

```bash
curl -fsSL https://raw.githubusercontent.com/QingJ01/Pebble-Web/master/scripts/deploy.sh | bash
```

脚本会自动拉取项目、生成 `.env`、构建镜像并启动服务。默认安装目录为 `~/pebble-web`，默认端口为 `8080`。

如需自定义目录、端口或登录密码：

```bash
curl -fsSL https://raw.githubusercontent.com/QingJ01/Pebble-Web/master/scripts/deploy.sh | env PEBBLE_INSTALL_DIR=/opt/pebble-web PEBBLE_PORT=8080 PEBBLE_PASSWORD='change-me' bash
```

### Docker Compose

推荐使用 Docker Compose 部署：

```bash
git clone https://github.com/QingJ01/Pebble-Web.git
cd Pebble-Web
cp .env.example .env
```

编辑 `.env`，至少设置：

```env
PEBBLE_PASSWORD=请替换为登录密码
PEBBLE_JWT_SECRET=请替换为至少32位的随机字符串
```

启动服务：

```bash
docker-compose up -d
```

浏览器访问：

```text
http://localhost:8080
```

### 手动构建

环境要求：

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

运行前设置环境变量：

```bash
export PEBBLE_PASSWORD=your-password
export PEBBLE_JWT_SECRET=your-random-secret-at-least-32-chars
export PEBBLE_DATA_DIR=./data
export PEBBLE_STATIC_DIR=./frontend/dist

./target/release/pebble-web
```

Windows PowerShell 示例：

```powershell
$env:PEBBLE_PASSWORD="your-password"
$env:PEBBLE_JWT_SECRET="your-random-secret-at-least-32-chars"
$env:PEBBLE_DATA_DIR="./data"
$env:PEBBLE_STATIC_DIR="./frontend/dist"

.\target\release\pebble-web.exe
```

## 本地开发

后端：

```bash
cargo run
```

前端：

```bash
cd frontend
npm install
npm run dev
```

开发服务器会把前端请求代理到后端服务。

## 配置项

| 变量 | 必填 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `PEBBLE_PASSWORD` | 是 | 无 | Web 登录密码 |
| `PEBBLE_JWT_SECRET` | 是 | 无 | 登录令牌签名密钥，至少 32 字符 |
| `PEBBLE_PORT` | 否 | `8080` | 服务端口 |
| `PEBBLE_DATA_DIR` | 否 | `/data` | 数据目录 |
| `PEBBLE_STATIC_DIR` | 否 | `./frontend/dist` | 前端静态文件目录 |
| `PEBBLE_SYNC_INTERVAL` | 否 | `300` | 邮件同步间隔，单位秒 |
| `PEBBLE_ENCRYPTION_KEY` | 否 | 自动生成 | 32 字节 Hex 编码加密密钥 |
| `PEBBLE_PUBLIC_URL` | 否 | `http://localhost:8080` | 对外访问根地址，用于拼接 OAuth 回调 URL |
| `GOOGLE_CLIENT_ID` | 否 | 无 | 配置后启用 Gmail OAuth |
| `GOOGLE_CLIENT_SECRET` | 否 | 无 | Google Web 客户端通常需要 |
| `MICROSOFT_CLIENT_ID` | 否 | 无 | 配置后启用 Outlook OAuth |
| `MICROSOFT_CLIENT_SECRET` | 否 | 无 | 机密客户端需要；公共客户端可留空 |

### OAuth 邮箱登录（Gmail / Outlook）

Web 端支持 OAuth 2.0 Authorization Code + PKCE。流程：

1. 在设置中添加账户，点击「使用 Google / Outlook 账号登录」
2. 浏览器跳转到服务商授权页
3. 授权完成后回调 `{PEBBLE_PUBLIC_URL}/api/v1/oauth/callback`
4. 服务端换取 token、加密保存，并启动 Gmail / Outlook 同步

在 Google Cloud / Microsoft Entra 中登记的 Redirect URI 必须为：

```text
{PEBBLE_PUBLIC_URL}/api/v1/oauth/callback
```

未配置对应 `*_CLIENT_ID` 时，前端不会显示该服务商的 OAuth 按钮；仍可使用 IMAP 应用专用密码方式添加账户。

安全建议：

- 不要使用 `.env.example` 中的占位密码和密钥。
- 生产环境请使用 HTTPS 反向代理。
- 请定期备份 `PEBBLE_DATA_DIR`。

## 数据目录

默认数据目录结构：

```text
data/
├─ pebble.db
├─ index/
├─ attachments/
│  └─ staging/
└─ encryption.key
```

其中：

- `pebble.db` 保存账户、邮件元数据、文件夹、标签、规则等信息。
- `index/` 保存全文搜索索引。
- `attachments/` 保存附件和上传暂存文件。
- `encryption.key` 用于本地敏感数据加密，丢失后无法解密已有凭据。

## 技术栈

| 模块 | 技术 |
| --- | --- |
| 前端 | React 19、TypeScript、Vite、Tailwind CSS、Zustand、TanStack Query |
| 编辑器 | TipTap、Markdown 支持 |
| 后端 | Rust、Axum、Tokio |
| 存储 | SQLite、rusqlite |
| 搜索 | Tantivy |
| 邮件 | IMAP、SMTP |
| 加密 | AES-256-GCM、Argon2、JWT |
| 部署 | Docker、Docker Compose |

## 项目结构

```text
Pebble-Web/
├─ src/                 # Rust Web 后端
├─ crates/              # Rust workspace 子包
├─ frontend/            # React 前端
├─ Dockerfile
├─ docker-compose.yml
├─ .env.example
├─ README.md
├─ README_EN.md
└─ LICENSE
```

## 友情链接

- [LINUX DO](https://linux.do/) — 真诚、友善、团结、专业，共建你我引以为荣的社区

## 许可协议

本项目基于 [GNU Affero General Public License v3.0](./LICENSE) 开源，SPDX 标识为 `AGPL-3.0-only`。
