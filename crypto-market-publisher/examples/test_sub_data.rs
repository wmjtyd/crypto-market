use std::io::Write;
use std::thread;
use std::time::Duration;

use wmjtyd_libstock::message::traits::Bind;
use wmjtyd_libstock::message::zeromq::ZeromqPublisher;

#[tokio::main]
async fn main() {
    const PATH: &str = "ipc:///tmp/test_test_test_test.ipc";

    let mut publisher = ZeromqPublisher::new().expect("init error");
    publisher.bind(PATH).expect("ipc bind error");

    let mut package_num = 0;
    loop {
        publisher
            .write_all(format!("{package_num}").as_bytes())
            .expect("write_all error");
        thread::sleep(Duration::from_millis(400));
        package_num += 1;
    }
}
