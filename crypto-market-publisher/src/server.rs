use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    io::Read,
    net::{self, SocketAddr},
    sync::{
        mpsc::{self, channel},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use mio::net::UdpSocket;
use nanomsg::{Protocol, Socket};
use quiche::Config;
use ring::rand::SystemRandom;

const MAX_DATAGRAM_SIZE: usize = 1350;

extern crate lazy_static;

extern crate log;

type ClientList = HashMap<quiche::ConnectionId<'static>, Client>;
type ClientTake = HashMap<String, HashSet<quiche::ConnectionId<'static>>>;
type CLientChannelTake = ((String, String), quiche::ConnectionId<'static>);

lazy_static! {
    static ref CLIENT_LIST: Mutex<ClientList> = {
        let map: ClientList = HashMap::new();
        Mutex::new(map)
    };
    static ref CLIENT_TAKE: Mutex<ClientTake> = {
        let map: ClientTake = HashMap::new();
        Mutex::new(map)
    };
    static ref CLIENT_QUIC_PACKAGE: ServerChannel<quiche::ConnectionId<'static>> = {
        let c: ServerChannel<quiche::ConnectionId<'static>> = ServerChannel::new();
        c
    };
    static ref SERVER_CHANNEL_TAKE: ServerTake = {
        let (send, revc) = mpsc::channel::<CLientChannelTake>();
        ServerTake {
            send: Arc::new(Mutex::new(send)),
            recv: Arc::new(Mutex::new(revc)),
        }
    };
    static ref SERVER: Server = {
        let mut socket = mio::net::UdpSocket::bind("127.0.0.1:4433".parse().unwrap()).unwrap();
        let poll = mio::Poll::new().unwrap();
        poll.registry()
            .register(&mut socket, mio::Token(0), mio::Interest::READABLE)
            .unwrap();

        Server {
            socket: Arc::new(socket),
            poll: Mutex::new(RefCell::new(poll)),
        }
    };
}

struct ServerTake {
    send: Arc<Mutex<mpsc::Sender<CLientChannelTake>>>,
    recv: Arc<Mutex<mpsc::Receiver<CLientChannelTake>>>,
}

struct ServerChannel<T> {
    tx: Arc<Mutex<mpsc::Sender<T>>>,
    rx: Arc<Mutex<mpsc::Receiver<T>>>,
}

impl<T> ServerChannel<T> {
    fn new() -> ServerChannel<T> {
        let (tx, rx) = channel::<T>();
        ServerChannel {
            tx: Arc::new(Mutex::new(tx)),
            rx: Arc::new(Mutex::new(rx)),
        }
    }
}

struct Server {
    socket: Arc<UdpSocket>,
    poll: Mutex<RefCell<mio::Poll>>,
}

impl Server {
    pub fn event_poll(&self, events: &mut mio::Events, timeout: Option<Duration>) {
        self.poll
            .lock()
            .unwrap()
            .get_mut()
            .poll(events, timeout)
            .unwrap();
    }
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

    println!("{:?}", addr);
    let mut socket = mio::net::UdpSocket::bind(addr).unwrap();
    let poll = mio::Poll::new().unwrap();
    poll.registry()
        .register(&mut socket, mio::Token(0), mio::Interest::READABLE)
        .unwrap();

    let server = Server {
        socket: Arc::new(socket),
        poll: Mutex::new(RefCell::new(poll)),
    };

    let server = Arc::new(server);

    let server_r = server.clone();

    // 连接初始
    service.push(thread::spawn(move || {
        server_connection(server_r, config);
    }));

    // 订阅
    service.push(thread::spawn(|| {
        server_take();
    }));

    // 存活检查
    service.push(thread::spawn(|| {
        server_survive();
    }));

    let server_r = server.clone();
    // 发送
    service.push(thread::spawn(|| {
        server_distribute(ipc, server_r);
    }));

    let server_r = server.clone();
    // quic数据包
    service.push(thread::spawn(|| {
        server_quic_packets(server_r);
    }));

    for s in service {
        s.join().unwrap();
        break;
    }
}

fn server_connection(server: Arc<Server>, mut config: Config) {
    let mut buf = [0; 65535];
    let mut out = [0; MAX_DATAGRAM_SIZE];

    let mut events = mio::Events::with_capacity(1024);

    let socket = server.socket.clone();

    let rng = SystemRandom::new();
    let conn_id_seed = ring::hmac::Key::generate(ring::hmac::HMAC_SHA256, &rng).unwrap();

    println!("start server");
    loop {
        server.event_poll(&mut events, None);
        println!("event! --> {:?}", events);

        'read: loop {
            // If the event loop reported no events, it means that the timeout
            // has expired, so handle it without attempting to read packets. We
            // will then proceed with the send loop.
            if events.is_empty() {
                println!("timed out");
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
                        println!("recv() would block");
                        break 'read;
                    }

                    panic!("recv() failed: {:?}", e);
                }
            };

            println!("got {} bytes", len);

            let pkt_buf = &mut buf[..len];

            // Parse the QUIC packet's header.
            let hdr = match quiche::Header::from_slice(pkt_buf, quiche::MAX_CONN_ID_LEN) {
                Ok(v) => v,

                Err(e) => {
                    error!("Parsing packet header failed: {:?}", e);
                    continue 'read;
                }
            };

            println!("got packet {:?}", hdr);

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
                            println!("send() would block");
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
                            println!("send() would block");
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

                println!("New connection: dcid={:?} scid={:?}", hdr.dcid, scid);

                let conn = quiche::accept(&scid, odcid.as_ref(), from, &mut config).unwrap();

                let client = Client {
                    conn,
                    partial_responses: HashMap::new(),
                };

                clients_lock.insert(scid.clone(), client);
                println!("insert");

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

            println!("{} processed {} bytes", client.conn.trace_id(), read);

            if client.conn.is_in_early_data() || client.conn.is_established() {
                // Handle writable streams.
                for stream_id in client.conn.writable() {
                    handle_writable(client, stream_id);
                }

                // Process all readable streams.
                for s in client.conn.readable() {
                    while let Ok((read, fin)) = client.conn.stream_recv(s, &mut buf) {
                        println!("{} received {} bytes", client.conn.trace_id(), read);

                        let stream_buf = &buf[..read];

                        println!(
                            "{} stream {} has {} bytes (fin? {})",
                            client.conn.trace_id(),
                            s,
                            stream_buf.len(),
                            fin
                        );

                        handle_stream(client, s, stream_buf, client_cid.clone());
                    }
                }
            }
            if let Ok(tx) = CLIENT_QUIC_PACKAGE.tx.lock() {
                tx.send(client_cid.clone()).unwrap();
            }
        }
    }
}

