#[derive(Debug)]
pub struct Viewport {
    pub width: i32,
    pub height: i32,
    pub scale_factor: i32,
    pub zoom_level: f64,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            width: 1700,
            height: 1004,
            scale_factor: 1,
            zoom_level: 0.0,
        }
    }
}
