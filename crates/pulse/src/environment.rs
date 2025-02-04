use std::{env, net::IpAddr};

use lazy_static::lazy_static;
use pulse_api::Region;

lazy_static! {
    pub static ref LISTEN_ADDRESS: String =
        env::var("LISTEN_ADDRESS").unwrap_or("0.0.0.0:3001".to_string());
    pub static ref SOCKET_ADDRESS: IpAddr = env::var("SOCKET_ADDRESS")
        .unwrap_or("209.145.60.11".to_string())
        .parse()
        .unwrap();
    pub static ref PUBLIC_ADDRESS: String =
        env::var("PUBLIC_ADDRESS").unwrap_or("209.145.60.11".to_string());
    pub static ref REDIS_URI: String = env::var("REDIS_URI").expect("REDIS_URI must be set");
    pub static ref REGION: Region = env::var("REGION").expect("REGION must be set").parse().expect("Invalid region");
}
