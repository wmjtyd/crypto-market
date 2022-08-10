use clap::clap_app;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let matches: clap::ArgMatches = clap_app!(quic =>
        (about: "multiceast server and client")
        (@arg MODE: -m "server or client; default server")
        (@arg IP: --ip  "ip addr")
        (@arg PORT: --port "ip port")
        (@arg CONFIGPATH: -c "config path")
    )
    .get_matches();

    let ip = matches.value_of("IP").unwrap_or("230.0.0.1");
    let port = matches.value_of("PORT").unwrap_or("8888");

    let config_path = matches.value_of("CONFIGPATH").unwrap_or("./conf/ipc.json");

    let mode = matches.value_of("MODE").unwrap_or("server");

    match mode {
        "server" => {
            crypto_market_multicast::server(ip, port, config_path);
        }
        "client" => {
            let rx = crypto_market_multicast::client(ip, port);
            while let Ok(data) = rx.recv() {
                println!("{:?}", data);
            }
        }
        _ => panic!("error mode <server> or <client>"),
    };
}
