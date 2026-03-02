pub mod calls;
pub mod channels;
pub mod invites;
pub mod messages;
pub mod users;

use std::sync::OnceLock;

use crate::services::environment::{MONGODB_DATABASE, MONGODB_URI};

use mongodb::{Client, Database};

static DATABASE: OnceLock<Client> = OnceLock::new();

pub async fn connect() {
    let client = Client::with_uri_str(&*MONGODB_URI)
        .await
        .expect("Failed to connect to MongoDB");
    DATABASE.set(client).expect("Failed to set MongoDB client");
}

pub fn get_connection() -> &'static Client {
    DATABASE.get().expect("Failed to get MongoDB client")
}

pub fn get_database() -> Database {
    get_connection().database(&MONGODB_DATABASE)
}
