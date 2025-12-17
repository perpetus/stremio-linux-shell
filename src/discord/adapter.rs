use discord_rich_presence::{DiscordIpc, DiscordIpcClient, activity};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct DiscordAdapter {
    client: DiscordIpcClient,
    connected: bool,
}

impl DiscordAdapter {
    pub fn new(client_id: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let client = DiscordIpcClient::new(client_id);
        Ok(Self {
            client,
            connected: false,
        })
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }

    pub fn connect(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.client.connect()?;
        self.connected = true;
        tracing::info!("Discord RPC Connected");
        Ok(())
    }

    pub fn disconnect(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.connected {
            self.client.close()?;
            self.connected = false;
            tracing::info!("Discord RPC Disconnected");
        }
        Ok(())
    }

    pub fn update_activity(
        &mut self,
        title: Option<&str>,
        artist: Option<&str>,
        large_image_url: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !self.connected {
            return Ok(());
        }

        let details = if let Some(t) = title {
            if t.trim().is_empty() {
                "Watching Video"
            } else {
                t
            }
        } else {
            "Watching Video"
        };

        // Discord requires state to be at least 2 chars if set.
        let state = if let Some(a) = artist {
            if a.trim().is_empty() { "Stremio" } else { a }
        } else {
            "Stremio"
        };

        let start_time = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;

        let mut activity = activity::Activity::new()
            .details(details)
            .state(state) // State is mandatory for most RP views or at least good practice
            .timestamps(activity::Timestamps::new().start(start_time));

        if let Some(url) = large_image_url
            && !url.trim().is_empty()
        {
            activity =
                activity.assets(activity::Assets::new().large_image(url).large_text(details));
        }

        tracing::info!(
            "Discord RPC Update: details='{}' state='{}' image='{:?}'",
            details,
            state,
            large_image_url
        );
        if let Err(e) = self.client.set_activity(activity) {
            tracing::error!("Discord RPC Failed to set activity: {:?}", e);
            return Err(Box::new(e));
        }
        Ok(())
    }
}

impl Drop for DiscordAdapter {
    fn drop(&mut self) {
        let _ = self.disconnect();
    }
}
