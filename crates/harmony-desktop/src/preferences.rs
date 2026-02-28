use keyring::Entry;
use rkyv::{Archive, Deserialize, Serialize};
// PURPOSE: user preferences

pub enum Language {
    English,
    Spanish,
    French,
    Chinese,
}

pub struct Preferences {
    language: Language,
}

impl Preferences {
    pub fn load() -> Self {
        todo!()
    }

    pub fn save(&self) {
        todo!()
    }
}

// PURPOSE: items to be stored in encrypted db
// contains sensitive information such as messages, user data
pub struct Store {}

#[derive(Archive, Debug, Clone, Serialize, Deserialize)]
pub struct Keys {
    token: String,
    encryption_key: Vec<u8>,
}

impl Keys {
    pub fn load() -> Option<Self> {
        let entry = Entry::new("harmony_desktop", "user_token").unwrap();
        let secret = entry.get_secret().ok()?;
        rkyv::from_bytes::<_, rkyv::rancor::Error>(&secret).ok()
    }

    pub fn save(&self) {
        let entry = Entry::new("harmony_desktop", "user_token").unwrap();
        let serialized = rkyv::to_bytes::<rkyv::rancor::Error>(self).unwrap();
        entry
            .set_secret(&serialized)
            .expect("Failed to save keys to keyring");
    }
}
