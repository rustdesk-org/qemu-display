use gdk_wl::WaylandDisplayManualExt;
use glib::subclass::prelude::*;
use gtk::prelude::*;
use gtk::{gdk, glib, graphene};
use std::cell::{Cell, RefCell};

use crate::egl;
use gl::{self, types::*};
use qemu_display_listener::Scanout;

mod imp {
    use super::*;
    use glib::subclass;
    use gtk::subclass::prelude::*;

    pub struct QemuConsoleArea {
        pub scanout: Cell<Option<Scanout>>,
        pub scanout_size: Cell<(u32, u32)>,
        pub tex_id: Cell<GLuint>,
        pub texture: RefCell<Option<gdk::Texture>>,
    }

    impl ObjectSubclass for QemuConsoleArea {
        const NAME: &'static str = "QemuConsoleArea";
        type Type = super::QemuConsoleArea;
        type ParentType = gtk::Widget;
        type Interfaces = ();
        type Instance = subclass::simple::InstanceStruct<Self>;
        type Class = subclass::simple::ClassStruct<Self>;

        glib::object_subclass!();

        fn new() -> Self {
            Self {
                scanout: Cell::new(None),
                scanout_size: Cell::new((0, 0)),
                tex_id: Cell::new(0),
                texture: RefCell::new(None),
            }
        }

        fn class_init(_klass: &mut Self::Class) {
            // GL loading could be done earlier?
            let egl = egl::egl();

            gl::load_with(|s| {
                egl.get_proc_address(s)
                    .map(|f| f as _)
                    .unwrap_or(std::ptr::null())
            });
        }
    }

    impl ObjectImpl for QemuConsoleArea {
        fn constructed(&self, obj: &Self::Type) {
            self.parent_constructed(obj);
        }
    }

    impl WidgetImpl for QemuConsoleArea {
        fn snapshot(&self, widget: &Self::Type, snapshot: &gtk::Snapshot) {
            let (width, height) = (widget.get_width() as f32, widget.get_height() as f32);
            let whole = &graphene::Rect::new(0_f32, 0_f32, width, height);
            // TODO: make this a CSS style?
            //snapshot.append_color(&gdk::RGBA::black(), whole);
            if let Some(texture) = &*self.texture.borrow() {
                snapshot.append_texture(texture, whole);
            }
        }
    }
}

glib::wrapper! {
    pub struct QemuConsoleArea(ObjectSubclass<imp::QemuConsoleArea>) @extends gtk::Widget;
}

impl QemuConsoleArea {
    pub fn tex_id(&self) -> GLuint {
        let priv_ = imp::QemuConsoleArea::from_instance(self);
        let mut tex_id = priv_.tex_id.get();
        if tex_id == 0 {
            unsafe { gl::GenTextures(1, &mut tex_id) }
            priv_.tex_id.set(tex_id);
        }
        tex_id
    }

    fn update_texture(&self, s: &Scanout) {
        let priv_ = imp::QemuConsoleArea::from_instance(self);
        let ctxt = gdk::GLContext::get_current().unwrap();
        let tex =
            unsafe { gdk::GLTexture::new(&ctxt, self.tex_id(), s.width as i32, s.height as i32) };

        //tex.save_to_png("/tmp/tex.png");
        //tex.clone().downcast::<gdk::GLTexture>().unwrap().release();
        tex.release();
        *priv_.texture.borrow_mut() = Some(tex.upcast());
    }

    pub fn scanout_size(&self) -> (u32, u32) {
        let priv_ = imp::QemuConsoleArea::from_instance(self);

        priv_.scanout_size.get()
    }

    pub fn set_scanout(&self, s: Scanout) {
        let priv_ = imp::QemuConsoleArea::from_instance(self);
        let egl = egl::egl();

        let egl_dpy = if let Ok(dpy) = self.get_display().downcast::<gdk_wl::WaylandDisplay>() {
            let wl_dpy = dpy.get_wl_display();
            egl.get_display(wl_dpy.as_ref().c_ptr() as _)
                .expect("Failed to get EGL display")
        } else {
            eprintln!("Unsupported display kind");
            return;
        };

        let attribs = vec![
            egl::WIDTH as usize,
            s.width as usize,
            egl::HEIGHT as usize,
            s.height as usize,
            egl::LINUX_DRM_FOURCC_EXT as usize,
            s.fourcc as usize,
            egl::DMA_BUF_PLANE0_FD_EXT as usize,
            s.fd as usize,
            egl::DMA_BUF_PLANE0_PITCH_EXT as usize,
            s.stride as usize,
            egl::DMA_BUF_PLANE0_OFFSET_EXT as usize,
            0,
            egl::DMA_BUF_PLANE0_MODIFIER_LO_EXT as usize,
            (s.modifier & 0xffffffff) as usize,
            egl::DMA_BUF_PLANE0_MODIFIER_HI_EXT as usize,
            (s.modifier >> 32 & 0xffffffff) as usize,
            egl::NONE as usize,
        ];

        let img = egl
            .create_image(
                egl_dpy,
                unsafe { egl::Context::from_ptr(egl::NO_CONTEXT) },
                egl::LINUX_DMA_BUF_EXT,
                unsafe { egl::ClientBuffer::from_ptr(std::ptr::null_mut()) },
                &attribs,
            )
            .expect("Failed to eglCreateImage");

        let tex_id = self.tex_id();
        unsafe { gl::BindTexture(gl::TEXTURE_2D, tex_id) }
        if let Some(image_target) = egl::image_target_texture_2d_oes() {
            image_target(gl::TEXTURE_2D, img.as_ptr() as gl::types::GLeglImageOES);
        } else {
            eprintln!("Failed to set texture image");
        }

        self.update_texture(&s);
        self.queue_draw();

        if let Err(e) = egl.destroy_image(egl_dpy, img) {
            eprintln!("Destroy image failed: {}", e);
        }

        priv_.scanout_size.set((s.width, s.height));
        priv_.scanout.set(Some(s));
    }
}
