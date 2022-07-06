use std::{
    collections::{HashMap, HashSet},
    io::Read,
    net::{self, SocketAddr},
    sync::{Arc, Mutex},
    thread,
};

use mio::{net::UdpSocket};
use mio::Poll;
use nanomsg::{Protocol, Socket};
use quiche::Config;
use ring::rand::SystemRandom;

const MAX_DATAGRAM_SIZE: usize = 1350;

extern crate lazy_static;

extern crate log;

type ClientList = HashMap<quiche::ConnectionId<'static>, Client>;
type ClientSubscribe = HashMap<String, HashSet<quiche::ConnectionId<'static>>>;

lazy_static! {
    static ref CLIENT_LIST: Mutex<ClientList> = {
        let map: ClientList = HashMap::new();
        Mutex::new(map)
    };
    static ref CLIENT_SUBSCRIBE: Mutex<ClientSubscribe> = {
        let map: ClientSubscribe = HashMap::new();
        Mutex::new(map)
    };
}

struct PartialResponse {
    body: Vec<u8>,
    written: usize,
}

struct Client {
    conn: quiche::Connection,
    partial_responses: HashMap<u64, PartialResponse>,
}

pub fn create_server(addr: SocketAddr, ipc: String, config: Config) {
    let mut service = Vec::new();

    debug!("{:?}", addr);
    let mut socket = mio::net::UdpSocket::bind(addr).unwrap();
    let poll = mio::Poll::new().unwrap();
    poll.registry()
        .register(&mut socket, mio::Token(0), mio::Interest::READABLE)
        .unwrap();

    let socket = Arc::new(socket);

    let ipcs= vec![ipc];

    // 套字节
    let socket_clone = socket.clone();

    // 连接初始
    service.push(thread::spawn(move || {
        server_connection(poll, socket_clone, config);
    }));

    // 存活检查
    service.push(thread::spawn(|| {
        server_alive_check();
    }));

    let socket_clone = socket.clone();
    // 发送
    service.push(thread::spawn(|| {
        server_send(ipcs, socket_clone);
    }));

    for s in service {
        s.join().unwrap();
        break;
    }
}

