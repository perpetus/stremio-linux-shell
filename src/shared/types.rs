use std::sync::atomic::AtomicU64;

pub static SCALE_FACTOR: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy)]
pub enum MprisCommand {
    Play,
    Pause,
    PlayPause,
    Stop,
    Next,
    Previous,
    Seek(i64),
    SetPosition(i64),
    SetRate(f64),
}

#[derive(Debug, Clone)]
pub enum UserEvent {
    Raise,
    Show,
    Hide,
    Quit,
    MpvEventAvailable,
    WebViewEventAvailable,
    MprisCommand(MprisCommand),
    MetadataUpdate {
        title: Option<String>,
        artist: Option<String>,
        poster: Option<String>,
        thumbnail: Option<String>,
        logo: Option<String>,
    },
}
