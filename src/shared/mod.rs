pub mod ipc;
pub mod pbo_manager;
pub mod states;
pub mod types;

#[derive(Default, Debug)]
pub struct Frame {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub full_width: i32,
    pub full_height: i32,
    pub buffer: Option<std::sync::Arc<Vec<u8>>>,
    pub dirty_rects: Vec<(i32, i32, i32, i32)>,
    pub pbo_idx: Option<usize>,
    pub handle: Option<usize>,
    pub stride: u32,
    pub format: u32,
    pub modifier: u64,
    pub created_at: Option<std::time::Instant>,
}
