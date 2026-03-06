use keyring::Entry;
use rkyv::{Archive, Deserialize, Serialize};
// user preferences

#[derive(Debug, Clone)]
pub enum Locale {
    En,
    Es,
    Fr,
    ZhCn,
    ZhTw,
}

impl Locale {
    pub fn code(&self) -> &str {
        match self {
            Locale::En => "en",
            Locale::Es => "es",
            Locale::Fr => "fr",
            Locale::ZhCn => "zh-CN",
            Locale::ZhTw => "zh-TW",
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            Locale::En => "English",
            Locale::Es => "Español",
            Locale::Fr => "Français",
            Locale::ZhCn => "简体中文",
            Locale::ZhTw => "繁體中文",
        }
    }

    pub fn all() -> Vec<Locale> {
        vec![
            Locale::En,
            Locale::Es,
            Locale::Fr,
            Locale::ZhCn,
            Locale::ZhTw,
        ]
    }
}

pub struct Preferences {
    locale: Locale,
}

impl Preferences {
    pub fn load() -> Self {
        todo!()
    }

    pub fn save(&self) {
        todo!()
    }
}

// items to be stored in encrypted db
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
