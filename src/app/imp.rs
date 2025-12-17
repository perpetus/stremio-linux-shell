use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use adw::{prelude::*, subclass::prelude::*};

use gtk::glib::{self, ControlFlow, Properties, clone};

use crate::{
    app::{
        config::URI_SCHEME, settings_window::SettingsWindow, tray::Tray, video::Video,
        webview::WebView, window::Window,
    },
    chromium::{Chromium, ChromiumEvent},
    discord::adapter::DiscordAdapter,
    mpris::{adapter::MprisAdapter, metadata},
    shared::{
        ipc::{
            self,
            event::{IpcEvent, IpcEventMpv},
        },
        types::{MprisCommand, SCALE_FACTOR, UserEvent},
    },
};

#[derive(Properties, Default)]
#[properties(wrapper_type = super::Application)]
pub struct Application {
    #[property(get, set)]
    dev_mode: Cell<bool>,
    #[property(get, set)]
    startup_url: RefCell<String>,
    #[property(get, set)]
    open_uri: RefCell<Option<String>>,
    #[property(get, set)]
    decorations: Cell<bool>,
    tray: RefCell<Option<Tray>>,
    browser: Rc<RefCell<Option<Chromium>>>,
    deeplink: Rc<RefCell<Option<String>>>,
    mpris_adapter: Rc<RefCell<Option<MprisAdapter>>>,
    discord_adapter: Rc<RefCell<Option<DiscordAdapter>>>,
}

impl Application {
    pub fn set_browser(&self, browser: Chromium) {
        *self.browser.borrow_mut() = Some(browser);
    }
}

#[glib::object_subclass]
impl ObjectSubclass for Application {
    const NAME: &'static str = "Application";
    type Type = super::Application;
    type ParentType = adw::Application;
}

#[glib::derived_properties]
impl ObjectImpl for Application {}

impl ApplicationImpl for Application {
    fn startup(&self) {
        self.parent_startup();

        let app = self.obj();
        app.setup_actions();
        app.setup_accels();

        let settings_action = gtk::gio::SimpleAction::new("settings", None);
        settings_action.connect_activate(clone!(
            #[weak]
            app,
            move |_, _| {
                if let Some(main_window) = app
                    .active_window()
                    .and_then(|w| w.downcast::<Window>().ok())
                {
                    let window = glib::Object::new::<SettingsWindow>();
                    window.set_fps_active(main_window.get_fps_visible());

                    let discord_active = app
                        .imp()
                        .discord_adapter
                        .borrow()
                        .as_ref()
                        .map(|d| d.is_connected())
                        .unwrap_or(false);
                    window.set_discord_active(discord_active);

                    window.present(Some(&main_window));

                    let main_window_weak = main_window.downgrade();
                    window.connect_closure(
                        "fps-toggled",
                        false,
                        glib::closure_local!(move |_: SettingsWindow, active: bool| {
                            if let Some(main_window) = main_window_weak.upgrade() {
                                main_window.set_fps_visible(active);
                            }
                        }),
                    );

                    let discord_adapter = app.imp().discord_adapter.clone();
                    window.connect_closure(
                        "discord-toggled",
                        false,
                        glib::closure_local!(move |_: SettingsWindow, active: bool| {
                            let mut adapter_lock = discord_adapter.borrow_mut();
                            if let Some(adapter) = adapter_lock.as_mut() {
                                if active {
                                    let _ = adapter.connect();
                                } else {
                                    let _ = adapter.disconnect();
                                }
                            } else if active {
                                // Lazy init if not exists? (Should exist though)
                                if let Ok(mut adapter) = DiscordAdapter::new("1450906751607111781")
                                {
                                    let _ = adapter.connect();
                                    *adapter_lock = Some(adapter);
                                }
                            }
                        }),
                    );
                }
            }
        ));
        app.add_action(&settings_action);
        app.set_accels_for_action("app.settings", &["<Ctrl>comma"]);

        if let Some(ref mut browser) = *self.browser.borrow_mut() {
            browser.start();
        }
    }

