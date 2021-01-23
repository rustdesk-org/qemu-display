#![allow(clippy::too_many_arguments)]

mod error;
pub use error::*;

mod vm;
pub use vm::*;

mod console;
pub use console::*;

mod listener;
pub use listener::*;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
