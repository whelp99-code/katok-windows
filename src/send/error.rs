use thiserror::Error;

#[derive(Debug, Error)]
pub enum SendError {
    #[error(
        "katok send drives the official KakaoTalk Windows desktop app UI and is not available \
         on this OS. macOS send is out of scope for this fork."
    )]
    UnsupportedOs,
    #[error(
        "KakaoTalk.exe is not running. Start and log into the official KakaoTalk Windows app \
         first. katok send is not a login tool."
    )]
    NotRunning,
    #[error(
        "KakaoTalk appears to be on the login screen. Log in first; katok send is not a \
         login tool."
    )]
    NotLoggedIn,
    #[error(
        "chat not found in KakaoTalk UI: {0}. Open the 1:1 chat so its title is visible, \
         or check the name. If KakaoTalk is on the login screen, log in first; katok send \
         is not a login tool."
    )]
    ChatNotFound(String),
    #[error(
        "no chat {0} in the archive; run `katok sync --source txt` first or pass --room \
         with the visible title"
    )]
    ChatNotInArchive(String),
    #[error("multiple chats match {room} ({ids}); pass a more specific --room or --chat <id>")]
    AmbiguousRoom { room: String, ids: String },
    #[error("pass --room <visible title> or --chat <archive chat_id> to choose the 1:1 chat")]
    MissingTarget,
    #[error("refusing to send an empty message")]
    EmptyMessage,
    #[error("no local archive; run `katok sync --source txt` first or pass --room")]
    ArchiveMissing,
    #[error(
        "KakaoTalk did not send: the compose box still has text after Send. Paste without \
         Enter is a failure. The Send button may have stayed disabled."
    )]
    NotDelivered,
    #[error("KakaoTalk UI error: {0}")]
    Ui(String),
}
