use cef::{rc::*, *};
use flume::Sender;
use std::sync::Mutex;

use crate::chromium::{
    ChromiumEvent,
    config::{IPC_RECEIVER, IPC_SCRIPT, IPC_SENDER},
};

static NAV_START: Mutex<Option<std::time::Instant>> = Mutex::new(None);

wrap_load_handler! {
    pub struct ChromiumLoadHandler {
        sender: Sender<ChromiumEvent>,
    }

    impl LoadHandler {
        fn on_load_start(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            transition_type: TransitionType,
        ) {
            if let Some(frame) = frame {
                let url = CefString::from(&frame.url());
                let is_main = frame.is_main() == 1;
                tracing::info!(
                    "OnLoadStart: url={}, is_main={}, transition={:?}",
                    url,
                    is_main,
                    transition_type
                );

                if is_main {
                    // Track navigation start time
                    if let Ok(mut nav) = NAV_START.lock() {
                        *nav = Some(std::time::Instant::now());
                    }
                    let script = IPC_SCRIPT
                        .replace("IPC_SENDER", IPC_SENDER)
                        .replace("IPC_RECEIVER", IPC_RECEIVER);
                    let code = CefString::from(script.as_str());
                    frame.execute_java_script(Some(&code), None, 0);
                }
            }
        }

        fn on_load_end(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            http_status_code: i32,
        ) {
            if let Some(frame) = frame {
                let url = CefString::from(&frame.url());
                let is_main = frame.is_main() == 1;
                tracing::info!(
                    "OnLoadEnd: url={}, is_main={}, status={}",
                    url,
                    is_main,
                    http_status_code
                );

                if is_main {
                    // Calculate navigation duration
                    if let Ok(mut nav) = NAV_START.lock() && let Some(start) = nav.take() {
                        let duration = start.elapsed();
                        if duration.as_secs() > 1 {
                            tracing::error!("NAVIGATION SLOW: {:?} for status={}", duration, http_status_code);
                        } else {
                            tracing::info!("Navigation duration: {:?}, status={}", duration, http_status_code);
                        }
                    }
                    if http_status_code == 200 {
                        self.sender.send(ChromiumEvent::Loaded).ok();
                    }
                }
            }
        }

        fn on_load_error(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            error_code: Errorcode,
            error_text: Option<&CefString>,
            failed_url: Option<&CefString>,
        ) {
            let url_str = frame.map(|f| {
                let url = CefString::from(&f.url());
                format!("{}", url)
            }).unwrap_or_default();
            let error_text_str = error_text.map(|s| format!("{}", s)).unwrap_or_default();
            let failed_url_str = failed_url.map(|s| format!("{}", s)).unwrap_or_default();
            tracing::error!(
                "OnLoadError: url={}, error_code={:?}, text={}, failed_url={}",
                url_str,
                error_code,
                error_text_str,
                failed_url_str
            );
        }
    }
}
