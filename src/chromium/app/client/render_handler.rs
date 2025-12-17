use std::sync::{Arc, RwLock};

use cef::{rc::*, *};
use flume::Sender;

use crate::{
    chromium::{ChromiumEvent, types::Viewport},
    shared::Frame,
};

use crate::shared::pbo_manager::{BufferPool, PboManager};

wrap_render_handler! {
    pub struct ChromiumRenderHandler {
        viewport: Arc<RwLock<Viewport>>,
        sender: Sender<ChromiumEvent>,
        last_paint: std::sync::Arc<std::sync::Mutex<Option<std::time::Instant>>>,
        pbo_manager: Arc<PboManager>,
        buffer_pool: Arc<BufferPool>,
    }

    impl RenderHandler {
        fn screen_info(&self, _browser: Option<&mut Browser>, screen_info: Option<&mut ScreenInfo>) -> i32 {
            if let Some(screen_info) = screen_info {
                    screen_info.device_scale_factor = 1.0;
                    return true.into();
                }

            false.into()
        }

        fn screen_point(
            &self,
            _browser: Option<&mut Browser>,
            _view_x: i32,
            _view_y: i32,
            _screen_x: Option<&mut i32>,
            _screen_y: Option<&mut i32>,
        ) -> i32 {
            false.into()
        }

        fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
            if let Some(rect) = rect
                && let Ok(viewport) = self.viewport.read() {
                    rect.width = viewport.width;
                    rect.height = viewport.height;
                }
        }

        fn on_paint(
            &self,
            _browser: Option<&mut Browser>,
            _type: PaintElementType,
            dirty_rects: Option<&[Rect]>,
            buffer: *const u8,
            width: i32,
            height: i32,
        ) {
            let fn_start = std::time::Instant::now();

            // Gap detection
            if let Ok(mut last_paint) = self.last_paint.lock() {
                if let Some(last) = *last_paint {
                    let elapsed = fn_start.duration_since(last);
                    if elapsed.as_millis() > 500 {
                        tracing::error!("OnPaint GAP CRITICAL: {:?} - possible navigation stall", elapsed);
                    }
                }
                *last_paint = Some(fn_start);
            }
            let _lock_time = fn_start.elapsed();

            if let Some(dirty_rects_slice) = dirty_rects {
                let width = width as i32;
                let height = height as i32;
                let dirty_rects_count = dirty_rects_slice.len();

                // 1. Calculate bounding box of dirty rects
                // If no dirty rects provided, assume full frame
                let (x, y, w, h) = if dirty_rects_count > 0 {
                    let mut min_x = width;
                    let mut min_y = height;
                    let mut max_x = 0;
                    let mut max_y = 0;

                    for r in dirty_rects_slice {
                        min_x = min_x.min(r.x);
                        min_y = min_y.min(r.y);
                        max_x = max_x.max(r.x + r.width);
                        max_y = max_y.max(r.y + r.height);
                    }

                    // Clamp to valid area
                    min_x = min_x.max(0);
                    min_y = min_y.max(0);
                    max_x = max_x.min(width);
                    max_y = max_y.min(height);

                    if max_x > min_x && max_y > min_y {
                        (min_x, min_y, max_x - min_x, max_y - min_y)
                    } else {
                        (0, 0, width, height)
                    }
                } else {
                    (0, 0, width, height)
                };

                let pool_start = std::time::Instant::now();
                // 2. Acquire buffer (returns None if full/busy - frame dropping)
                if let Some((idx, dest_ptr)) = self.buffer_pool.acquire_for_write(width, height) {
                    let pool_acquire_time = pool_start.elapsed();
                    let memcpy_start = std::time::Instant::now();

                    // 3. Copy only the bounding box area
                    // CEF buffer is always full frame packed (BGRA usually)
                    // We copy from (x, y) in source to (x, y) in dest?
                    // No, dest is our pool buffer. It is also full frame sized.
                    // We want to update the region (x, y) w*h in the pool buffer.
                    // The consumer (WebView) will then upload starting from (x, y).

                    unsafe {
                        let src_stride = width * 4;
                        let row_bytes = w * 4;

                        // Optimize for full frame copy vs partial
                        if w == width && h == height {
                             std::ptr::copy_nonoverlapping(buffer as *const u8, dest_ptr, (width * height * 4) as usize);
                        } else {
                            // Row-by-row copy for dirty region
                            for r in 0..h {
                                let src_offset = ((y + r) * src_stride + x * 4) as usize;
                                // Dest buffer has same layout as source (full frame)
                                let dst_offset = src_offset;

                                std::ptr::copy_nonoverlapping(
                                    (buffer as *const u8).add(src_offset),
                                    dest_ptr.add(dst_offset),
                                    row_bytes as usize
                                );
                            }
                        }
                    }
                    let memcpy_time = memcpy_start.elapsed();

                    // 4. Create Frame with Arc (Zero-Copy)
                    if let Some(arc_data) = self.buffer_pool.get_buffer_arc(idx) {
                        let mut dirty_vec = Vec::with_capacity(dirty_rects_count);
                        if dirty_rects_count > 0 {
                             for r in dirty_rects_slice {
                                 dirty_vec.push((r.x, r.y, r.width, r.height));
                             }
                        }

                        let frame = Frame {
                            x,
                            y,
                            width: w,
                            height: h,
                            full_width: width,
                            full_height: height,
                            buffer: Some(arc_data),
                            dirty_rects: dirty_vec,
                            format: 0, // BGRA
                            stride: (width * 4) as u32,
                            pbo_idx: None,
                            handle: None,
                            modifier: 0,
                            created_at: None,
                        };

                        // 5. Non-blocking send
                        let send_start = std::time::Instant::now();
                        match self.sender.try_send(ChromiumEvent::Render(frame)) {
                            Ok(_) => {
                                let send_time = send_start.elapsed();
                                let total_time = fn_start.elapsed();

                                if total_time.as_millis() > 10 {
                                     tracing::warn!(
                                        "OnPaint SLOW: total={:?} | pool_acq={:?} memcpy={:?} send={:?} | {}x{} updated {}x{}",
                                        total_time,
                                        pool_acquire_time,
                                        memcpy_time,
                                        send_time,
                                        width, height, w, h
                                    );
                                }
                            }
                            Err(_) => {
                                tracing::debug!("Frame dropped (channel full)");
                            }
                        }
                    }
                } else {
                    tracing::debug!("Frame dropped (pool exhausted)");
                }
            }
        }
    }
}
