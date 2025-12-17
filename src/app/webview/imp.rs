use std::{cell::Cell, rc::Rc, sync::OnceLock};

use adw::subclass::prelude::*;
use crossbeam_queue::SegQueue;
use epoxy::{
    types::{GLint, GLuint},
    *,
};
use gtk::{
    DropTarget,
    gdk::{DragAction, FileList, GLContext},
    glib::{self, ControlFlow, Propagation, Properties},
    prelude::*,
};

use crate::{
    app::webview::gl,
    shared::{
        Frame,
        states::{KeyboardState, PointerState},
    },
};

pub const FRAGMENT_SRC: &str = include_str!("shader.frag");
pub const VERTEX_SRC: &str = include_str!("shader.vert");

#[derive(Default, Properties)]
#[properties(wrapper_type = super::WebView)]
pub struct WebView {
    #[property(get, set)]
    scale_factor: Cell<i32>,
    program: Cell<GLuint>,
    vao: Cell<GLuint>,
    vbo: Cell<GLuint>,
    texture: Cell<GLuint>,
    texture_uniform: Cell<GLint>,
    pbos: Cell<[GLuint; 2]>,
    pbo_index: Cell<usize>,
    // Cached dimensions to avoid redundant resize calls
    last_width: Cell<i32>,
    last_height: Cell<i32>,
    pub pointer_state: Rc<PointerState>,
    pub keyboard_state: Rc<KeyboardState>,
    pub frames: Box<SegQueue<Frame>>,
    // FPS Tracking
    pub fps_last_time: Cell<Option<std::time::Instant>>,
    pub fps_counter: Cell<u32>,
}

#[glib::object_subclass]
impl ObjectSubclass for WebView {
    const NAME: &'static str = "WebView";
    type Type = super::WebView;
    type ParentType = gtk::GLArea;
}

#[glib::derived_properties]
impl ObjectImpl for WebView {
    fn signals() -> &'static [glib::subclass::Signal] {
        static SIGNALS: OnceLock<Vec<glib::subclass::Signal>> = OnceLock::new();
        SIGNALS.get_or_init(|| {
            vec![
                glib::subclass::Signal::builder("fps-update")
                    .param_types([u32::static_type()])
                    .build(),
            ]
        })
    }

    fn constructed(&self) {
        self.parent_constructed();

        let drop_target = DropTarget::new(FileList::static_type(), DragAction::COPY);
        self.obj().add_controller(drop_target);
    }
}

impl WidgetImpl for WebView {
    fn realize(&self) {
        self.parent_realize();

        let gl_area = self.obj();
        gl_area.make_current();

        if gl_area.error().is_some() {
            return;
        }

        let vertex_shader = gl::compile_vertex_shader(VERTEX_SRC);
        let fragment_shader = gl::compile_fragment_shader(FRAGMENT_SRC);
        let program = gl::create_program(vertex_shader, fragment_shader);
        let (vao, vbo) = gl::create_geometry(program);
        let (texture, texture_uniform) = gl::create_texture(program, "text_uniform");

        let width = self.obj().width();
        let height = self.obj().height();
        let pbo1 = gl::create_pbo(width, height);
        let pbo2 = gl::create_pbo(width, height);

        self.program.set(program);
        self.vao.set(vao);
        self.vbo.set(vbo);
        self.texture.set(texture);
        self.pbos.set([pbo1, pbo2]);
        self.pbo_index.set(0);
        self.texture_uniform.set(texture_uniform);

        self.obj().add_tick_callback(|webview, _| {
            if !webview.imp().frames.is_empty() {
                webview.queue_render();
            }

            ControlFlow::Continue
        });
    }

    fn unrealize(&self) {
        unsafe {
            epoxy::DeleteProgram(self.program.get());
            epoxy::DeleteTextures(1, &self.texture.get());
            epoxy::DeleteBuffers(1, &self.vbo.get());
            epoxy::DeleteVertexArrays(1, &self.vao.get());
            epoxy::DeleteBuffers(2, self.pbos.get().as_ptr());
        }

        self.program.take();
        self.vao.take();
        self.vbo.take();
        self.texture.take();
        self.texture_uniform.take();

        self.parent_unrealize();
    }
}

