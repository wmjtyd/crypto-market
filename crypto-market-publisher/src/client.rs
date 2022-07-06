extern crate log;

use std::{
    net::{SocketAddr, ToSocketAddrs},
    sync::{mpsc::{channel, Receiver, Sender}, Arc},
};

use quiche::Config;
use ring::rand::*;

use crate::msg_type::Code;

// const MAX_DATAGRAM_SIZE: usize = 1350;

const HTTP_REQ_STREAM_ID: u64 = 4;

pub fn client<T>(
    msg: &'static dyn Code<T>,
    addr: SocketAddr,
    mut config: Config, 
    subscribe_list: Vec<String>, 
    tx: Sender<T>
) {
    let mut buf = [0; 65535];
    let mut out = [0; 1350];

    let url_ptah = format!("http://{}", addr);
    let url = url::Url::parse(&url_ptah).unwrap();

    // Setup the event loop.
    let mut poll = mio::Poll::new().unwrap();
    let mut events = mio::Events::with_capacity(1024);

    // Resolve server address.
    let peer_addr = url.to_socket_addrs().unwrap().next().unwrap();

    // Bind to INADDR_ANY or IN6ADDR_ANY depending on the IP family of the
    // server address. This is needed on macOS and BSD variants that don't
    // support binding to IN6ADDR_ANY for both v4 and v6.
    let bind_addr = match peer_addr {
        std::net::SocketAddr::V4(_) => "0.0.0.0:0",
        std::net::SocketAddr::V6(_) => "[::]:0",
    };

    // Create the UDP socket backing the QUIC connection, and register it with
    // the event loop.
    let mut socket = mio::net::UdpSocket::bind(bind_addr.parse().unwrap()).unwrap();
    poll.registry()
        .register(&mut socket, mio::Token(0), mio::Interest::READABLE)
        .unwrap();

    // Generate a random source connection ID for the connection.
    let mut scid = [0; quiche::MAX_CONN_ID_LEN];
    SystemRandom::new().fill(&mut scid[..]).unwrap();

    let scid = quiche::ConnectionId::from_ref(&scid);

    // Create a QUIC connection and initiate handshake.
    debug!("{:?}", url);
    let mut conn = quiche::connect(url.domain(), &scid, peer_addr, &mut config).unwrap();
    debug!("......");
    debug!("{:?}", socket.local_addr());

    debug!(
        "connecting to {:?} from {:?} with scid {}",
        peer_addr,
        socket.local_addr().unwrap(),
        buf.iter()
            .map(|b| format!("{:02x}", b))
            .collect::<Vec<String>>()
            .join("")
            .len()
    );

    let (write, send_info) = conn.send(&mut out).expect("initial send failed");

    while let Err(e) = socket.send_to(&out[..write], send_info.to) {
        if e.kind() == std::io::ErrorKind::WouldBlock {
            debug!("send() would block");
            continue;
        }

        warn!("send() failed: {:?}", e);
    }

    debug!("written {}", write);

    let req_start = std::time::Instant::now();

    let mut req_sent = false;

    loop {
        debug!("{:?}", conn.timeout());
        poll.poll(&mut events, None).unwrap();

        debug!("event ---> {:?}", events);

        // Read incoming UDP packets from the socket and feed them to quiche,
        // until there are no more packets to read.
        'read: loop {
            // If the event loop reported no events, it means that the timeout
            // has expired, so handle it without attempting to read packets. We
            // will then proceed with the send loop.
            if events.is_empty() {
                debug!("not event");
                debug!("timed out");
                conn.on_timeout();
                break 'read;
            }

            let (len, from) = match socket.recv_from(&mut buf) {
                Ok(v) => v,

                Err(e) => {
                    // There are no more UDP packets to read, so end the read
                    // loop.
                    if e.kind() == std::io::ErrorKind::WouldBlock {
                        warn!("recv() would block");
                        break 'read;
                    }

                    panic!("recv() failed: {:?}", e);
                }
            };

            debug!("got {} bytes", len);

            let recv_info = quiche::RecvInfo { from };

            // Process potentially coalesced packets.
            let read = match conn.recv(&mut buf[..len], recv_info) {
                Ok(v) => v,

                Err(e) => {
                    debug!("recv failed: {:?}", e);
                    continue 'read;
                }
            };

            debug!("processed {} bytes", read);
        }

        debug!("done reading");

        if conn.is_closed() {
            error!("connection closed, {:?}", conn.stats());
            break;
        }

        debug!("not connection closed");

        // Send an HTTP request as soon as the connection is established.
        if conn.is_established() && !req_sent {
            debug!("sending sub {:?}", subscribe_list);
            let mut data = Vec::new();
            for sub in &subscribe_list {
                data.extend_from_slice(&format!("sub@{};", sub).as_bytes());
            }

            debug!("{:?}", String::from_utf8(data[..data.len() - 1].to_vec()));

            conn.stream_send(HTTP_REQ_STREAM_ID, &data[..data.len() - 1], true)
                .unwrap();
            req_sent = true;
        }

        debug!("established");

        // Process all readable streams.
        for s in conn.readable() {
            while let Ok((read, fin)) = conn.stream_recv(s, &mut buf) {
                debug!("received {} bytes", read);

                let stream_buf = &buf[..read];

                debug!("stream {} has {} bytes (fin? {})", s, stream_buf.len(), fin);


                if tx.send(msg.decode(stream_buf)).is_err() {
                    return;
                };

                // The server reported that it has no more data to send, which
                // we got the full response. Close the connection.
                if s == HTTP_REQ_STREAM_ID && fin {
                    debug!("response received in {:?}, closing...", req_start.elapsed());

                    conn.close(true, 0x00, b"kthxbye").unwrap();
                }
            }
        }

        debug!("readable");

        // Generate outgoing QUIC packets and send them on the UDP socket, until
        // quiche reports that there are no more packets to be sent.
        loop {
            let (write, send_info) = match conn.send(&mut out) {
                Ok(v) => v,

                Err(quiche::Error::Done) => {
                    debug!("done writing");
                    break;
                }

                Err(e) => {
                    debug!("send failed: {:?}", e);

                    conn.close(false, 0x1, b"fail").ok();
                    break;
                }
            };

            if let Err(e) = socket.send_to(&out[..write], send_info.to) {
                if e.kind() == std::io::ErrorKind::WouldBlock {
                    debug!("send() would block");
                    break;
                }

                panic!("send() failed: {:?}", e);
            }

            debug!("written {}", write);
        }

        if conn.is_closed() {
            error!("connection closed, {:?}", conn.stats());
            break;
        }
    }
}

macro_rules! start_client {
    ($msg_type: ident, $addr:ident, $config:ident, $subscribe:ident) => {{

        let (tx, rx) = std::sync::mpsc::channel::<$msg_type>();
        let subscribe_list: Vec<String> = $subscribe.into_iter().map(|e| e.to_string()).collect();

        tokio::task::spawn(async move {
            client::client(&msg_type::Msg::<$msg_type>(None), $addr, $config, subscribe_list, tx);
        });
        rx
    }}
}