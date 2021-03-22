use gtk::glib;

#[derive(Clone, Copy, Debug, PartialEq, Eq, glib::GErrorDomain)]
#[gerror_domain(name = "QemuGtk")]
pub enum QemuGtkError {
    GL,
    Failed,
}
