use std::{error::Error, result::Result};

use qemu_display::{Audio, AudioOutHandler};

#[derive(Debug)]
pub struct Handler {
    #[allow(unused)]
    audio: Audio,
}

#[derive(Debug, Default)]
pub struct OutListener {
    gst: rdw::GstAudio,
}

#[async_trait::async_trait]
impl AudioOutHandler for OutListener {
    async fn init(&mut self, id: u64, info: qemu_display::PCMInfo) {
        if let Err(e) = self.gst.init_out(id, &info.gst_caps()) {
            log::warn!("Failed to initialize audio stream: {}", e);
        }
    }

    async fn fini(&mut self, id: u64) {
        self.gst.fini_out(id);
    }

    async fn set_enabled(&mut self, id: u64, enabled: bool) {
        if let Err(e) = self.gst.set_enabled_out(id, enabled) {
            log::warn!("Failed to set enabled audio stream: {}", e);
        }
    }

    async fn set_volume(&mut self, id: u64, volume: qemu_display::Volume) {
        if let Err(e) = self.gst.set_volume_out(
            id,
            volume.mute,
            volume.volume.first().map(|v| *v as f64 / 255f64),
        ) {
            log::warn!("Failed to set volume: {}", e);
        }
    }

    async fn write(&mut self, id: u64, data: Vec<u8>) {
        if let Err(e) = self.gst.write_out(id, data) {
            log::warn!("Failed to output stream: {}", e);
        }
    }
}

impl Handler {
    pub async fn new(mut audio: Audio) -> Result<Handler, Box<dyn Error>> {
        let gst = rdw::GstAudio::new()?;
        audio.register_out_listener(OutListener { gst }).await?;
        Ok(Handler { audio })
    }
}
