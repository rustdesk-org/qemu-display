use gst::prelude::*;
use gst_audio::StreamVolumeExt;
use std::thread::{self, JoinHandle};
use std::{collections::HashMap, error::Error};

use qemu_display_listener::{Audio, PCMInfo};

#[derive(Debug)]
struct OutStream {
    pipeline: gst::Pipeline,
    src: gst_app::AppSrc,
    sink: gst::Element,
}

fn pcminfo_as_caps(info: &PCMInfo) -> String {
    let format = format!(
        "{}{}{}",
        if info.is_float {
            "F"
        } else if info.is_signed {
            "S"
        } else {
            "U"
        },
        info.bits,
        if info.be { "BE" } else { "LE" }
    );
    format!(
        "audio/x-raw,format={format},channels={channels},rate={rate},layout=interleaved",
        format = format,
        channels = info.nchannels,
        rate = info.freq,
    )
}

impl OutStream {
    fn new(info: &PCMInfo) -> Result<Self, Box<dyn Error>> {
        let caps = pcminfo_as_caps(info);
        let pipeline = &format!("appsrc name=src is-live=1 do-timestamp=0 format=time caps=\"{}\" ! queue ! audioconvert ! audioresample ! autoaudiosink name=sink", caps);
        let pipeline = gst::parse_launch(pipeline)?;
        let pipeline = pipeline.dynamic_cast::<gst::Pipeline>().unwrap();
        let src = pipeline
            .get_by_name("src")
            .unwrap()
            .dynamic_cast::<gst_app::AppSrc>()
            .unwrap();
        let sink = pipeline.get_by_name("sink").unwrap();
        Ok(Self {
            pipeline,
            src,
            sink,
        })
    }
}

#[derive(Debug)]
pub struct GstAudio {
    out_thread: JoinHandle<()>,
}

impl GstAudio {
    pub fn new(audio: Audio) -> Result<Self, Box<dyn Error>> {
        gst::init()?;

        // TODO audio.listen_in() for capture.
        let rx = audio.listen_out()?;
        let mut out = HashMap::new();
        let out_thread = thread::spawn(move || loop {
            match rx.recv() {
                Ok(event) => {
                    use qemu_display_listener::AudioOutEvent::*;
                    match event {
                        Init { id, info } => {
                            if out.contains_key(&id) {
                                eprintln!("Invalid Init, id {} is already setup", id);
                                continue;
                            }
                            match OutStream::new(&info) {
                                Ok(s) => {
                                    out.insert(id, s);
                                }
                                Err(e) => {
                                    eprintln!("Failed to create stream: {}", e);
                                }
                            }
                        }
                        Fini { id } => {
                            out.remove(&id);
                        }
                        SetEnabled { id, enabled } => {
                            if let Some(s) = out.get(&id) {
                                if let Err(e) = s.pipeline.set_state(if enabled {
                                    gst::State::Playing
                                } else {
                                    gst::State::Ready
                                }) {
                                    eprintln!("Failed to change state: {}", e);
                                }
                            } else {
                                eprintln!("Stream was not setup yet: {}", id);
                            }
                        }
                        SetVolume { id, volume } => {
                            if let Some(s) = out.get(&id) {
                                if let Some(stream_vol) = s
                                    .pipeline
                                    .get_by_interface(gst_audio::StreamVolume::static_type())
                                {
                                    let stream_vol = stream_vol
                                        .dynamic_cast::<gst_audio::StreamVolume>()
                                        .unwrap();
                                    stream_vol.set_mute(volume.mute);
                                    if let Some(vol) = volume.volume.first() {
                                        let vol = *vol as f64 / 255f64;
                                        stream_vol
                                            .set_volume(gst_audio::StreamVolumeFormat::Cubic, vol);
                                    }
                                } else {
                                    eprintln!("Volume not implemented for this pipeline");
                                }
                            } else {
                                eprintln!("Stream was not setup yet: {}", id);
                            }
                        }
                        Write { id, data } => {
                            if let Some(s) = out.get(&id) {
                                let b = gst::Buffer::from_slice(data);
                                let _ = s.src.push_buffer(b);
                            } else {
                                eprintln!("Stream was not setup yet: {}", id);
                            }
                        }
                    }
                }
                Err(e) => eprintln!("Audio thread error: {}", e),
            }
        });
        Ok(Self { out_thread })
    }
}
