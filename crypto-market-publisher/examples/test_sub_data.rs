use std::{thread, time::Duration};
use wmjtyd_libstock::message::zeromq::Zeromq;
use tokio::io::AsyncWriteExt;
use wmjtyd_libstock::message::zeromq::Pub;


#[tokio::main]
async fn main() {
    const PATH: &str = "ipc:///tmp/test_test_test_test.ipc";
    let mut pub_ = Zeromq::<Pub>::new(PATH).await.unwrap();
    let mut package_num = 0;
    loop {
        pub_.write_all(format!("{package_num}").as_bytes()).await.unwrap();
        thread::sleep(Duration::from_millis(400));
        package_num += 1;
    }
}