    fn activate(&self) {
        self.parent_activate();

        let app = self.obj();

        if let Some(window) = app.active_window() {
            window.present();
            return;
        }

        let tray = Tray::default();
        let video = Video::default();
        let webview = WebView::default();
        let window = Window::new(&app);
        window.set_property("decorations", self.decorations.get());
        window.set_underlay(&video);
        window.set_overlay(&webview);

        let (mpris_sender, mpris_receiver) = flume::unbounded::<UserEvent>();
        let adapter = MprisAdapter::new(mpris_sender.clone());
        *self.mpris_adapter.borrow_mut() = Some(adapter);

        // Initialize Discord adapter (disconnected)
        if let Ok(discord) = DiscordAdapter::new("1450906751607111781") {
            *self.discord_adapter.borrow_mut() = Some(discord);
        }

        let mpris_adapter_ref = self.mpris_adapter.clone();
        let discord_adapter_ref = self.discord_adapter.clone();
        glib::MainContext::default().spawn_local(clone!(
            #[weak]
            video,
            #[strong]
            mpris_adapter_ref,
            #[strong]
            discord_adapter_ref,
            async move {
                while let Ok(event) = mpris_receiver.recv_async().await {
                    let mut adapter_lock = mpris_adapter_ref.borrow_mut();
                    if let Some(adapter) = adapter_lock.as_mut() {
                        match event {
                            UserEvent::MetadataUpdate {
                                title,
                                artist,
                                poster,
                                thumbnail,
                                logo,
                            } => {
                                adapter.update_metadata(
                                    title.clone(),
                                    artist.clone(),
                                    poster.clone(),
                                    thumbnail,
                                    logo.clone(),
                                );
                                if let Some(discord) = discord_adapter_ref.borrow_mut().as_mut() {
                                    tracing::info!(
                                        "Discord RPC: updating activity for '{}' (Logo: {:?})",
                                        title.as_deref().unwrap_or("Unknown"),
                                        logo
                                    );
                                    // User requested Logo instead of Poster
                                    let _ = discord.update_activity(
                                        title.as_deref(),
                                        artist.as_deref(),
                                        logo.as_deref(),
                                    );
                                }
                            }
                            UserEvent::MprisCommand(cmd) => match cmd {
                                MprisCommand::Play => video
                                    .send_command("set".into(), vec!["pause".into(), "no".into()]),
                                MprisCommand::Pause => video
                                    .send_command("set".into(), vec!["pause".into(), "yes".into()]),
                                MprisCommand::PlayPause => {
                                    video.send_command("cycle".into(), vec!["pause".into()])
                                }
                                MprisCommand::Stop => video.send_command("stop".into(), vec![]),
                                MprisCommand::Seek(offset) => {
                                    let seconds = offset as f64 / 1_000_000.0;
                                    video.send_command(
                                        "seek".into(),
                                        vec![format!("{}", seconds), "relative".into()],
                                    )
                                }
                                MprisCommand::SetPosition(pos) => {
                                    let seconds = pos as f64 / 1_000_000.0;
                                    video.send_command(
                                        "seek".into(),
                                        vec![format!("{}", seconds), "absolute".into()],
                                    )
                                }
                                MprisCommand::SetRate(rate) => video.send_command(
                                    "set".into(),
                                    vec!["speed".into(), format!("{}", rate)],
                                ),
                                _ => {}
                            },
                            _ => {}
                        }
                    }
                }
            }
        ));

        let browser = self.browser.clone();
        window.connect_monitor_info(clone!(
            #[weak]
            video,
            #[weak]
            webview,
            move |refresh_rate, scale_factor, zoom_level| {
                SCALE_FACTOR.store(
                    f64::from(scale_factor).to_bits(),
                    std::sync::atomic::Ordering::Relaxed,
                );
                if let Some(ref browser) = *browser.borrow() {
                    browser.set_monitor_info(refresh_rate, scale_factor);
                    browser.set_zoom(zoom_level);
                    video.set_property("scale-factor", scale_factor);
                    webview.set_property("scale-factor", scale_factor);
                }
            }
        ));

        let window_weak = window.downgrade();
        webview.connect_closure(
            "fps-update",
            false,
            glib::closure_local!(move |_: super::webview::WebView, fps: u32| {
                if let Some(window) = window_weak.upgrade() {
                    window.set_fps(fps);
                }
            }),
        );

        video.connect_playback_started(clone!(
            #[weak]
            window,
            move || {
                window.disable_idling();
            }
        ));

        video.connect_playback_ended(clone!(
            #[weak]
            window,
            move || {
                window.enable_idling();
            }
        ));

        let browser = self.browser.clone();
        let mpris_adapter_ref = self.mpris_adapter.clone();
        let discord_adapter_ref = self.discord_adapter.clone();
        let mpris_sender_mpv = mpris_sender.clone();
        video.connect_mpv_property_change(move |name, value| {
            if let Some(ref browser) = *browser.borrow() {
                let message = ipc::create_response(IpcEvent::Mpv(IpcEventMpv::Change((
                    name.to_string(),
                    value.clone(),
                ))));

                browser.post_message(message);
            }

            let mut adapter_lock = mpris_adapter_ref.borrow_mut();
            if let Some(adapter) = adapter_lock.as_mut() {
                match name {
                    "pause" => {
                        if let Some(paused) = value.as_bool() {
                            adapter.update_playback_status(if paused {
                                "Paused"
                            } else {
                                "Playing"
                            });
                        }
                    }
                    "media-title" => {
                        if let Some(title) = value.as_str() {
                            let clean_title = if title.starts_with("file://")
                                || title.contains("&tr=")
                                || title.contains("announce")
                                || title.contains("dht:")
                            {
                                "Stremio".to_string()
                            } else {
                                title.to_string()
                            };

                            if !adapter.rich_metadata_active {
                                adapter.update_metadata_simple(
                                    Some(clean_title.clone()),
                                    None,
                                    None,
                                    None,
                                );
                            }

                            if let Some(discord) = discord_adapter_ref.borrow_mut().as_mut() {
                                let _ = discord.update_activity(Some(&clean_title), None, None);
                            }

                            metadata::fetch_metadata(clean_title, mpris_sender_mpv.clone());
                        }
                    }
                    "duration" => {
                        if let Some(d) = value.as_f64() {
                            adapter.update_metadata_simple(None, None, None, Some(d));
                        }
                    }
                    "time-pos" => {
                        if let Some(p) = value.as_f64() {
                            adapter.update_position(p);
                        }
                    }
                    "sid" => {
                        if let Some(sid) = value.as_str() {
                            metadata::fetch_metadata_by_sid(
                                sid.to_string(),
                                mpris_sender_mpv.clone(),
                            );
                        }
                    }
                    _ => {}
                }
            }
        });

        let browser = self.browser.clone();
        let dev_mode = self.dev_mode.get();
        let startup_url = self.startup_url.clone();
        let open_uri = self.open_uri.clone();
        let deeplink = self.deeplink.clone();
        glib::timeout_add_local(
            std::time::Duration::from_millis(5),
            clone!(
                #[weak]
                webview,
                #[weak]
                video,
                #[weak]
                window,
                #[weak]
                app,
                #[upgrade_or]
                ControlFlow::Continue,
                move || {
                    if let Some(ref browser) = *browser.borrow() {
                        browser.on_event(|event| match event {
                            ChromiumEvent::Ready => {
                                browser.dev_tools(dev_mode);
                                browser.load_url(&startup_url.borrow());
                            }
                            ChromiumEvent::Loaded => {
                                if let Some(ref uri) = *open_uri.borrow()
                                    && uri.starts_with(URI_SCHEME)
                                {
                                    let message =
                                        ipc::create_response(IpcEvent::OpenMedia(uri.to_string()));
                                    browser.post_message(message);
                                }
                            }
                            ChromiumEvent::Fullscreen(state) => window.set_fullscreen(state),
                            ChromiumEvent::Render(frame) => webview.render(frame),
                            ChromiumEvent::Open(url) => window.open_uri(url),
                            ChromiumEvent::Ipc(message) => {
                                if let Ok(event) = ipc::parse_request(&message) {
                                    match event {
                                        IpcEvent::Init => {
                                            let message = ipc::create_response(IpcEvent::Init);
                                            browser.post_message(message);
                                        }
                                        IpcEvent::Ready => {
                                            if let Some(ref uri) = *deeplink.borrow() {
                                                let message = ipc::create_response(
                                                    IpcEvent::OpenMedia(uri.to_string()),
                                                );
                                                browser.post_message(message);
                                            }
                                        }
                                        IpcEvent::Quit => {
                                            app.quit();
                                        }
                                        IpcEvent::Fullscreen(state) => {
                                            window.set_fullscreen(state);

                                            let message =
                                                ipc::create_response(IpcEvent::Fullscreen(state));
                                            browser.post_message(message);
                                        }
                                        IpcEvent::Mpv(event) => match event {
                                            IpcEventMpv::Observe(name) => {
                                                video.observe_mpv_property(name)
                                            }
                                            IpcEventMpv::Command((name, args)) => {
                                                video.send_command(name, args)
                                            }
                                            IpcEventMpv::Set((name, value)) => {
                                                video.set_mpv_property(name, value)
                                            }
                                            _ => {}
                                        },
                                        IpcEvent::MetadataUpdate(data) => {
                                            mpris_sender
                                                .send(UserEvent::MetadataUpdate {
                                                    title: data.title,
                                                    artist: data.artist,
                                                    poster: data.poster,
                                                    thumbnail: data.thumbnail,
                                                    logo: data.logo,
                                                })
                                                .ok();
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        });
                    }

                    ControlFlow::Continue
                }
            ),
        );

        let browser = self.browser.clone();
        window.connect_visibility(clone!(
            #[weak]
            tray,
            move |state| {
                if let Some(ref browser) = *browser.borrow() {
                    browser.hidden(!state);

                    let message = ipc::create_response(IpcEvent::Visibility(state));
                    browser.post_message(message);
                }

                tray.update(state);
            }
        ));

        let browser = self.browser.clone();
        webview.connect_has_focus_notify(move |_| {
            if let Some(ref browser) = *browser.borrow() {
                browser.focus(true);
            }
        });

        let browser = self.browser.clone();
        webview.connect_resized(move |width, height| {
            if let Some(ref browser) = *browser.borrow() {
                browser.resize(width, height);
            }
        });

        let browser = self.browser.clone();
        webview.connect_motion(move |pointer_state| {
            if let Some(ref browser) = *browser.borrow() {
                browser.forward_motion(&pointer_state);
            }
        });

        let browser = self.browser.clone();
        webview.connect_scroll(move |pointer_state, delta_x, delta_y| {
            if let Some(ref browser) = *browser.borrow() {
                browser.forward_scroll(&pointer_state, delta_x, delta_y);
            }
        });

        let browser = self.browser.clone();
        webview.connect_click(clone!(
            #[weak]
            webview,
            move |pointer_state, count| {
                if let Some(ref browser) = *browser.borrow() {
                    webview.grab_focus();
                    browser.forward_click(&pointer_state, count);
                }
            }
        ));

        let browser = self.browser.clone();
        webview.connect_keys(clone!(
            #[weak]
            webview,
            move |keyboard_state| {
                if let Some(ref browser) = *browser.borrow() {
                    webview.grab_focus();
                    browser.forward_key(&keyboard_state);
                }
            }
        ));

        let browser = self.browser.clone();
        webview.connect_clipboard(move |text| {
            if let Some(ref browser) = *browser.borrow() {
                browser.clipboard(text);
            }
        });

        let browser = self.browser.clone();
        webview.connect_file_enter(move |pointer_state, path| {
            if let Some(ref browser) = *browser.borrow() {
                browser.forward_file_enter(pointer_state.as_ref(), path);
            }
        });

        let browser = self.browser.clone();
        webview.connect_file_leave(move || {
            if let Some(ref browser) = *browser.borrow() {
                browser.forward_file_leave();
            }
        });

        let browser = self.browser.clone();
        webview.connect_file_motion(move |pointer_state| {
            if let Some(ref browser) = *browser.borrow() {
                browser.forward_file_hover(pointer_state.as_ref());
            }
        });

        let browser = self.browser.clone();
        webview.connect_file_drop(move |pointer_state| {
            if let Some(ref browser) = *browser.borrow() {
                browser.forward_file_drop(pointer_state.as_ref());
            }
        });

        tray.connect_show(clone!(
            #[weak]
            window,
            move || {
                window.set_visible(true);
            }
        ));

        tray.connect_hide(clone!(
            #[weak]
            window,
            move || {
                window.set_visible(false);
            }
        ));

        tray.connect_quit(clone!(
            #[weak]
            app,
            move || {
                app.quit();
            }
        ));

        *self.tray.borrow_mut() = Some(tray);

        window.present();
    }

    fn open(&self, files: &[gtk::gio::File], hint: &str) {
        self.parent_open(files, hint);

        self.activate();

        if let Some(file) = files.first() {
            let uri = file.uri().to_string();
            if uri.starts_with(URI_SCHEME) {
                let mut deeplink = self.deeplink.borrow_mut();
                *deeplink = Some(uri.clone());

                if let Some(ref browser) = *self.browser.borrow() {
                    let message = ipc::create_response(IpcEvent::OpenMedia(uri));
                    browser.post_message(message);
                }
            }
        }
    }

    fn shutdown(&self) {
        if let Some(browser) = self.browser.take() {
            browser.stop();
        }

        self.parent_shutdown();
    }
}

impl GtkApplicationImpl for Application {}
impl AdwApplicationImpl for Application {}
