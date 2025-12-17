use std::{cell::Cell, rc::Rc};

use adw::subclass::prelude::*;
use crossbeam_queue::SegQueue;
use epoxy::types::{GLint, GLuint};
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
pub const UPDATES_PER_RENDER: i32 = 8;

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
    pub pointer_state: Rc<PointerState>,
    pub keyboard_state: Rc<KeyboardState>,
    pub frames: Box<SegQueue<Frame>>,
    pub frame_count: Cell<u32>,
    pub last_frame_time: Cell<i64>,
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
        static SIGNALS: std::sync::OnceLock<Vec<glib::subclass::Signal>> =
            std::sync::OnceLock::new();
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

        unsafe {
            let renderer = epoxy::GetString(epoxy::RENDERER);
            if !renderer.is_null() {
                let renderer = std::ffi::CStr::from_ptr(renderer as *const i8);
                let _ = super::GPU_RENDERER.set(renderer.to_string_lossy().into_owned());
            }
        }

        let vertex_shader = gl::compile_vertex_shader(VERTEX_SRC);
        let fragment_shader = gl::compile_fragment_shader(FRAGMENT_SRC);
        let program = gl::create_program(vertex_shader, fragment_shader);
        let (vao, vbo) = gl::create_geometry(program);
        let (texture, texture_uniform) = gl::create_texture(program, "text_uniform");

        self.program.set(program);
        self.vao.set(vao);
        self.vbo.set(vbo);
        self.texture.set(texture);
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
        let start = std::time::Instant::now();
        let scale_factor = self.scale_factor.get();
        let queue_len = self.frames.len();
        tracing::info!("Render start. Queue len: {}", queue_len);

        for i in 0..UPDATES_PER_RENDER {
            if let Some(frame) = self.frames.pop() {
                gl::resize_texture(self.texture.get(), frame.full_width, frame.full_height);

                let upload_start = std::time::Instant::now();
                gl::update_texture(
                    self.texture.get(),
                    frame.x,
                    frame.y,
                    frame.width,
                    frame.height,
                    frame.full_width,
                    &frame.buffer,
                );
                tracing::info!(
                    "Frame {}/{}: Texture upload took {:?}",
                    i + 1,
                    queue_len,
                    upload_start.elapsed()
                );

                gl::resize_viewport(
                    frame.full_width * scale_factor,
                    frame.full_height * scale_factor,
                );

                gl::draw_texture(
                    self.program.get(),
                    self.texture.get(),
                    self.texture_uniform.get(),
                    self.vao.get(),
                );

                self.update_fps();
            } else {
                break;
            }
        }

        tracing::info!("Render finished in {:?}", start.elapsed());

        Propagation::Proceed
    }
}

impl WebView {
    fn update_fps(&self) {
        let now = glib::monotonic_time();
        let last_time = self.last_frame_time.get();
        let frame_count = self.frame_count.get();

        if now - last_time >= 1_000_000 {
            tracing::debug!("FPS: {}", frame_count);
            self.obj().emit_by_name::<()>("fps-update", &[&frame_count]);
            self.frame_count.set(0);
            self.last_frame_time.set(now);
        } else {
            self.frame_count.set(frame_count + 1);
        }
    }
}
