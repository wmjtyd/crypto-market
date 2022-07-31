use clap::clap_app;

#[tokio::main]
async fn main() {
    let matches: clap::ArgMatches = clap_app!(quic =>
            (about: "Publish data in the form of multicast")
            (@subcommand server =>
                (@arg IP: +required "comma_seperated_symbols")
                (@arg PORT: +required "comma_seperated_symbols")
                (@arg IPC: +required +use_delimiter "comma_seperated_symbols")
            )
            (@subcommand client =>
                (@arg IP: +required "comma_seperated_symbols")
                (@arg PORT: +required "comma_seperated_symbols")
            )
    )
    .get_matches();

    match matches.subcommand() {
        ("server", m) => {
            let m = m.expect("command error");
            let ip = m.value_of("IP").unwrap();
            let port = m.value_of("PORT").unwrap();

            let ipc = m.values_of("IPC").unwrap().collect::<Vec<&str>>();

            crypto_market_multicast::server(ip, port, ipc).await;
        }
        ("client", m) => {
            let m = m.expect("command error");
            let ip = m.value_of("IP").unwrap();
            let port = m.value_of("PORT").unwrap();

            let rx = crypto_market_multicast::client(ip, port);
            while let Ok(data) = rx.recv() {
                println!("{:?}", data);
            }
        }
        _ => {}
    };
}
