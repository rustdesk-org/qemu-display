use glib::subclass::prelude::*;
use glib::translate::*;
use gtk::prelude::*;
use gtk::subclass::widget::WidgetImplExt;
use gtk::{gdk, glib};
use std::cell::Cell;
use std::ffi::{CStr, CString};

use crate::egl;
use crate::error::*;
use gl::{self, types::*};
use qemu_display_listener::{Scanout, ScanoutDMABUF, Update};

mod imp {
    use super::*;
    use gtk::subclass::prelude::*;

    #[derive(Default)]
    pub struct QemuConsoleArea {
        pub tex_id: Cell<GLuint>,
        pub texture_blit_vao: Cell<GLuint>,
        pub texture_blit_prog: Cell<GLuint>,
        pub texture_blit_flip_prog: Cell<GLuint>,
        pub scanout: Cell<Option<ScanoutDMABUF>>,
        pub scanout_size: Cell<(u32, u32)>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for QemuConsoleArea {
        const NAME: &'static str = "QemuConsoleArea";
        type Type = super::QemuConsoleArea;
        type ParentType = gtk::GLArea;

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

        fn properties() -> &'static [glib::ParamSpec] {
            use once_cell::sync::Lazy;
            static PROPERTIES: Lazy<Vec<glib::ParamSpec>> = Lazy::new(|| {
                vec![glib::ParamSpec::boolean(
                    "resize-hack",
                    "resize-hack",
                    "Resize hack to notify parent",
                    false,
                    glib::ParamFlags::READWRITE | glib::ParamFlags::CONSTRUCT,
                )]
            });
            PROPERTIES.as_ref()
        }

        fn set_property(
            &self,
            _obj: &Self::Type,
            _id: usize,
            _value: &glib::Value,
            _pspec: &glib::ParamSpec,
        ) {
        }
    }

    impl WidgetImpl for QemuConsoleArea {
        fn realize(&self, widget: &Self::Type) {
            widget.set_has_depth_buffer(false);
            widget.set_has_stencil_buffer(false);
            widget.set_auto_render(false);
            widget.set_required_version(3, 2);
            self.parent_realize(widget);
            widget.make_current();

            if let Err(e) = unsafe { self.realize_gl() } {
                let e = glib::Error::new(AppError::GL, &e);
                widget.set_error(Some(&e));
            }
        }

        fn size_allocate(&self, widget: &Self::Type, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(widget, width, height, baseline);
            widget.notify("resize-hack");
        }
    }

    impl GLAreaImpl for QemuConsoleArea {
        fn render(&self, gl_area: &Self::Type, _context: &gdk::GLContext) -> bool {
            unsafe {
                gl::ClearColor(0.1, 0.1, 0.1, 1.0);
                gl::Clear(gl::COLOR_BUFFER_BIT);
                gl::Disable(gl::BLEND);

                let vp = self.viewport(gl_area);
                gl::Viewport(vp.x, vp.y, vp.width, vp.height);
                self.texture_blit(false);
            }
            // parent will return to update call
            false
        }
    }

    impl QemuConsoleArea {
        pub fn borders(&self, gl_area: &super::QemuConsoleArea) -> (u32, u32) {
            let sf = gl_area.get_scale_factor();
            let (w, h) = (gl_area.get_width() * sf, gl_area.get_height() * sf);
            let (gw, gh) = gl_area.scanout_size();
            let (sw, sh) = (w as f32 / gw as f32, h as f32 / gh as f32);

            if sw < sh {
                let bh = h - (h as f32 * sw / sh) as i32;
                (0, bh as u32 / 2)
            } else {
                let bw = w - (w as f32 * sh / sw) as i32;
                (bw as u32 / 2, 0)
            }
        }

        pub fn viewport(&self, gl_area: &super::QemuConsoleArea) -> gdk::Rectangle {
            let sf = gl_area.get_scale_factor();
            let (w, h) = (gl_area.get_width() * sf, gl_area.get_height() * sf);
            let (borderw, borderh) = self.borders(gl_area);
            let (borderw, borderh) = (borderw as i32, borderh as i32);
            gdk::Rectangle {
                x: borderw,
                y: borderh,
                width: w - borderw * 2,
                height: h - borderh * 2,
            }
        }