fn server_connection(mut poll: mio::Poll, socket: Arc<UdpSocket>, mut config: Config) {
    let mut buf = [0; 65535];
    let mut out = [0; MAX_DATAGRAM_SIZE];

    let mut events = mio::Events::with_capacity(1024);

    let rng = SystemRandom::new();
    let conn_id_seed = ring::hmac::Key::generate(ring::hmac::HMAC_SHA256, &rng).unwrap();

    debug!("start server");
    loop {
        poll.poll(&mut events, None).unwrap();
        debug!("event! --> {:?}", events);

        'read: loop {
            // If the event loop reported no events, it means that the timeout
            // has expired, so handle it without attempting to read packets. We
            // will then proceed with the send loop.
            if events.is_empty() {
                debug!("timed out");
                // clients.values_mut().for_each(|c| c.conn.on_timeout());
                break 'read;
            }

            let mut clients_lock = CLIENT_LIST.lock().unwrap();

            let (len, from) = match socket.recv_from(&mut buf) {
                Ok(v) => v,

                Err(e) => {
                    // There are no more UDP packets to read, so end the read
                    // loop.
                    if e.kind() == std::io::ErrorKind::WouldBlock {
                        debug!("recv() would block");

                        break 'read;
                    }

                    panic!("recv() failed: {:?}", e);
                }
            };

            debug!("got {} bytes", len);

            let pkt_buf = &mut buf[..len];

            // Parse the QUIC packet's header.
            let hdr = match quiche::Header::from_slice(pkt_buf, quiche::MAX_CONN_ID_LEN) {
                Ok(v) => v,

                Err(e) => {
                    error!("Parsing packet header failed: {:?}", e);
                    continue 'read;
                }
            };

            debug!("got packet {:?}", hdr);

            let conn_id = ring::hmac::sign(&conn_id_seed, &hdr.dcid);
            let conn_id = &conn_id.as_ref()[..quiche::MAX_CONN_ID_LEN];
            let conn_id = conn_id.to_vec().into();

            // Lookup a connection based on the packet's connection ID. If there
            // is no connection matching, create a new one.
            let (client, client_cid) = if !clients_lock.contains_key(&hdr.dcid)
                && !clients_lock.contains_key(&conn_id)
            {
                if hdr.ty != quiche::Type::Initial {
                    error!("Packet is not Initial");
                    continue 'read;
                }

                if !quiche::version_is_supported(hdr.version) {
                    warn!("Doing version negotiation");

                    let len = quiche::negotiate_version(&hdr.scid, &hdr.dcid, &mut out).unwrap();

                    let out = &out[..len];

                    if let Err(e) = socket.send_to(out, from) {
                        if e.kind() == std::io::ErrorKind::WouldBlock {
                            debug!("send() would block");
                            break;
                        }

                        panic!("send() failed: {:?}", e);
                    }
                    continue 'read;
                }

                let mut scid = [0; quiche::MAX_CONN_ID_LEN];
                scid.copy_from_slice(&conn_id);

                let scid = quiche::ConnectionId::from_ref(&scid);

                // Token is always present in Initial packets.
                let token = hdr.token.as_ref().unwrap();

                // Do stateless retry if the client didn't send a token.
                if token.is_empty() {
                    warn!("Doing stateless retry");

                    let new_token = mint_token(&hdr, &from);

                    let len = quiche::retry(
                        &hdr.scid,
                        &hdr.dcid,
                        &scid,
                        &new_token,
                        hdr.version,
                        &mut out,
                    )
                    .unwrap();

                    let out = &out[..len];

                    if let Err(e) = socket.send_to(out, from) {
                        if e.kind() == std::io::ErrorKind::WouldBlock {
                            debug!("send() would block");
                            break;
                        }

                        panic!("send() failed: {:?}", e);
                    }
                    continue 'read;
                }

                let odcid = validate_token(&from, token);

                // The token was not valid, meaning the retry failed, so
                // drop the packet.
                if odcid.is_none() {
                    error!("Invalid address validation token");
                    continue 'read;
                }

                if scid.len() != hdr.dcid.len() {
                    error!("Invalid destination connection ID");
                    continue 'read;
                }

                // Reuse the source connection ID we sent in the Retry packet,
                // instead of changing it again.
                let scid = hdr.dcid.clone();

                info!("New connection: dcid={:?} scid={:?}", hdr.dcid, scid);

                let conn = quiche::accept(&scid, odcid.as_ref(), from, &mut config).unwrap();

                let client = Client {
                    conn,
                    partial_responses: HashMap::new(),
                };

                clients_lock.insert(scid.clone(), client);

                (clients_lock.get_mut(&scid).unwrap(), scid.clone())
            } else {
                match clients_lock.get_mut(&hdr.dcid) {
                    Some(v) => (v, hdr.dcid.clone()),

                    None => (clients_lock.get_mut(&conn_id).unwrap(), conn_id.clone()),
                }
            };

            let recv_info = quiche::RecvInfo { from };

            // Process potentially coalesced packets.
            let read = match client.conn.recv(pkt_buf, recv_info) {
                Ok(v) => v,

                Err(e) => {
                    error!("{} recv failed: {:?}", client.conn.trace_id(), e);
                    continue 'read;
                }
            };

            debug!("{} processed {} bytes", client.conn.trace_id(), read);


            println!("is !!!!!!!!!!!!!");
            if client.conn.is_in_early_data() || client.conn.is_established() {
                // Handle writable streams.
                println!("is !!!!!!!!!!!!!");
                for stream_id in client.conn.writable() {
                    debug!("writable????");
                    handle_writable(client, stream_id);
                }

                // Process all readable streams.
                for s in client.conn.readable() {
                    debug!("readable");
                    while let Ok((read, fin)) = client.conn.stream_recv(s, &mut buf) {
                        debug!("{} received {} bytes", client.conn.trace_id(), read);

                        let stream_buf = &buf[..read];

                        debug!(
                            "{} stream {} has {} bytes (fin? {})",
                            client.conn.trace_id(),
                            s,
                            stream_buf.len(),
                            fin
                        );

                        handle_sub(client, s, stream_buf, client_cid.clone());
                    }
                }
            }
            send_package(client, socket.clone(), &mut out);
        }
    }
}

