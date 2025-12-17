mod browser_process_handler;
mod client;
mod process;
mod render_process_handler;

use std::sync::{Arc, Mutex, RwLock};

use cef::{rc::*, *};
use flume::Sender;

use crate::chromium::{
    ChromiumEvent, app::render_process_handler::ChromiumRenderProcessHandler, config::CMD_SWITCHES,
    types::Viewport,
};
use crate::shared::pbo_manager::{BufferPool, PboManager};
use browser_process_handler::ChromiumBrowserProcessHandler;

wrap_app! {
    pub struct ChromiumApp {
        browser: Arc<Mutex<Option<Browser>>>,
        viewport: Arc<RwLock<Viewport>>,
        sender: Sender<ChromiumEvent>,
        pbo_manager: Arc<PboManager>,
        buffer_pool: Arc<BufferPool>,
    }

    impl App {
        fn on_before_command_line_processing(
            &self,
            _process_type: Option<&CefString>,
            command_line: Option<&mut CommandLine>,
        ) {
            if let Some(line) = command_line {
                CMD_SWITCHES.iter().for_each(|switch| {
                    if let Some((key, value)) = switch.split_once('=') {
                        line.append_switch_with_value(
                            Some(&CefString::from(key)),
                            Some(&CefString::from(value)),
                        );
                    } else {
                        line.append_switch(Some(&CefString::from(switch.to_owned())));
                    }
                });

                // Manual overrides if needed (currently handling everything via config.rs)
                line.append_switch(Some(&CefString::from("disable-site-isolation-trials")));
                line.append_switch(Some(&CefString::from("disable-extensions")));
                line.append_switch(Some(&CefString::from("no-zygote")));
                line.append_switch(Some(&CefString::from("no-proxy-server")));
                line.append_switch(Some(&CefString::from("ignore-certificate-errors")));
                line.append_switch(Some(&CefString::from("disable-web-security")));
                line.append_switch(Some(&CefString::from("allow-running-insecure-content")));

                // High-Performance GPU Flags
                line.append_switch(Some(&CefString::from("enable-gpu-rasterization")));
                line.append_switch(Some(&CefString::from("enable-zero-copy")));
                line.append_switch(Some(&CefString::from("ignore-gpu-blocklist")));
                line.append_switch(Some(&CefString::from("disable-gpu-driver-bug-workarounds")));

                // Reduce compositor stalls
                // Removed disable-threaded-animation to allow off-thread animations
                // Removed disable-checker-imaging
                // Removed disable-image-animation-resync
                line.append_switch(Some(&CefString::from("disable-renderer-backgrounding")));

                // Improve JavaScript scheduling
                line.append_switch(Some(&CefString::from("disable-background-timer-throttling")));
                line.append_switch(Some(&CefString::from("disable-backgrounding-occluded-windows")));

                // Disable features that can cause stalls
                line.append_switch(Some(&CefString::from("disable-software-rasterizer")));
                line.append_switch(Some(&CefString::from("disable-smooth-scrolling")));

                use crate::shared::types::SCALE_FACTOR;
                let scale_factor = SCALE_FACTOR.load(std::sync::atomic::Ordering::Relaxed);
                let scale = f64::from_bits(scale_factor);

                if scale > 0.1 {
                    let switch = CefString::from("force-device-scale-factor");
                    let value = CefString::from(scale.to_string().as_str());
                    line.append_switch_with_value(Some(&switch), Some(&value));
                }
            }
        }

        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(ChromiumBrowserProcessHandler::new(
                self.browser.clone(),
                self.viewport.clone(),
                self.sender.clone(),
                self.pbo_manager.clone(),
                self.buffer_pool.clone(),
            ))
        }

        fn render_process_handler(&self) -> Option<RenderProcessHandler> {
            Some(ChromiumRenderProcessHandler::new())
        }
    }
}