        unsafe fn realize_gl(&self) -> Result<(), String> {
            let texture_blit_vs = CString::new(include_str!("texture-blit.vert")).unwrap();
            let texture_blit_flip_vs =
                CString::new(include_str!("texture-blit-flip.vert")).unwrap();
            let texture_blit_fs = CString::new(include_str!("texture-blit.frag")).unwrap();

            let texture_blit_prg =
                compile_prog(texture_blit_vs.as_c_str(), texture_blit_fs.as_c_str())?;
            self.texture_blit_prog.set(texture_blit_prg);
            let texture_blit_flip_prg =
                compile_prog(texture_blit_flip_vs.as_c_str(), texture_blit_fs.as_c_str())?;
            self.texture_blit_flip_prog.set(texture_blit_flip_prg);

            let mut vao = 0;
            gl::GenVertexArrays(1, &mut vao);
            gl::BindVertexArray(vao);
            let mut vb = 0;
            gl::GenBuffers(1, &mut vb);
            gl::BindBuffer(gl::ARRAY_BUFFER, vb);
            static POS: [f32; 8] = [-1.0, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0];
            gl::BufferData(
                gl::ARRAY_BUFFER,
                std::mem::size_of::<[f32; 8]>() as _,
                POS.as_ptr() as _,
                gl::STATIC_DRAW,
            );
            let in_pos = gl::GetAttribLocation(
                texture_blit_prg,
                CString::new("in_position").unwrap().as_c_str().as_ptr(),
            ) as u32;
            gl::VertexAttribPointer(in_pos, 2, gl::FLOAT, gl::FALSE, 0, std::ptr::null());
            gl::EnableVertexAttribArray(in_pos);
            gl::BindBuffer(gl::ARRAY_BUFFER, 0);
            gl::BindVertexArray(0);
            self.texture_blit_vao.set(vao);

            let tex_unit = gl::GetUniformLocation(
                texture_blit_prg,
                CString::new("tex_unit").unwrap().as_c_str().as_ptr(),
            );
            gl::ProgramUniform1i(texture_blit_prg, tex_unit, 0);

            let mut tex_id = 0;
            gl::GenTextures(1, &mut tex_id);
            self.tex_id.set(tex_id);

            Ok(())
        }

        unsafe fn texture_blit(&self, flip: bool) {
            gl::UseProgram(if flip {
                todo!();
                //self.texture_blit_flip_prog.get()
            } else {
                self.texture_blit_prog.get()
            });
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, self.tex_id());
            gl::BindVertexArray(self.texture_blit_vao.get());
            gl::DrawArrays(gl::TRIANGLE_STRIP, 0, 4);
        }

        pub fn tex_id(&self) -> GLuint {
            self.tex_id.get()
        }

        pub fn save_to_png(&self, widget: &super::QemuConsoleArea, filename: &str) {
            let (gw, gh) = self.scanout_size.get();
            let ctxt = widget.get_context().unwrap();
            let tex = unsafe { gdk::GLTexture::new(&ctxt, self.tex_id(), gw as _, gh as _) };
            tex.save_to_png(filename);
        }

        pub fn set_scanout(&self, widget: &super::QemuConsoleArea, s: Scanout) {
            widget.make_current();

            if s.format != 0x20020888 {
                todo!();
            }
            unsafe {
                gl::BindTexture(gl::TEXTURE_2D, self.tex_id());
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as _);
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as _);
                gl::PixelStorei(gl::UNPACK_ROW_LENGTH, s.stride as i32 / 4);
                gl::TexImage2D(
                    gl::TEXTURE_2D,
                    0,
                    gl::RGB as _,
                    s.width as _,
                    s.height as _,
                    0,
                    gl::BGRA,
                    gl::UNSIGNED_BYTE,
                    s.data.as_ptr() as _,
                );
            }

            self.scanout_size.set((s.width, s.height));
        }

        pub fn update(&self, widget: &super::QemuConsoleArea, u: Update) {
            widget.make_current();

            if u.format != 0x20020888 {
                todo!();
            }
            unsafe {
                gl::BindTexture(gl::TEXTURE_2D, self.tex_id());
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as _);
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as _);
                gl::PixelStorei(gl::UNPACK_ROW_LENGTH, u.stride as i32 / 4);
                gl::TexSubImage2D(
                    gl::TEXTURE_2D,
                    0,
                    u.x,
                    u.y,
                    u.w,
                    u.h,
                    gl::BGRA,
                    gl::UNSIGNED_BYTE,
                    u.data.as_ptr() as _,
                );
            }
        }

        pub fn set_scanout_dmabuf(&self, widget: &super::QemuConsoleArea, s: ScanoutDMABUF) {
            widget.make_current();
            let egl = egl::egl();

            let egl_dpy = if let Ok(dpy) = widget.get_display().downcast::<gdk_wl::WaylandDisplay>()
            {
                let wl_dpy = dpy.get_wl_display();
                egl.get_display(wl_dpy.as_ref().c_ptr() as _)
                    .expect("Failed to get EGL display")
            } else if let Ok(dpy) = widget.get_display().downcast::<gdk_x11::X11Display>() {
                let _dpy =
                    unsafe { gdk_x11::ffi::gdk_x11_display_get_xdisplay(dpy.to_glib_none().0) };
                eprintln!("X11: unsupported display kind, todo: EGL");
                return;
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

            unsafe {
                gl::BindTexture(gl::TEXTURE_2D, self.tex_id());
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as _);
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as _);
            }
            if let Some(image_target) = egl::image_target_texture_2d_oes() {
                image_target(gl::TEXTURE_2D, img.as_ptr() as gl::types::GLeglImageOES);
            } else {
                eprintln!("Failed to set texture image");
            }

            self.scanout_size.set((s.width, s.height));
            self.scanout.set(Some(s));

            if let Err(e) = egl.destroy_image(egl_dpy, img) {
                eprintln!("Destroy image failed: {}", e);
            }
        }
    }
}

