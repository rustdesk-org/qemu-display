use std::error::Error;
use std::result::Result;
use std::thread;

use qemu_display_listener::Audio;

#[derive(Debug, Default)]
pub struct Handler {
    thread: Option<thread::JoinHandle<()>>,
}

impl Handler {
    pub async fn new(audio: Audio) -> Result<Self, Box<dyn Error>> {
        let rx = audio.listen_out().await?;
        let mut gst = rdw::GstAudio::new()?;

        let thread = thread::spawn(move || loop {
            match rx.recv() {
                Ok(event) => {
                    use qemu_display_listener::AudioOutEvent::*;

                    match event {
                        Init { id, info } => {
                            if let Err(e) = gst.init_out(id, &info.gst_caps()) {
                                log::warn!("Failed to initialize audio stream: {}", e);
                            }
                        }
                        Fini { id } => {
                            gst.fini_out(id);
                        }
                        SetEnabled { id, enabled } => {
                            if let Err(e) = gst.set_enabled_out(id, enabled) {
                                log::warn!("Failed to set enabled audio stream: {}", e);
                            }
                        }
                        SetVolume { id, volume } => {
                            if let Err(e) = gst.set_volume_out(
                                id,
                                volume.mute,
                                volume.volume.first().map(|v| *v as f64 / 255f64),
                            ) {
                                log::warn!("Failed to set volume: {}", e);
                            }
                        }
                        Write { id, data } => {
                            if let Err(e) = gst.write_out(id, data) {
                                log::warn!("Failed to output stream: {}", e);
                            }
                        }
                    }
                }
                Err(e) => log::warn!("Audio thread error: {}", e),
            }
        });

        Ok(Self {
            thread: Some(thread),
        })
    }
}
