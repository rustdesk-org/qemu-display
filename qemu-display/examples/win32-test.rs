use std::env::args;
use std::error::Error;
use std::thread::sleep;
use std::time::Duration;

use qemu_display::Display;

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt::init();

    let qmp_path = args().nth(1).expect("argument: QMP socket path");
    let display = Display::new_qmp(qmp_path).await?;

    loop {
        sleep(Duration::from_secs(1));
    }
}
