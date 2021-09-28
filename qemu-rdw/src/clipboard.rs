use std::{
    error::Error,
    result::Result,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
};

use glib::{clone, SignalHandlerId};
use gtk::{
    gdk, gio, glib,
    prelude::{DisplayExt, *},
};
use qemu_display::{Clipboard, ClipboardHandler, ClipboardProxy, ClipboardSelection};
use rdw::gtk;

#[derive(Debug)]
pub struct Handler {
    #[allow(unused)]
    clipboard: Clipboard,
    cb_handler: Option<SignalHandlerId>,
    cb_primary_handler: Option<SignalHandlerId>,
}

#[derive(Debug)]
struct InnerHandler {
    proxy: ClipboardProxy<'static>,
    serials: Arc<[AtomicU32; 2]>,
}

impl InnerHandler {
    fn reset_serials(&mut self) {
        self.serials[0].store(0, Ordering::SeqCst);
        self.serials[1].store(0, Ordering::SeqCst);
    }
}

#[async_trait::async_trait]
impl ClipboardHandler for InnerHandler {
    async fn register(&mut self) {
        self.reset_serials();
    }

    async fn unregister(&mut self) {
        self.reset_serials();
    }

    async fn grab(&mut self, selection: ClipboardSelection, serial: u32, mimes: Vec<String>) {
        if let Some((clipboard, idx)) = clipboard_from_selection(selection) {
            let cur_serial = self.serials[idx].load(Ordering::SeqCst);
            if serial < cur_serial {
                log::debug!("Ignored peer grab: {} < {}", serial, cur_serial);
                return;
            }

            self.serials[idx].store(serial, Ordering::SeqCst);
            let m: Vec<_> = mimes.iter().map(|s| s.as_str()).collect();
            let p = self.proxy.clone();
            let content = rdw::ContentProvider::new(&m, move |mime, stream, prio| {
                log::debug!("content-provider-write: {:?}", (mime, stream));

                let p = p.clone();
                let mime = mime.to_string();
                Some(Box::pin(
                    clone!(@strong stream => @default-return panic!(), async move {
                        match p.request(selection, &[&mime]).await {
                            Ok((_, data)) => {
                                let bytes = glib::Bytes::from(&data);
                                stream.write_bytes_async_future(&bytes, prio).await.map(|_| ())
                            }
                            Err(e) => {
                                let err = format!("failed to request clipboard data: {}", e);
                                log::warn!("{}", err);
                                Err(glib::Error::new(gio::IOErrorEnum::Failed, &err))
                            }
                        }
                    }),
                ))
            });

            if let Err(e) = clipboard.set_content(Some(&content)) {
                log::warn!("Failed to set clipboard grab: {}", e);
            }
        }
    }

    async fn release(&mut self, selection: ClipboardSelection) {
        if let Some((clipboard, _)) = clipboard_from_selection(selection) {
            // TODO: track if the outside/app changed the clipboard
            if let Err(e) = clipboard.set_content(gdk::NONE_CONTENT_PROVIDER) {
                log::warn!("Failed to release clipboard: {}", e);
            }
        }
    }

    async fn request(
        &mut self,
        selection: ClipboardSelection,
        mimes: Vec<String>,
    ) -> qemu_display::Result<(String, Vec<u8>)> {
        // we have to spawn a local future, because clipboard is not Send
        let (sender, receiver) = futures::channel::oneshot::channel();
        glib::MainContext::default().spawn_local(async move {
            let res = if let Some((clipboard, _)) = clipboard_from_selection(selection) {
                let m: Vec<_> = mimes.iter().map(|s| s.as_str()).collect();
                let res = clipboard
                    .read_async_future(&m, glib::Priority::default())
                    .await;
                log::debug!("clipboard-read: {}", res.is_ok());
                match res {
                    Ok((stream, mime)) => {
                        let out = gio::MemoryOutputStream::new_resizable();
                        let res = out
                            .splice_async_future(
                                &stream,
                                gio::OutputStreamSpliceFlags::CLOSE_SOURCE
                                    | gio::OutputStreamSpliceFlags::CLOSE_TARGET,
                                glib::Priority::default(),
                            )
                            .await;
                        match res {
                            Ok(_) => {
                                let data = out.steal_as_bytes();
                                Ok((mime.to_string(), data.as_ref().to_vec()))
                            }
                            Err(e) => Err(qemu_display::Error::Failed(format!("{}", e))),
                        }
                    }
                    Err(e) => Err(qemu_display::Error::Failed(format!("{}", e))),
                }
            } else {
                Err(qemu_display::Error::Failed(
                    "Clipboard request failed".into(),
                ))
            };
            sender.send(res).unwrap()
        });
        match receiver.await {
            Ok(res) => res,
            Err(e) => Err(qemu_display::Error::Failed(format!(
                "Clipboard request failed: {}",
                e
            ))),
        }
    }
}

impl Handler {
    pub async fn new(clipboard: Clipboard) -> Result<Handler, Box<dyn Error>> {
        let proxy = clipboard.proxy.clone();
        let serials = Arc::new([AtomicU32::new(0), AtomicU32::new(0)]);
        let cb_handler = watch_clipboard(
            clipboard.proxy.clone(),
            ClipboardSelection::Clipboard,
            serials.clone(),
        );
        let cb_primary_handler = watch_clipboard(
            clipboard.proxy.clone(),
            ClipboardSelection::Primary,
            serials.clone(),
        );
        clipboard.register(InnerHandler { proxy, serials }).await?;
        Ok(Handler {
            clipboard,
            cb_handler,
            cb_primary_handler,
        })
    }
}

impl Drop for Handler {
    fn drop(&mut self) {
        if let Some(id) = self.cb_primary_handler.take() {
            clipboard_from_selection(ClipboardSelection::Primary)
                .unwrap()
                .0
                .disconnect(id);
        }
        if let Some(id) = self.cb_handler.take() {
            clipboard_from_selection(ClipboardSelection::Clipboard)
                .unwrap()
                .0
                .disconnect(id);
        }
    }
}

fn watch_clipboard(
    proxy: ClipboardProxy<'static>,
    selection: ClipboardSelection,
    serials: Arc<[AtomicU32; 2]>,
) -> Option<SignalHandlerId> {
    let (clipboard, idx) = match clipboard_from_selection(selection) {
        Some(it) => it,
        None => return None,
    };

    let id = clipboard.connect_changed(move |clipboard| {
        if clipboard.is_local() {
            return;
        }

        if let Some(formats) = clipboard.formats() {
            let types = formats.mime_types();
            log::debug!(">clipboard-changed({:?}): {:?}", selection, types);
            let proxy = proxy.clone();
            let serials = serials.clone();
            glib::MainContext::default().spawn_local(async move {
                if types.is_empty() {
                    let _ = proxy.release(selection).await;
                } else {
                    let mimes: Vec<_> = types.iter().map(|s| s.as_str()).collect();
                    let ser = serials[idx].load(Ordering::SeqCst);
                    let _ = proxy.grab(selection, ser, &mimes).await;
                    serials[idx].store(ser + 1, Ordering::SeqCst);
                }
            });
        }
    });
    Some(id)
}

fn clipboard_from_selection(selection: ClipboardSelection) -> Option<(gdk::Clipboard, usize)> {
    let display = match gdk::Display::default() {
        Some(display) => display,
        None => return None,
    };

    match selection {
        ClipboardSelection::Clipboard => Some((display.clipboard(), 0)),
        ClipboardSelection::Primary => Some((display.primary_clipboard(), 1)),
        _ => {
            log::warn!("Unsupport clipboard selection: {:?}", selection);
            None
        }
    }
}
