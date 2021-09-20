use derivative::Derivative;
use std::{
    ops::Drop,
    os::unix::io::{AsRawFd, IntoRawFd, RawFd},
};
use zbus::{dbus_interface, zvariant::Fd};

#[derive(Derivative)]
#[derivative(Debug)]
pub struct Scanout {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: u32,
    #[derivative(Debug = "ignore")]
    pub data: Vec<u8>,
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct Update {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub stride: u32,
    pub format: u32,
    #[derivative(Debug = "ignore")]
    pub data: Vec<u8>,
}

#[derive(Debug)]
pub struct ScanoutDMABUF {
    pub fd: RawFd,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub fourcc: u32,
    pub modifier: u64,
    pub y0_top: bool,
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct Cursor {
    pub width: i32,
    pub height: i32,
    pub hot_x: i32,
    pub hot_y: i32,
    #[derivative(Debug = "ignore")]
    pub data: Vec<u8>,
}

impl Drop for ScanoutDMABUF {
    fn drop(&mut self) {
        if self.fd >= 0 {
            unsafe {
                libc::close(self.fd);
            }
        }
    }
}

impl IntoRawFd for ScanoutDMABUF {
    fn into_raw_fd(mut self) -> RawFd {
        std::mem::replace(&mut self.fd, -1)
    }
}

#[derive(Debug, Copy, Clone)]
pub struct MouseSet {
    pub x: i32,
    pub y: i32,
    pub on: i32,
}

#[derive(Debug, Copy, Clone)]
pub struct UpdateDMABUF {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[async_trait::async_trait]
pub trait ConsoleListenerHandler: 'static + Send + Sync {
    async fn scanout(&mut self, scanout: Scanout);

    async fn update(&mut self, update: Update);

    async fn scanout_dmabuf(&mut self, scanout: ScanoutDMABUF);

    async fn update_dmabuf(&mut self, update: UpdateDMABUF);

    async fn mouse_set(&mut self, set: MouseSet);

    async fn cursor_define(&mut self, cursor: Cursor);

    fn disconnected(&mut self);
}

#[derive(Debug)]
pub(crate) struct ConsoleListener<H: ConsoleListenerHandler> {
    handler: H,
}

#[dbus_interface(name = "org.qemu.Display1.Listener")]
impl<H: ConsoleListenerHandler> ConsoleListener<H> {
    async fn scanout(
        &mut self,
        width: u32,
        height: u32,
        stride: u32,
        format: u32,
        data: serde_bytes::ByteBuf,
    ) {
        self.handler
            .scanout(Scanout {
                width,
                height,
                stride,
                format,
                data: data.into_vec(),
            })
            .await;
    }

    async fn update(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        stride: u32,
        format: u32,
        data: serde_bytes::ByteBuf,
    ) {
        self.handler
            .update(Update {
                x,
                y,
                w,
                h,
                stride,
                format,
                data: data.into_vec(),
            })
            .await;
    }

    #[dbus_interface(name = "ScanoutDMABUF")]
    async fn scanout_dmabuf(
        &mut self,
        fd: Fd,
        width: u32,
        height: u32,
        stride: u32,
        fourcc: u32,
        modifier: u64,
        y0_top: bool,
    ) {
        let fd = unsafe { libc::dup(fd.as_raw_fd()) };
        self.handler
            .scanout_dmabuf(ScanoutDMABUF {
                fd,
                width,
                height,
                stride,
                fourcc,
                modifier,
                y0_top,
            })
            .await;
    }

    #[dbus_interface(name = "UpdateDMABUF")]
    async fn update_dmabuf(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.handler
            .update_dmabuf(UpdateDMABUF { x, y, w, h })
            .await;
    }

    async fn mouse_set(&mut self, x: i32, y: i32, on: i32) {
        self.handler.mouse_set(MouseSet { x, y, on }).await;
    }

    async fn cursor_define(
        &mut self,
        width: i32,
        height: i32,
        hot_x: i32,
        hot_y: i32,
        data: Vec<u8>,
    ) {
        self.handler
            .cursor_define(Cursor {
                width,
                height,
                hot_x,
                hot_y,
                data,
            })
            .await;
    }
}

impl<H: ConsoleListenerHandler> ConsoleListener<H> {
    pub(crate) fn new(handler: H) -> Self {
        Self { handler }
    }
}

impl<H: ConsoleListenerHandler> Drop for ConsoleListener<H> {
    fn drop(&mut self) {
        self.handler.disconnected();
    }
}
