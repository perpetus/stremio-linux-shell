mod imp;

use adw::subclass::prelude::*;
use gtk::{
    Widget,
    gdk::prelude::{DisplayExt, MonitorExt},
    gio,
    glib::{
        self,
        object::{IsA, ObjectExt},
    },
    prelude::{GtkWindowExt, NativeExt, WidgetExt},
};
use url::Url;

use crate::app::Application;

glib::wrapper! {
    pub struct Window(ObjectSubclass<imp::Window>)
    @extends gtk::Window, gtk::ApplicationWindow, adw::ApplicationWindow, gtk::Widget,
    @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager, gtk::Native, gtk::Root;
}

impl Window {
    pub fn new(application: &Application) -> Self {
        glib::Object::builder()
            .property("application", application)
            .build()
    }

    pub fn set_underlay(&self, widget: &impl IsA<Widget>) {
        let window = self.imp();

        window.overlay.set_child(Some(&graphics_offload(widget)));
    }

    pub fn set_overlay(&self, widget: &impl IsA<Widget>) {
        let window = self.imp();
        let offload = graphics_offload(widget);

        window.overlay.add_overlay(&offload);
        window
            .fps_label
            .insert_after(&*window.overlay, Some(&offload));
    }

    pub fn set_fullscreen(&self, fullscreen: bool) {
        let window = self.imp();

        window.header.set_visible(!fullscreen);
        self.set_fullscreened(fullscreen);
    }

    pub fn connect_monitor_info<T: Fn(f64, i32, f64) + 'static>(&self, callback: T) {
        let callback = std::rc::Rc::new(callback);

        self.connect_realize(glib::clone!(
            #[weak(rename_to = window)]
            self,
            #[strong]
            callback,
            move |_| {
                let display = window.display();
                if let Some(monitor) = display.monitor_at_surface(&window.surface().unwrap()) {
                    let refresh_rate = monitor.refresh_rate() as f64 / 1000.0;
                    let scale_factor = window.scale_factor();

                    let mut zoom_level = 0.0;
                    let width_mm = monitor.width_mm();
                    let width_px = monitor.geometry().width();
                    if scale_factor == 1 && width_mm > 0 {
                        let dpi = width_px as f64 / (width_mm as f64 / 25.4);
                        if dpi > 150.0 {
                            zoom_level = 2.0;
                        }
                    }

                    callback(refresh_rate, scale_factor, zoom_level);
                    tracing::info!(
                        "Monitor info initialized: refresh_rate={}, scale_factor={}, zoom_level={}",
                        refresh_rate,
                        scale_factor,
                        zoom_level
                    );
                }
            }
        ));

        self.connect_notify_local(
            Some("scale-factor"),
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                #[strong]
                callback,
                move |_, _| {
                    let display = window.display();
                    if let Some(monitor) = display.monitor_at_surface(&window.surface().unwrap()) {
                        let refresh_rate = monitor.refresh_rate() as f64 / 1000.0;
                        let scale_factor = window.scale_factor();

                        let mut zoom_level = 0.0;
                        // DPI heuristic: detect HiDPI screens incorrectly reporting scale 1
                        let width_mm = monitor.width_mm();
                        let width_px = monitor.geometry().width();
                        if scale_factor == 1 && width_mm > 0 {
                            let dpi = width_px as f64 / (width_mm as f64 / 25.4);
                            if dpi > 150.0 {
                                zoom_level = 3.0;
                            }
                        }

                        callback(refresh_rate, scale_factor, zoom_level);
                        tracing::info!(
                            "Monitor info updated: refresh_rate={}, scale_factor={}, zoom_level={}",
                            refresh_rate,
                            scale_factor,
                            zoom_level
                        );
                    }
                }
            ),
        );
    }

    pub fn connect_visibility<T: Fn(bool) + 'static>(&self, callback: T) {
        self.connect_visible_notify(move |window| {
            callback(window.is_visible());
        });
    }

    fn request_backgound(&self) {
        self.imp().request_backgound();
    }

    pub fn disable_idling(&self) {
        self.imp().disable_idling();
    }

    pub fn enable_idling(&self) {
        self.imp().enable_idling();
    }

    pub fn open_uri(&self, uri: Url) {
        self.imp().open_uri(uri);
    }

    pub fn set_fps_visible(&self, visible: bool) {
        self.imp().fps_label.set_visible(visible);
    }

    pub fn get_fps_visible(&self) -> bool {
        self.imp().fps_label.is_visible()
    }

    pub fn set_fps(&self, fps: u32) {
        let label = &self.imp().fps_label;
        if label.is_mapped() {
            label.set_label(&format!("FPS: {fps}"));
        }
    }
}

fn graphics_offload(widget: &impl IsA<Widget>) -> gtk::GraphicsOffload {
    gtk::GraphicsOffload::builder()
        .vexpand(true)
        .hexpand(true)
        .child(widget)
        .build()
}
