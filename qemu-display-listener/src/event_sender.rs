use std::sync::mpsc::{SendError, Sender};

pub(crate) trait EventSender: Send {
    type Event;

    fn send_event(&self, t: Self::Event) -> Result<(), SendError<Self::Event>>;
}

impl<T: Send> EventSender for Sender<T> {
    type Event = T;

    fn send_event(&self, t: Self::Event) -> Result<(), SendError<Self::Event>> {
        self.send(t)
    }
}

#[cfg(feature = "glib")]
impl<T: Send> EventSender for glib::Sender<T> {
    type Event = T;

    fn send_event(&self, t: Self::Event) -> Result<(), SendError<Self::Event>> {
        self.send(t)
    }
}
