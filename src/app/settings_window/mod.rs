mod imp;

use glib::Object;
use gtk::{glib, prelude::*};

glib::wrapper! {
    pub struct SettingsWindow(ObjectSubclass<imp::SettingsWindow>)
        @extends gtk::Widget, adw::Dialog, adw::PreferencesDialog,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl SettingsWindow {
    pub fn new(app: &impl IsA<gtk::Application>) -> Self {
        Object::builder().property("application", app).build()
    }
}

impl Default for SettingsWindow {
    fn default() -> Self {
        Object::builder().build()
    }
}

use gtk::glib::subclass::Signal;
use std::sync::OnceLock;

impl SettingsWindow {
    pub fn signals() -> &'static [Signal] {
        static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
        SIGNALS.get_or_init(|| {
            vec![
                Signal::builder("fps-toggled")
                    .param_types([bool::static_type()])
                    .build(),
            ]
        })
    }
}
