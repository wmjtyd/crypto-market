use std::net::SocketAddr;

#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, Clone)]
pub struct ServerConfig {
    /// bind host name
    pub host: String,

    /// bind server port
    pub port: u16,
}

/// server application config
#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, Clone)]
pub struct ApplicationConfig {
    /// log save directory "target/logs/"
    pub log_dir: String,
    /// log show level
    pub log_level: String,
    /// server base config
    pub server: ServerConfig,
    /// market data base directory
    pub market_data_dir: String,
}

impl ApplicationConfig {
    pub fn read_file(toml_data: &str) -> Self {
        let config = match toml::from_str(toml_data) {
            Ok(e) => e,
            Err(e) => panic!("{}", e),
        };
        println!("{:?}", config);
        config
    }

    pub fn new(config: &clap::ArgMatches) -> Self {
        let server = ServerConfig::new(config);
        
        let log_dir = config.value_of("LOG").unwrap_or("logs").to_string();
        let log_level = config.value_of("LEVEL").unwrap_or("info").to_string();
        let market_data_dir = config.value_of("DATA").unwrap_or("./").to_string();

        Self { log_dir, log_level, server, market_data_dir}
    }
}

impl ServerConfig {
    pub fn new(config: &clap::ArgMatches) -> Self {
        let host = config.value_of("HOST").unwrap_or("127.0.0.1").to_string();
        let port = config.value_of("POST").unwrap_or("3001").parse().expect("port format error");
        Self {
            port,
            host,
        }
    }
    pub fn get_addr(&self) -> SocketAddr {
        const MSG: &str = "addr format error";
        let host: Vec<u8> = self
            .host
            .split(".")
            .map(|num| num.parse::<u8>().expect(MSG))
            .collect();

        SocketAddr::from((
            [
                *host.get(0).expect(MSG),
                *host.get(1).expect(MSG),
                *host.get(2).expect(MSG),
                *host.get(3).expect(MSG),
            ],
            self.port,
        ))
    }
}
