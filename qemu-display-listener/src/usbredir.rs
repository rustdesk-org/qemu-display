use crate::Chardev;

pub struct UsbRedir;

impl UsbRedir {
    pub fn new(chardevs: Vec<Chardev>) -> Self {
        dbg!(chardevs);
        Self
    }
}
