extern crate net2;

use net2::UdpBuilder;
use std::net::Ipv4Addr;
use std::str::FromStr;
use tokio::io::AsyncReadExt;
use wmjtyd_libstock::message::zeromq::{Sub, Zeromq};

pub async fn server(ip: &str, port: &str, ipcs: Vec<&str>) {
    // bind ip addr
    let socket = UdpBuilder::new_v4()
        .expect("cannot create UDP socket")
        .reuse_address(true)
        .expect("cannot set reuse address")
        .bind((Ipv4Addr::from([0, 0, 0, 0]), 0))
        .expect("cannot bind");

    let addr = format!("{}:{}", ip, port);

    let mut thread = Vec::new();

    let (tx, rx) = std::sync::mpsc::channel();

    for ipc in ipcs {
        let ipc = ipc.to_owned();
        let tx = tx.clone();
        thread.push(tokio::task::spawn(async move {
            //  message queue
            let mut mq = Zeromq::<Sub>::new(&ipc)
                .await
                .expect(format!("sub error; ipc: {}", ipc).as_str());
            mq.subscribe("").await.expect("sub error;");

            let mut data = [0u8; 8192];

            // Message pairs are used to obtain data and forward it to multicast
            while let Ok(data_size) = mq.read(&mut data).await {
                if 0 == data_size {
                    break;
                }
                if tx.send(data[..data_size].to_vec()).is_err() {
                    break;
                }
            }
        }));
    }

    while let Ok(data) = rx.recv() {
        let size = socket
            .send_to(&data, &addr)
            .expect("cannot send");
    }


    println!("end....");
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