impl GLAreaImpl for WebView {
    fn render(&self, _: &GLContext) -> Propagation {
        let scale_factor = self.scale_factor.get();

        // Process ALL frames in the queue to ensure all partial updates are applied
        // Frame Coalescing for RESIZE events:
        // 1. Collect all valid frames from queue
        // 2. Iterate and skip frames if the NEXT frame has a DIFFERENT size (intermediate resize step)
        // 3. Always process frames if size is consistent (partial updates) or if it's the last frame

        let mut frames_vec = Vec::new();
        while let Some(frame) = self.frames.pop() {
            // Basic validation
            let has_data = frame
                .buffer
                .as_deref()
                .map(|v| !v.is_empty())
                .unwrap_or(false);
            if has_data {
                frames_vec.push(frame);
            }
        }

        let mut iter = frames_vec.into_iter().peekable();
        while let Some(frame) = iter.next() {
            // Check optimization: skip if next frame has different size
            if let Some(next_frame) = iter.peek()
                && (frame.full_width != next_frame.full_width
                    || frame.full_height != next_frame.full_height)
            {
                continue;
            }

            let buffer = frame.buffer.as_deref().map(|v| v.as_slice()).unwrap_or(&[]);

            // Only resize if dimensions changed (cached comparison)
            let last_w = self.last_width.get();
            let last_h = self.last_height.get();
            if frame.full_width != last_w || frame.full_height != last_h {
                gl::resize_pbo(self.pbos.get()[0], frame.full_width, frame.full_height);
                gl::resize_pbo(self.pbos.get()[1], frame.full_width, frame.full_height);
                gl::resize_texture(self.texture.get(), frame.full_width, frame.full_height);
                self.last_width.set(frame.full_width);
                self.last_height.set(frame.full_height);
            }

            unsafe {
                let pbos = self.pbos.get();
                let current = self.pbo_index.get();
                let next_index = (current + 1) % 2;
                self.pbo_index.set(next_index);
                let next_pbo = pbos[next_index];

                BindBuffer(PIXEL_UNPACK_BUFFER, next_pbo);

                let size = (frame.full_width * frame.full_height * 4) as isize;
                let ptr = MapBufferRange(
                    PIXEL_UNPACK_BUFFER,
                    0,
                    size,
                    MAP_WRITE_BIT | MAP_INVALIDATE_BUFFER_BIT,
                ) as *mut u8;

                if !ptr.is_null() {
                    let stride = (frame.full_width * 4) as usize;
                    let width_bytes = (frame.width * 4) as usize;

                    // Fast path: if updating full frame, use single memcpy
                    if frame.x == 0
                        && frame.y == 0
                        && frame.width == frame.full_width
                        && frame.height == frame.full_height
                    {
                        std::ptr::copy_nonoverlapping(buffer.as_ptr(), ptr, size as usize);
                    } else {
                        // Partial update: row-by-row copy
                        let src_ptr = buffer.as_ptr();
                        for row in 0..frame.height {
                            let row_offset =
                                (frame.y as usize + row as usize) * stride + (frame.x as usize * 4);

                            std::ptr::copy_nonoverlapping(
                                src_ptr.add(row_offset),
                                ptr.add(row_offset),
                                width_bytes,
                            );
                        }
                    }

                    UnmapBuffer(PIXEL_UNPACK_BUFFER);
                }

                BindTexture(TEXTURE_2D, self.texture.get());

                // Set unpacking row length for partial update from full-width PBO
                PixelStorei(UNPACK_ROW_LENGTH, frame.full_width);

                // Calculate offset in PBO for the (x, y) start of the dirty rect
                let pbo_offset =
                    (frame.y as usize * frame.full_width as usize + frame.x as usize) * 4;

                TexSubImage2D(
                    TEXTURE_2D,
                    0,
                    frame.x,
                    frame.y,
                    frame.width,
                    frame.height,
                    BGRA,
                    UNSIGNED_BYTE,
                    pbo_offset as *const std::ffi::c_void,
                );

                // Reset unpack state
                PixelStorei(UNPACK_ROW_LENGTH, 0);

                BindBuffer(PIXEL_UNPACK_BUFFER, 0);
            }
        }

        // Always clear the background to prevent garbage artifacts
        // We handle the rendering fully, so we stop propagation.
        unsafe {
            epoxy::ClearColor(0.0, 0.0, 0.0, 1.0);
            epoxy::Clear(epoxy::COLOR_BUFFER_BIT);
        }

        // Draw the final state of the texture (either updated or cached)
        // We use last_width/last_height which represents the current texture dimensions.
        let width = self.last_width.get();
        let height = self.last_height.get();

        if width > 0 && height > 0 {
            gl::resize_viewport(width * scale_factor, height * scale_factor);

            gl::draw_texture(
                self.program.get(),
                self.texture.get(),
                self.texture_uniform.get(),
                self.vao.get(),
            );
        }

        // FPS Calculation
        let now = std::time::Instant::now();
        if let Some(last_time) = self.fps_last_time.get() {
            if now.duration_since(last_time).as_secs() >= 1 {
                let fps = self.fps_counter.get();
                self.obj().emit_by_name::<()>("fps-update", &[&fps]);
                self.fps_last_time.set(Some(now));
                self.fps_counter.set(0);
            } else {
                self.fps_counter.set(self.fps_counter.get() + 1);
            }
        } else {
            self.fps_last_time.set(Some(now));
            self.fps_counter.set(0);
        }

        Propagation::Stop
    }
}