fn server_alive_check() {
    loop {
        thread::sleep(std::time::Duration::from_millis(2000));
        let mut clients_lock = match CLIENT_LIST.lock() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let mut client_sub_lock = match CLIENT_SUBSCRIBE.lock() {
            Ok(v) => v,
            Err(_) => continue,
        };

        clients_lock.retain(|cid, ref mut c| {
            if c.conn.is_closed() {
                info!(
                    "{} connection collected {:?}",
                    c.conn.trace_id(),
                    c.conn.stats()
                );
                for cids in client_sub_lock.values_mut() {
                    cids.remove(&cid);
                }
            }

            !c.conn.is_closed()
        });
    }
}

fn server_send(ipcs: Vec<String>, socket: Arc<UdpSocket>) {

    let mut client_sub_lock = CLIENT_SUBSCRIBE.lock().unwrap();

    for ipc in ipcs {
    client_sub_lock.insert(ipc, HashSet::new());
    }
    // client_sub_lock.extend(iter)


    for topic in client_sub_lock.keys() {
        let url = format!("ipc:///tmp/{}.ipc", topic);
        let topic = topic.to_owned();
        let url = url.to_string();
        let network_socket = socket.clone();

        debug!("{}", url);
        thread::spawn(move || {
            let mut socket = Socket::new(Protocol::Sub).unwrap();
            let setopt = socket.subscribe("".as_bytes());
            let mut endpoint = socket.connect(&url).unwrap();

            match setopt {
                Ok(_) => debug!("Subscribed to '{:?}'.", topic),
                Err(err) => error!("Client failed to subscribe '{}'.", err),
            }

            let mut buf: Vec<u8> = Vec::new();
            loop {
                match socket.read_to_end(&mut buf) {
                    Ok(buf_size) => {
                        debug!("sub data len: {}", buf.len());
                        distribute(network_socket.clone(), topic.to_string(), &buf[..buf_size]);
                        buf.clear();
                    }
                    Err(err) => {
                        debug!("Client failed to receive msg '{}'.", err);
                        break;
                    }
                }
            }
            endpoint.shutdown().unwrap();
        });
    }
}

fn distribute(socket: Arc<UdpSocket>, key: String, data: &[u8]) {
    let client_sub = match CLIENT_SUBSCRIBE.lock() {
        Ok(v) => v,
        Err(_) => return,
    };
    let sub = match client_sub.get(&key) {
        Some(v) => v,
        None => return,
    };
    let mut clients = CLIENT_LIST.lock().unwrap();
    let mut out = [0u8; 1024];
    
    for cid in sub {
        let client = clients.get_mut(&cid).unwrap();

        if client.conn.is_established() {
            for stream_id in client.conn.writable() {
                if let Err(e) = client.conn.stream_send(stream_id, &data, false) {
                    println!("error: {:?}", e);
                };
            }

            info!("send client cid: {:?}", cid);
            send_package(client, socket.clone(), &mut out);
        }
    }
}

fn send_package(client: &mut Client, socket: Arc<UdpSocket>, out: &mut [u8]) {
    loop {
        let (write, send_info) = match client.conn.send(out) {
            Ok(v) => v,

            Err(quiche::Error::Done) => {
                debug!("{} done writing", client.conn.trace_id());
                break;
            }

            Err(e) => {
                error!("{} send failed: {:?}", client.conn.trace_id(), e);

                client.conn.close(false, 0x1, b"fail").ok();
                break;
            }
        };
        println!("send");

        if let Err(e) = socket.send_to(&out[..write], send_info.to) {
            if e.kind() == std::io::ErrorKind::WouldBlock {
                println!("send() would block");
                break;
            }

            panic!("send() failed: {:?}", e);
        }

        debug!("{} written {} bytes", client.conn.trace_id(), write);
    }
}

