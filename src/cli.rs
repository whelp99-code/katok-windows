use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "katok", about = "katok: local KakaoTalk search CLI")]
pub(crate) struct Cli {
    #[arg(long)]
    pub(crate) data_dir: Option<PathBuf>,
    #[arg(long)]
    pub(crate) config: Option<PathBuf>,
    #[command(subcommand)]
    pub(crate) command: Commands,
}

#[derive(Subcommand)]
pub(crate) enum Commands {
    Doctor {
        #[arg(long)]
        macos_probe: bool,
        #[arg(long)]
        json: bool,
    },
    Sync {
        #[arg(long)]
        source: Option<String>,
        path: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    Index {
        #[arg(long)]
        full: bool,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        json: bool,
    },
    Search {
        #[command(subcommand)]
        command: SearchCommand,
    },
    Chunk {
        #[command(subcommand)]
        command: ChunkCommand,
    },
    Source {
        #[command(subcommand)]
        command: SourceCommand,
    },
    Permissions {
        #[command(subcommand)]
        command: PermissionsCommand,
    },
    WipeIndex {
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
    Chunks {
        #[arg(long)]
        chat: String,
        #[arg(long)]
        json: bool,
    },
    /// Send text through the official KakaoTalk Windows desktop app UI.
    ///
    /// KakaoTalk PC must already be running and logged in. This is not a login
    /// tool and not a Kakao protocol client — it only drives KakaoTalk.exe via
    /// Windows UI Automation / SendInput. macOS send is out of scope.
    #[cfg(feature = "windows-send")]
    Send {
        /// Visible 1:1 chat title as KakaoTalk shows it (window / chat-list name).
        #[arg(long, required_unless_present_any = ["chat", "list_windows"])]
        room: Option<String>,
        /// Archive `chat_id` as reported by search / source chats / txt sync.
        #[arg(long, conflicts_with = "room")]
        chat: Option<String>,
        /// Message body. Reads stdin when omitted (ignored with --dry-run).
        #[arg(long)]
        text: Option<String>,
        /// Open/focus the chat but do not type or press Send.
        #[arg(long)]
        dry_run: bool,
        /// List KakaoTalk window titles and exit without sending.
        #[arg(long)]
        list_windows: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum SearchCommand {
    Keyword {
        query: String,
        #[arg(long)]
        json: bool,
    },
    Bm25 {
        query: String,
        #[arg(long)]
        json: bool,
    },
    Semantic {
        query: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum ChunkCommand {
    Get {
        chunk_id: String,
        #[arg(long)]
        include_message_ids: bool,
        #[arg(long)]
        redact: bool,
        #[arg(long)]
        json: bool,
    },
    Context {
        chunk_id: String,
        #[arg(long)]
        json: bool,
    },
    Parent {
        chunk_id: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum SourceCommand {
    Chats {
        #[arg(long)]
        source: Option<String>,
        path: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum PermissionsCommand {
    Macos {
        #[arg(long)]
        accessibility: bool,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        json: bool,
    },
}
