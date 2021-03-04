use std::sync::mpsc::{Sender, SendError};

pub(crate) trait EventSender {
    type Event;

    fn send_event(&self, t: Self::Event) -> Result<(), SendError<Self::Event>>;
}

impl<T> EventSender for Sender<T> {
    type Event = T;

    fn send_event(&self, t: Self::Event) -> Result<(), SendError<Self::Event>> {
        self.send(t)
    }
}

#[cfg(feature = "glib")]
impl<T> EventSender for glib::Sender<T> {
    type Event = T;

    fn send_event(&self, t: Self::Event) -> Result<(), SendError<Self::Event>> {
        self.send(t)
    }
}
