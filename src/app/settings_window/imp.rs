use adw::subclass::prelude::*;
use gtk::{CompositeTemplate, glib, prelude::*};

use std::sync::OnceLock;

#[derive(Default, CompositeTemplate, glib::Properties)]
#[template(file = "settings_window.ui")]
#[properties(wrapper_type = super::SettingsWindow)]
pub struct SettingsWindow {
    #[template_child]
    pub fps_switch: TemplateChild<gtk::Switch>,
    #[template_child]
    pub cpu_row: TemplateChild<adw::ActionRow>,
    #[template_child]
    pub gpu_row: TemplateChild<adw::ActionRow>,
}

#[glib::object_subclass]
impl ObjectSubclass for SettingsWindow {
    const NAME: &'static str = "SettingsWindow";
    type Type = super::SettingsWindow;
    type ParentType = adw::PreferencesDialog;

    fn class_init(klass: &mut Self::Class) {
        klass.bind_template();
        klass.bind_template_callbacks();
    }

    fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
        obj.init_template();
    }
}

#[glib::derived_properties]
impl ObjectImpl for SettingsWindow {
    fn signals() -> &'static [glib::subclass::Signal] {
        static SIGNALS: OnceLock<Vec<glib::subclass::Signal>> = OnceLock::new();
        SIGNALS.get_or_init(|| {
            vec![
                glib::subclass::Signal::builder("fps-toggled")
                    .param_types([bool::static_type()])
                    .build(),
            ]
        })
    }

    fn constructed(&self) {
        self.parent_constructed();
        self.update_hardware_info();
    }
}

impl SettingsWindow {
    fn update_hardware_info(&self) {
        // CPU
        let cpu = std::fs::read_to_string("/proc/cpuinfo")
            .unwrap_or_default()
            .lines()
            .find(|line| line.starts_with("model name"))
            .and_then(|line| line.split(':').nth(1))
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        self.cpu_row.set_property("subtitle", &cpu);

        // GPU
        let gpu = crate::app::webview::GPU_RENDERER
            .get()
            .map(|s| s.as_str())
            .unwrap_or("Unknown");
        self.gpu_row.set_property("subtitle", gpu);
    }
}

impl WidgetImpl for SettingsWindow {}
impl AdwDialogImpl for SettingsWindow {}
impl PreferencesDialogImpl for SettingsWindow {}

#[gtk::template_callbacks]
impl SettingsWindow {
    #[template_callback]
    fn on_fps_toggled(&self, _pspec: &glib::ParamSpec) {
        let active = self.fps_switch.is_active();
        self.obj().emit_by_name::<()>("fps-toggled", &[&active]);
    }
}
