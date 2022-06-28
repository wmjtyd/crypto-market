///服务启动配置
#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize, Clone)]
pub struct ApplicationConfig {
    ///日志目录 "target/logs/"
    pub log_dir: String,
    ///日志等级
    pub log_level: String,
    ///端口
    pub server:ServerConfig,
    ///数据库
    pub record_dir: String,

    pub port: u16,
}

impl ApplicationConfig {

   pub fn new(toml_data:&str) ->Self
    {  let config = match toml::from_str(toml_data) {
        Ok(e) => e,
        Err(e) => panic!("{}", e),
    };
    config

    }
}

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize, Clone)]
pub struct ServerConfig {
    ///当前服务地址
    host: String,
    port: String,
}
