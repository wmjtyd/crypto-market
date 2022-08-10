extern crate net2;

use std::net::Ipv4Addr;
use std::str::FromStr;

use crossbeam_channel::Sender;
use net2::UdpBuilder;
use tokio::runtime::Handle;
use wmjtyd_libstock::message::traits::{Connect, StreamExt, Subscribe};
use wmjtyd_libstock::message::zeromq::ZeromqSubscriber;

mod file;
use crossbeam_channel::unbounded as channel;
use file::{DynamicConfigHandler, IpcUrl};

use crate::file::create_watcher;

async fn receive_data(url: IpcUrl, sender: &Sender<Vec<u8>>) {
    tracing::info!("handle ipc {}", url);
    if let Ok(mut subscriber) = ZeromqSubscriber::new() {
        if subscriber.connect(&url).is_err() || subscriber.subscribe(b"").is_err() {
            tracing::error!("connect error");
        }

        loop {
            let message = StreamExt::next(&mut subscriber).await;
            if message.is_none() {
                break;
            }
            match message.unwrap() {
                Ok(message) => {
                    if sender.send(message).is_err() {
                        tracing::error!("tx stop!");
                        break;
                    };
                }
                Err(msg) => {
                    println!("data error");
                    println!("{:?}", msg);
                    break;
                }
            }
        }

        tracing::info!("send stop; url -> {}", url);
    }
}

pub fn server(ip: &str, port: &str, config_path: &str) {
    // bind ip addr
    let socket = UdpBuilder::new_v4()
        .expect("cannot create UDP socket")
        .reuse_address(true)
        .expect("cannot set reuse address")
        .bind((Ipv4Addr::from([0, 0, 0, 0]), 0))
        .expect("cannot bind");

    let addr = format!("{}:{}", ip, port);

    let handle = Handle::current();

    let (tx, rx) = channel::<Vec<u8>>();

    let mut handler = DynamicConfigHandler::new(handle, {
        Box::new(move |handle, _typ, url| {
            let tx = tx.clone();
            handle.spawn(async move {
                receive_data(url, &tx).await;
            })
        })
    });

    if handler.initiate(config_path).is_err() {
        tracing::error!("config format; path -> {}", config_path);
        panic!("server stop");
    }

    let watcher = create_watcher(config_path, handler);

    if watcher.is_err() {
        tracing::error!("create watcher; path -> {}", config_path);
        panic!("server stop");
    }

    tracing::info!("server status [start]");

    for data in rx {
        let size = socket.send_to(&data, &addr);

        if let Ok(size) = size {
            tracing::debug!("send {}", size);
        } else {
            tracing::error!("multicast send error");
            break;
        }
    }

    tracing::info!("server status [stop]");
}

pub fn client(ip: &str, port: &str) -> std::sync::mpsc::Receiver<Vec<u8>> {
    let mc_ip = Ipv4Addr::from_str(ip).expect("cannot parse group_ip");
    let mc_port = port.parse::<u16>().expect("cannot parse port");
    let any = Ipv4Addr::from([0, 0, 0, 0]);

    // bind ip addr
    let socket = UdpBuilder::new_v4()
        .expect("cannot create UDP socket")
        .reuse_address(true)
        .expect("cannot set reuse address")
        .bind((any, mc_port))
        .expect("cannot bind");

    socket.join_multicast_v4(&mc_ip, &any).expect("cannot join");

    let (tx, rx) = std::sync::mpsc::channel();

    tokio::task::spawn(async move {
        let mut buffer = [0u8; 1600];
        loop {
            // Listen for data from the server
            let (size, _addr) = socket.recv_from(&mut buffer).expect("cannot recv");
            if tx.send(buffer[..size].to_vec()).is_err() {
                break;
            }
        }
    });

    rx
}