fn validate_token<'a>(src: &net::SocketAddr, token: &'a [u8]) -> Option<quiche::ConnectionId<'a>> {
    if token.len() < 6 {
        return None;
    }

    if &token[..6] != b"quiche" {
        return None;
    }

    let token = &token[6..];

    let addr = match src.ip() {
        std::net::IpAddr::V4(a) => a.octets().to_vec(),
        std::net::IpAddr::V6(a) => a.octets().to_vec(),
    };

    if token.len() < addr.len() || &token[..addr.len()] != addr.as_slice() {
        return None;
    }

    Some(quiche::ConnectionId::from_ref(&token[addr.len()..]))
}

fn mint_token(hdr: &quiche::Header, src: &net::SocketAddr) -> Vec<u8> {
    let mut token = Vec::new();

    token.extend_from_slice(b"quiche");

    let addr = match src.ip() {
        std::net::IpAddr::V4(a) => a.octets().to_vec(),
        std::net::IpAddr::V6(a) => a.octets().to_vec(),
    };

    token.extend_from_slice(&addr);
    token.extend_from_slice(&hdr.dcid);

    token
}

fn handle_writable(client: &mut Client, stream_id: u64) {
    let conn = &mut client.conn;

    debug!("{} stream {} is writable", conn.trace_id(), stream_id);

    if !client.partial_responses.contains_key(&stream_id) {
        return;
    }

    let resp = client.partial_responses.get_mut(&stream_id).unwrap();
    let body = &resp.body[resp.written..];

    let written = match conn.stream_send(stream_id, body, true) {
        Ok(v) => v,

        Err(quiche::Error::Done) => 0,

        Err(e) => {
            client.partial_responses.remove(&stream_id);

            error!("{} stream send failed {:?}", conn.trace_id(), e);
            return;
        }
    };

    resp.written += written;

    if resp.written == resp.body.len() {
        client.partial_responses.remove(&stream_id);
    }
}

fn handle_sub(
    client: &mut Client,
    write_stream_id: u64,
    buf: &[u8],
    cid: quiche::ConnectionId<'static>,
) {
    let actions = if let Ok(v) = String::from_utf8(buf.to_vec()) {
        v
    } else {
        return;
    };
    debug!("{}", actions);

    let mut client_sub_lock = CLIENT_SUBSCRIBE.lock().unwrap();

    // sub@xxx_xxx
    //  or
    // sub@xxx_xx;sub@xxx_xxx
    for action  in actions.split(";").into_iter() {
        let command: Vec<&str> = action.split("@").collect();
        if command.len() != 2 {
            break;
            }

        let msg = match command[0] {
            "sub" => {
                if let Some(client_sub_list) = client_sub_lock.get_mut(command[1]) {
                    debug!("{:?} sub {}", cid, command[1]);
                    client_sub_list.insert(cid.clone());
                    Ok("Success sub")
                } else {
                    Err("Subscription does not exist")
        }
            },
            "unsub" => {
                if let Some(client_sub_list) = client_sub_lock.get_mut(command[1]) {
                    client_sub_list.remove(&cid);
                    debug!("{:?} unsub {}", cid, command[1]);
                    Ok("Success nusub")
    } else {
                    Err("Subscription does not exist")
                }
            }
            _ => {
                Err("error: Non-existent Action")
            }
        };
        match msg {
            Err(msg) => {
                let msg = format!("ERROR: {}", msg);
                _ = client.conn.stream_send(
                    write_stream_id,
                    msg.as_bytes(),
                    false);
                warn!("cid: {:?}; {}", cid,msg);
            }
            Ok(msg) => {
                debug!("cid: {:?}; {}", cid, msg)
            }
        }
    }
}
