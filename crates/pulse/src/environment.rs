use std::env;

use lazy_static::lazy_static;
use pulse_types::Region;

use crate::mls::ExternalSenderIdentity;

lazy_static! {
    pub static ref LISTEN_ADDRESS: String =
        env::var("LISTEN_ADDRESS").unwrap_or("0.0.0.0:3001".to_string());
    pub static ref PUBLIC_ADDRESS: String =
        env::var("PUBLIC_ADDRESS").unwrap_or("192.168.0.101".to_string());
    pub static ref REDIS_URI: String = env::var("REDIS_URI").expect("REDIS_URI must be set");
    pub static ref REGION: Region = env::var("REGION")
        .expect("REGION must be set")
        .parse()
        .expect("Invalid region");

    /// External sender identity for MLS group management
    /// Generated once at startup and used for all Add/Remove proposals
    pub static ref EXTERNAL_SENDER: ExternalSenderIdentity =
        ExternalSenderIdentity::generate("pulse")
            .expect("Failed to generate external sender identity");
}
