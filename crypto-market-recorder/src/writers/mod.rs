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

async fn create_nanomsg_reader_to_file_thread(
    venue: String,
    symbol: String,
    ipc_path: String
) {
    tokio::task::spawn(async move {
        let ipc_exchange_market_type_msg_type = format!(
            "ipc:///tmp/{}.ipc",
            ipc_path
        ); 

        let mut write_data = WriteData::new();
        write_data.start();

        let mut socket = Socket::new(Protocol::Sub).unwrap();
        let setopt = socket.subscribe(symbol.as_bytes());
        let mut endpoint = socket.connect(&ipc_exchange_market_type_msg_type).unwrap();
        match setopt {
            Ok(_) => debug!("Subscribed to '{}'.", symbol),
            Err(err) => error!("market_data failed to subscribe '{}'.", err),
        }
        loop {
            // 数据 payload
            let mut payload: Vec<u8> = Vec::new();
            let read_result = socket.read_to_end(&mut payload);
            match read_result {
                Ok(size) => {
                    let data = unsafe {std::mem::transmute::<Vec<u8>, Vec<i8>>(payload)};
                    write_data.add_order_book(venue.to_string(), symbol.to_string(), data);
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

pub fn create_writer_threads(
    exchange: &'static str,
    venue: &str,
    symbol: &str,
    ipc_path: &str
) -> Vec<BoxFuture<'static, ()>> {

    let mut threads = Vec::new();

    threads.push( create_nanomsg_reader_to_file_thread(
            venue.to_string(),
            symbol.to_string(),
            ipc_path.to_string()
        ) .boxed()
    );
    threads
}