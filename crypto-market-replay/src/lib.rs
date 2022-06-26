use tokio::fs::{read_to_string};

use crate::config::config::ApplicationConfig;

pub mod config;

pub async fn init_context() {
    init_config().await;

}


//初始化配置信息
pub async fn init_config()->ApplicationConfig {
    let content = read_to_string("application.toml").await.unwrap();
    let config = ApplicationConfig::new(content.as_str());
    println!("读取配置成功");
    config
}

