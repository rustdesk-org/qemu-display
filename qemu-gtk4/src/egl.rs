pub use khronos_egl::*;
use once_cell::sync::OnceCell;

type EglInstance = Instance<khronos_egl::Dynamic<libloading::Library, khronos_egl::EGL1_5>>;

pub(crate) fn egl() -> &'static EglInstance {
    static INSTANCE: OnceCell<EglInstance> = OnceCell::new();
    INSTANCE.get_or_init(|| {
        let lib = libloading::Library::new("libEGL.so").expect("unable to find libEGL.so");
        unsafe {
            khronos_egl::DynamicInstance::<khronos_egl::EGL1_5>::load_required_from(lib)
                .expect("unable to load libEGL.so")
        }
    })
}

pub const LINUX_DMA_BUF_EXT: Enum = 0x3270;
pub const LINUX_DRM_FOURCC_EXT: Int = 0x3271;
pub const DMA_BUF_PLANE0_FD_EXT: Int = 0x3272;
pub const DMA_BUF_PLANE0_OFFSET_EXT: Int = 0x3273;
pub const DMA_BUF_PLANE0_PITCH_EXT: Int = 0x3274;
pub const DMA_BUF_PLANE0_MODIFIER_LO_EXT: Int = 0x3443;
pub const DMA_BUF_PLANE0_MODIFIER_HI_EXT: Int = 0x3444;

// GLAPI void APIENTRY glEGLImageTargetTexture2DOES (GLenum target, GLeglImageOES image);

pub type ImageTargetTexture2DOesFn = extern "C" fn(gl::types::GLenum, gl::types::GLeglImageOES);

pub fn image_target_texture_2d_oes() -> Option<ImageTargetTexture2DOesFn> {
    unsafe {
        egl()
            .get_proc_address("glEGLImageTargetTexture2DOES")
            .map(|f| std::mem::transmute::<_, ImageTargetTexture2DOesFn>(f))
    }
}
