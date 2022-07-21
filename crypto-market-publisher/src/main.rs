#[macro_use]
mod client;
mod server;


use std::net::SocketAddr;

use clap::{clap_app, ArgMatches};

use server::create_server;


#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate log;

const MAX_DATAGRAM_SIZE: usize = 1350;

fn start_server(m: &ArgMatches) {
    let addr: SocketAddr = if let Some(v) = m.value_of("ADDR") {
        v.parse().unwrap()
    } else {
        "127.0.0.1:4433".parse().unwrap()
    };

    let crt = m.value_of("CRT").unwrap_or("./examples/cert.crt");

    let key = m.value_of("KEY").unwrap_or("./examples/cert.key");

    let exchange = m.value_of("EXCHANGE").unwrap();
    let market_type = m.value_of("MARKET_TYPE").unwrap();
    let msg_type = m.value_of("MSG_TYPE").unwrap();

    let ipc = if let Some(period) = m.value_of("PERIOD") {
        format!("{}_{}_{}_{}", exchange, market_type, msg_type, period)
    } else {
        format!("{}_{}_{}", exchange, market_type, msg_type)
    };
    debug!("ipc: {}", ipc);

    let mut config = quiche::Config::new(quiche::PROTOCOL_VERSION).unwrap();

    debug!("{} {}", crt, key);

    config.load_cert_chain_from_pem_file(crt).unwrap();
    config.load_priv_key_from_pem_file(key).unwrap();

    config
        .set_application_protos(b"\x0ahq-interop\x05hq-29\x05hq-28\x05hq-27\x08http/0.9")
        .unwrap();

    config.set_max_idle_timeout(5000);
    config.set_max_recv_udp_payload_size(MAX_DATAGRAM_SIZE);
    config.set_max_send_udp_payload_size(MAX_DATAGRAM_SIZE);
    config.set_initial_max_data(10_000_000);
    config.set_initial_max_stream_data_bidi_local(1_000_000);
    config.set_initial_max_stream_data_bidi_remote(1_000_000);
    config.set_initial_max_stream_data_uni(1_000_000);
    config.set_initial_max_streams_bidi(100);
    config.set_initial_max_streams_uni(100);
    config.set_disable_active_migration(true);
    config.enable_early_data();

    create_server(addr, ipc, config);
}

fn start_client(m: &ArgMatches) {
    let addr: SocketAddr = if let Some(v) = m.value_of("ADDR") {
        v.parse().expect("addr input error")
    } else {
        "127.0.0.1:4433".parse().unwrap()
    };

    let mut config = quiche::Config::new(quiche::PROTOCOL_VERSION).unwrap();

    let exchange = m.value_of("EXCHANGE").unwrap();
    let market_type = m.value_of("MARKET_TYPE").unwrap();
    let msg_type = m.value_of("MSG_TYPE").unwrap();

    let ipc = if let Some(period) = m.value_of("PERIOD") {
        format!("{}_{}_{}_{}", exchange, market_type, msg_type, period)
    } else {
        format!("{}_{}_{}", exchange, market_type, msg_type)
    };

    let _subscribe_list = vec![ipc.as_str()];

    // *CAUTION*: this should not be set to `false` in production!!!
    config.verify_peer(false);

    config
        .set_application_protos(b"\x0ahq-interop\x05hq-29\x05hq-28\x05hq-27\x08http/0.9")
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


    let rx = client::create_client(addr, config, ipc);
    for i in rx.iter() {
        // decode space
        println!("{:?}", i);
    }
}

#[tokio::main]
async fn main() {
    env_logger::init().unwrap();
    let matches: clap::ArgMatches = clap_app!(quic =>
        (@subcommand server =>
            (about: "start server")
            (@arg ADDR: +required "ip addr")
            (@arg EXCHANGE:     +required "exchange")
            (@arg MARKET_TYPE:  +required "market_type")
            (@arg MSG_TYPE:     +required "msg_type")
            (@arg PERIOD: "period")
            (@arg CRT: -c --crt +takes_value "ctr file path")
            (@arg KEY: -k --key +takes_value "key file path")
        )
        (@subcommand client =>
            (about: "use client")
            (@arg ADDR:         +required "ip addr")
            (@arg EXCHANGE:     +required "exchange")
            (@arg MARKET_TYPE:  +required "market_type")
            (@arg MSG_TYPE:     +required "msg_type")
            (@arg PERIOD: "period")
        )
    )
    .get_matches();

    match matches.subcommand() {
        ("server", m) => start_server(m.unwrap()),
        ("client", m) => start_client(m.unwrap()),
        _ => {}
    };
}
