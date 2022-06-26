extern crate log;

use std::net::{ToSocketAddrs, SocketAddr};

use ring::rand::*;

const MAX_DATAGRAM_SIZE: usize = 1350;

const HTTP_REQ_STREAM_ID: u64 = 4;

pub fn start_client(addr: SocketAddr, taket_list: Vec<&str>) {
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
    let mut socket =
        mio::net::UdpSocket::bind(bind_addr.parse().unwrap()).unwrap();
    poll.registry()
        .register(&mut socket, mio::Token(0), mio::Interest::READABLE)
        .unwrap();

    // Create the configuration for the QUIC connection.
    let mut config = quiche::Config::new(quiche::PROTOCOL_VERSION).unwrap();

    // *CAUTION*: this should not be set to `false` in production!!!
    config.verify_peer(false);

    config
        .set_application_protos(
            b"\x0ahq-interop\x05hq-29\x05hq-28\x05hq-27\x08http/0.9",
        )
        .unwrap();

    config.set_max_idle_timeout(5000);
    config.set_max_recv_udp_payload_size(MAX_DATAGRAM_SIZE);
    config.set_max_send_udp_payload_size(MAX_DATAGRAM_SIZE);
    config.set_initial_max_data(10_000_000);
    config.set_initial_max_stream_data_bidi_local(1_000_000);
    config.set_initial_max_stream_data_bidi_remote(1_000_000);
    config.set_initial_max_streams_bidi(100);
    config.set_initial_max_streams_uni(100);
    config.set_disable_active_migration(true);

    // Generate a random source connection ID for the connection.
    let mut scid = [0; quiche::MAX_CONN_ID_LEN];
    SystemRandom::new().fill(&mut scid[..]).unwrap();

    let scid = quiche::ConnectionId::from_ref(&scid);

    // Create a QUIC connection and initiate handshake.
    println!("{:?}", url);
    let mut conn =
        quiche::connect(url.domain(), &scid, peer_addr, &mut config).unwrap();
    println!("......");
    println!("{:?}", socket.local_addr());

    println!(
        "connecting to {:?} from {:?} with scid {}",
        peer_addr,
        socket.local_addr().unwrap(),
        hex_dump(&scid)
    );

    let (write, send_info) = conn.send(&mut out).expect("initial send failed");

    while let Err(e) = socket.send_to(&out[..write], send_info.to) {
        if e.kind() == std::io::ErrorKind::WouldBlock {
            println!("send() would block");
            continue;
        }

        println!("send() failed: {:?}", e);
    }

    println!("written {}", write);

    let req_start = std::time::Instant::now();

    let mut req_sent = false;


    loop {
        println!("{:?}", conn.timeout());
        poll.poll(&mut events, None).unwrap();

        println!("event ---> {:?}", events);

        // Read incoming UDP packets from the socket and feed them to quiche,
        // until there are no more packets to read.
        'read: loop {
            // If the event loop reported no events, it means that the timeout
            // has expired, so handle it without attempting to read packets. We
            // will then proceed with the send loop.
            if events.is_empty() {
                println!("not event");
                println!("timed out");
                conn.on_timeout();
                break 'read;
            }

            let (len, from) = match socket.recv_from(&mut buf) {
                Ok(v) => v,

                Err(e) => {
                    // There are no more UDP packets to read, so end the read
                    // loop.
                    if e.kind() == std::io::ErrorKind::WouldBlock {
                        println!("recv() would block");
                        break 'read;
                    }

                    panic!("recv() failed: {:?}", e);
                },
            };

            println!("got {} bytes", len);

            let recv_info = quiche::RecvInfo { from };

            // Process potentially coalesced packets.
            let read = match conn.recv(&mut buf[..len], recv_info) {
                Ok(v) => v,

                Err(e) => {
                    error!("recv failed: {:?}", e);
                    continue 'read;
                },
            };

            println!("processed {} bytes", read);
        }

        println!("done reading");

        if conn.is_closed() {
            println!("connection closed, {:?}", conn.stats());
            break;
        }

        println!("not connection closed");

        // Send an HTTP request as soon as the connection is established.
        if conn.is_established() && !req_sent{
            println!("sending take {:?}", taket_list);
            let mut data = Vec::new();
            for take in &taket_list {
                data.extend_from_slice(&format!("ADD {};", take).as_bytes());
            }

            println!("{:?}", String::from_utf8(data[..data.len()-1].to_vec()));

            conn.stream_send(HTTP_REQ_STREAM_ID, &data[..data.len()-1], true).unwrap();
            req_sent = true;
        }

        println!("established");

        // Process all readable streams.
        for s in conn.readable() {
            while let Ok((read, fin)) = conn.stream_recv(s, &mut buf) {
                println!("received {} bytes", read);

                let stream_buf = &buf[..read];

                debug!(
                    "stream {} has {} bytes (fin? {})",
                    s,
                    stream_buf.len(),
                    fin
                );

                


                
                // The server reported that it has no more data to send, which
                // we got the full response. Close the connection.
                if s == HTTP_REQ_STREAM_ID && fin {
                    println!(
                        "response received in {:?}, closing...",
                        req_start.elapsed()
                    );

                    conn.close(true, 0x00, b"kthxbye").unwrap();
                }
            }
        }

        println!("readable");

        // Generate outgoing QUIC packets and send them on the UDP socket, until
        // quiche reports that there are no more packets to be sent.
        loop {
            let (write, send_info) = match conn.send(&mut out) {
                Ok(v) => v,

                Err(quiche::Error::Done) => {
                    println!("done writing");
                    break;
                },

                Err(e) => {
                    error!("send failed: {:?}", e);

                    conn.close(false, 0x1, b"fail").ok();
                    break;
                },
            };

            if let Err(e) = socket.send_to(&out[..write], send_info.to) {
                if e.kind() == std::io::ErrorKind::WouldBlock {
                    println!("send() would block");
                    break;
                }

                panic!("send() failed: {:?}", e);
            }

            println!("written {}", write);
        }

        println!("loop end");

        if conn.is_closed() {
            println!("connection closed, {:?}", conn.stats());
            break;
        }
    }
}

fn hex_dump(buf: &[u8]) -> String {
    println!("hello ");
    let vec: Vec<String> = buf.iter().map(|b| format!("{:02x}", b)).collect();

    vec.join("")
}