fn server_take() {
    loop {
        let ((action, take_key), cid) = SERVER_CHANNEL_TAKE.recv.lock().unwrap().recv().unwrap();
        let mut client_take_lock = CLIENT_TAKE.lock().unwrap();
        let action = &action[..];
        match action {
            "ADD" => {
                if let Some(v) = client_take_lock.get_mut(&take_key) {
                    debug!("客户端订阅成功 {:?}", cid);
                    v.insert(cid);
                } else {
                    warn!("不存在的订阅");
                }
            }
            "RM" => {
                if let Some(v) = client_take_lock.get_mut(&take_key) {
                    v.remove(&cid);
                    debug!("客户端取消订阅成功 {:?}", cid);
                } else {
                    warn!("不存在的订阅");
                }
            }
            _ => continue,
        }
    }
}

fn server_survive() {
    loop {
        thread::sleep(std::time::Duration::from_millis(2000));
        let mut clients_lock = match CLIENT_LIST.lock() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let mut client_take_lock = match CLIENT_TAKE.lock() {
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
                for cids in client_take_lock.values_mut() {
                    cids.remove(&cid);
                }
            }

            !c.conn.is_closed()
        });
    }
}

fn server_distribute(ipc: String, server: Arc<Server>) {
    let url = format!("ipc:///tmp/{}.ipc", ipc);
    // let take_list = vec!["data"];
    let mut client_take_lock = CLIENT_TAKE.lock().unwrap();

    client_take_lock.insert(ipc, HashSet::new());

    debug!("{}", url);

    
    for topic in  client_take_lock.keys(){
        let topic = topic.to_owned();
        let url = url.to_string();
        let server_r = server.clone();
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
                        distribute(server_r.clone(), topic.to_string(), &buf[..buf_size]);
                        buf.clear();
                    }
                    Err(err) => {
                        println!("Client failed to receive msg '{}'.", err);
                        break;
                    }
                }
            }
            endpoint.shutdown().unwrap();
        });
    }
}

fn distribute(server: Arc<Server>, key: String, data: &[u8]) {
    let socket = server.socket.clone();
    let client_take = match CLIENT_TAKE.lock() {
        Ok(v) => v,
        Err(_) => return,
    };
    let take = match client_take.get(&key) {
        Some(v) => v,
        None => return,
    };
    let mut clients = CLIENT_LIST.lock().unwrap();
    let mut out = [0u8; 1024];
    for cid in take {
        let client = clients.get_mut(&cid).unwrap();

        if client.conn.is_established() {

            for stream_id in client.conn.writable() {
                if let Err(e) = client.conn.stream_send(stream_id, &data, false) {
                    println!("error: {:?}", e);
                };
            }

            send(client, socket.clone(), &mut out);
        }
    }
}

fn server_quic_packets(server: Arc<Server>) {
    let rx = CLIENT_QUIC_PACKAGE.rx.lock().unwrap();
    let socket = server.socket.clone();
    let mut out = [0u8; 1024];

    loop {
        if let Ok(cid) = rx.recv() {
            if let Ok(mut clients) = CLIENT_LIST.lock() {
                if let Some(client) = clients.get_mut(&cid) {
                    send(client, socket.clone(), &mut out);
                }
            }
        }
    }
}

fn send(client: &mut Client, socket: Arc<UdpSocket>, out: &mut [u8]) {
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

fn handle_stream(
    _client: &mut Client,
    _stream_id: u64,
    buf: &[u8],
    cid: quiche::ConnectionId<'static>,
) {
    let command = if let Ok(v) = String::from_utf8(buf.to_vec()) {
        v
    } else {
        return;
    };

    if let Some(_v) = command.find(";") {
        let commands: Vec<&str> = command.split(";").collect();
        let send_lock = SERVER_CHANNEL_TAKE.send.lock().unwrap();
        for i in commands {
            if let Some(num) = i.find(" ") {
                let data = (
                    (command[..num].to_string(), i[num + 1..].to_string()),
                    cid.clone(),
                );
                debug!("{:?}", data);
                send_lock.send(data).unwrap();
            }
        }
    } else {
        if let Some(num) = command.find(" ") {
            let data = (
                (command[..num].to_string(), command[num + 1..].to_string()),
                cid.clone(),
            );
            let send_lock = SERVER_CHANNEL_TAKE.send.lock().unwrap();
            send_lock.send(data).unwrap();
        }
    }
}
