use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Emoji {
    id: String,
    name: String,
    file_id: String,
}
