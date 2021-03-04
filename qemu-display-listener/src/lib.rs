#![allow(clippy::too_many_arguments)]

mod error;
pub use error::*;

mod event_sender;
use event_sender::*;

mod vm;
pub use vm::*;

// mod audio;
// pub use audio::*;

mod console;
pub use console::*;

mod console_listener;
pub use console_listener::*;

mod keyboard;
pub use keyboard::*;

mod mouse;
pub use mouse::*;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
