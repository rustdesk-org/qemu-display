use std::cell::Cell;
use std::error::Error;
use std::rc::Rc;
use std::result::Result;

use crate::glib::{self, clone, prelude::*, SignalHandlerId, SourceId};
use gtk::{gdk, gio, prelude::DisplayExt, prelude::*};
use qemu_display_listener::{
    self as qdl, AsyncClipboardProxy, Clipboard, ClipboardEvent, ClipboardSelection,
};

#[derive(Debug)]
pub struct Handler {
    rx: SourceId,
    cb_handler: Option<SignalHandlerId>,
    cb_primary_handler: Option<SignalHandlerId>,
}

impl Handler {
    pub async fn new(conn: &zbus::azync::Connection) -> Result<Self, Box<dyn Error>> {
        let ctxt = Clipboard::new(conn).await?;

        let rx = ctxt
            .glib_listen()
            .await
            .expect("Failed to listen to the clipboard");
        let proxy = ctxt.proxy.clone();
        let serial = Rc::new(Cell::new(0));
        let current_serial = serial.clone();
        let rx = rx.attach(None, move |evt| {
            use ClipboardEvent::*;

            log::debug!("Clipboard event: {:?}", evt);
            match evt {
                Register | Unregister => {
                    current_serial.set(0);
                }
                Grab { serial, .. } if serial < current_serial.get() => {
                    log::debug!("Ignored peer grab: {} < {}", serial, current_serial.get());
                }
                Grab {
                    selection,
                    serial,
                    mimes,
                } => {
                    current_serial.set(serial);
                    if let Some(clipboard) = clipboard_from_selection(selection) {
                        let m: Vec<_> = mimes.iter().map(|s|s.as_str()).collect();
                        let p = proxy.clone();
                        let content = rdw::ContentProvider::new(&m, move |mime, stream, prio| {
                            log::debug!("content-provider-write: {:?}", (mime, stream));

                            let p = p.clone();
                            let mime = mime.to_string();
                            Some(Box::pin(clone!(@strong stream => @default-return panic!(), async move {
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
                            })))
                        });

                        if let Err(e) = clipboard.set_content(Some(&content)) {
                            log::warn!("Failed to set clipboard grab: {}", e);
                        }
                    }
                }
                Release { selection } => {
                    if let Some(clipboard) = clipboard_from_selection(selection) {
                        // TODO: track if the outside/app changed the clipboard
                        if let Err(e) = clipboard.set_content(gdk::NONE_CONTENT_PROVIDER) {
                            log::warn!("Failed to release clipboard: {}", e);
                        }
                    }
                }
                Request { selection, mimes, tx } => {
                    if let Some(clipboard) = clipboard_from_selection(selection) {
                        glib::MainContext::default().spawn_local(async move {
                            let m: Vec<_> = mimes.iter().map(|s|s.as_str()).collect();
                            let res = clipboard.read_async_future(&m, glib::Priority::default()).await;
                            log::debug!("clipboard-read: {}", res.is_ok());
                            let reply = match res {
                                Ok((stream, mime)) => {
                                    let out = gio::MemoryOutputStream::new_resizable();
                                    let res = out.splice_async_future(
                                        &stream,
                                        gio::OutputStreamSpliceFlags::CLOSE_SOURCE | gio::OutputStreamSpliceFlags::CLOSE_TARGET,
                                        glib::Priority::default()).await;
                                    match res {
                                        Ok(_) => {
                                            let data = out.steal_as_bytes();
                                            Ok((mime.to_string(), data.as_ref().to_vec()))
                                        }
                                        Err(e) => {
                                            Err(qdl::Error::Failed(format!("{}", e)))
                                        }
                                    }
                                }
                                Err(e) => {
                                    Err(qdl::Error::Failed(format!("{}", e)))
                                }
                            };
                            let _ = tx.lock().unwrap().send(reply);
                        });
                    }
                }
            }
            Continue(true)
        });

        let cb_handler = watch_clipboard(
            ctxt.proxy.clone(),
            ClipboardSelection::Clipboard,
            serial.clone(),
        );
        let cb_primary_handler = watch_clipboard(
            ctxt.proxy.clone(),
            ClipboardSelection::Primary,
            serial.clone(),
        );

        ctxt.register().await?;
        Ok(Self {
            rx,
            cb_handler,
            cb_primary_handler,
        })
    }
}

fn watch_clipboard(
    proxy: AsyncClipboardProxy<'static>,
    selection: ClipboardSelection,
    serial: Rc<Cell<u32>>,
) -> Option<SignalHandlerId> {
    let clipboard = match clipboard_from_selection(selection) {
        Some(clipboard) => clipboard,
        None => return None,
    };

    let id = clipboard.connect_changed(move |clipboard| {
        if clipboard.is_local() {
            return;
        }

        if let Some(formats) = clipboard.formats() {
            let types = formats.mime_types();
            log::debug!(">clipboard-changed({:?}): {:?}", selection, types);
            let proxy_clone = proxy.clone();
            let serial = serial.clone();
            glib::MainContext::default().spawn_local(async move {
                if types.is_empty() {
                    let _ = proxy_clone.release(selection).await;
                } else {
                    let mimes: Vec<_> = types.iter().map(|s| s.as_str()).collect();
                    let ser = serial.get();
                    let _ = proxy_clone.grab(selection, ser, &mimes).await;
                    serial.set(ser + 1);
                }
            });
        }
    });
    Some(id)
}

fn clipboard_from_selection(selection: ClipboardSelection) -> Option<gdk::Clipboard> {
    let display = match gdk::Display::default() {
        Some(display) => display,
        None => return None,
    };

    match selection {
        ClipboardSelection::Clipboard => Some(display.clipboard()),
        ClipboardSelection::Primary => Some(display.primary_clipboard()),
        _ => {
            log::warn!("Unsupport clipboard selection: {:?}", selection);
            None
        }
    }
}
