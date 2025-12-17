pub const MAX_FRAME_RATE: f64 = 1000.0;

pub const IPC_SENDER: &str = "__postMessage";
pub const IPC_RECEIVER: &str = "__onMessage";

pub const IPC_MESSAGE: &str = "IPC";
pub const READY_MESSAGE: &str = "READY";

pub const IPC_SCRIPT: &str = include_str!("ipc.js");

pub const CMD_SWITCHES: &[&str] = &[
    "use-angle=gl-egl",
    "enable-gpu",
    "ignore-gpu-blocklist",
    "js-flags=--max-old-space-size=4096 --expose-gc --no-optimize-for-size",
    "enable-oop-rasterization",
    "process-per-site",
    "disable-quic",
    "disable-background-networking",
    "disable-sync",
    "disable-default-apps",
    "autoplay-policy=no-user-gesture-required",
    "disable-background-media-suspend",
    "disable-features=BackForwardCache",
    "renderer-process-limit=2",
    "max-active-webgl-contexts=1",
];
