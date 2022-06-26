pub(super) mod file_writer;
pub(super) mod file_save;

use futures::{future::BoxFuture, FutureExt};
use log::*;
use nanomsg::{Protocol, Socket};
use std::io::Read;

pub use file_writer::FileWriter;
pub use crate::data::decode_orderbook;
pub use crate::writers::file_save::WriteData;

pub trait Writer {
    fn write(&mut self, s: &str);
    fn close(&mut self);
}

pub async fn create_write_file_thread(
    _exchange: String,
    _market_type: String,
    _msg_type: String,
    ipc: String
) {
    tokio::task::spawn(async move {
        let ipc_exchange_market_type_msg_type = format!(
            "ipc:///tmp/{}.ipc",
            &ipc
        ); 

        let mut write_data = WriteData::new();
        write_data.start();

        let mut socket = Socket::new(Protocol::Sub).unwrap();
        socket.subscribe("".as_bytes()).unwrap();
        let mut endpoint = socket.connect(&ipc_exchange_market_type_msg_type).unwrap();
        loop {
            // 数据 payload
            let mut payload: Vec<u8> = Vec::new();
            let read_result = socket.read_to_end(&mut payload);
            match read_result {
                Ok(_size) => {
                    let data = unsafe {std::mem::transmute::<Vec<u8>, Vec<i8>>(payload)};
                    write_data.add_order_book(ipc.to_string(), data);
                }
                Err(err) => {
                    error!("Client failed to receive payload '{}'.", err);
                    break;
                }
            }
        }
        endpoint.shutdown();
    })
    .await
    .expect("msg");
}