glib::wrapper! {
    pub struct QemuConsoleArea(ObjectSubclass<imp::QemuConsoleArea>)
        @extends gtk::Widget, gtk::GLArea;
}

impl QemuConsoleArea {
    pub fn scanout_size(&self) -> (u32, u32) {
        let priv_ = imp::QemuConsoleArea::from_instance(self);

        priv_.scanout_size.get()
    }

    pub fn set_scanout(&self, s: Scanout) {
        let priv_ = imp::QemuConsoleArea::from_instance(self);

        priv_.set_scanout(self, s);
    }

    pub fn update(&self, u: Update) {
        let priv_ = imp::QemuConsoleArea::from_instance(self);

        priv_.update(self, u);
    }

    pub fn set_scanout_dmabuf(&self, s: ScanoutDMABUF) {
        let priv_ = imp::QemuConsoleArea::from_instance(self);

        priv_.set_scanout_dmabuf(self, s);
    }

    pub fn save_to_png(&self, filename: &str) {
        let priv_ = imp::QemuConsoleArea::from_instance(self);

        priv_.save_to_png(self, filename);
    }

    pub fn transform_input(&self, x: f64, y: f64) -> Option<(u32, u32)> {
        let priv_ = imp::QemuConsoleArea::from_instance(self);

        let vp = priv_.viewport(self);
        let x = x as i32 * self.get_scale_factor();
        let y = y as i32 * self.get_scale_factor();
        if !vp.contains_point(x, y) {
            return None;
        }
        let (sw, sh) = priv_.scanout_size.get();
        let x = (x - vp.x) as f64 * (sw as f64 / vp.width as f64);
        let y = (y - vp.y) as f64 * (sh as f64 / vp.height as f64);
        Some((x as u32, y as u32))
    }
}

unsafe fn compile_shader(type_: GLenum, src: &CStr) -> GLuint {
    let shader = gl::CreateShader(type_);
    gl::ShaderSource(shader, 1, &src.as_ptr(), std::ptr::null());
    gl::CompileShader(shader);
    shader
}

fn cstring_new_len(len: usize) -> CString {
    let buffer: Vec<u8> = Vec::with_capacity(len + 1);
    unsafe { CString::from_vec_unchecked(buffer) }
}

unsafe fn compile_prog(vs: &CStr, fs: &CStr) -> Result<GLuint, String> {
    let vs = compile_shader(gl::VERTEX_SHADER, vs);
    let fs = compile_shader(gl::FRAGMENT_SHADER, fs);
    let prog = gl::CreateProgram();

    gl::AttachShader(prog, vs);
    gl::AttachShader(prog, fs);
    gl::LinkProgram(prog);

    let mut status: i32 = 0;
    gl::GetProgramiv(prog, gl::LINK_STATUS, &mut status);
    if status == 0 {
        let mut len: GLint = 0;
        gl::GetProgramiv(prog, gl::INFO_LOG_LENGTH, &mut len);
        let error = cstring_new_len(len as usize);
        gl::GetProgramInfoLog(
            prog,
            len,
            std::ptr::null_mut(),
            error.as_ptr() as *mut gl::types::GLchar,
        );
        return Err(error.to_string_lossy().into_owned());
    }
    gl::DeleteShader(vs);
    gl::DeleteShader(fs);
    Ok(prog)
}
