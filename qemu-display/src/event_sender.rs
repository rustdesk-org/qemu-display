use std::sync::mpsc::{SendError, Sender};
use std::sync::Mutex;

pub(crate) trait EventSender: Send + Sync {
    type Event;

    fn send_event(&self, t: Self::Event) -> Result<(), SendError<Self::Event>>;
}

impl<T: Send + Sync> EventSender for Mutex<Sender<T>> {
    type Event = T;

    fn send_event(&self, t: Self::Event) -> Result<(), SendError<Self::Event>> {
        self.lock().unwrap().send(t)
    }
}

#[cfg(feature = "glib")]
impl<T: Send + Sync> EventSender for glib::Sender<T> {
    type Event = T;

    fn send_event(&self, t: Self::Event) -> Result<(), SendError<Self::Event>> {
        self.send(t)
    }
}
