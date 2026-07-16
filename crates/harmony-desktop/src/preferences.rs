use std::{
    fs::{self, File},
    io::{BufReader, BufWriter},
    path::PathBuf,
    sync::{Arc, OnceLock},
};

use arc_swap::{ArcSwap, DefaultStrategy, Guard};
use keyring_core::Entry;
use serde::{Deserialize, Serialize};
// user preferences

static PREFERENCES: OnceLock<ArcSwap<Preferences>> = OnceLock::new();

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub enum Locale {
    #[default]
    #[serde(rename = "system")]
    System,
    #[serde(rename = "en")]
    En,
    #[serde(rename = "es")]
    Es,
    #[serde(rename = "fr")]
    Fr,
    #[serde(rename = "zh-CN")]
    ZhCn,
    #[serde(rename = "zh-TW")]
    ZhTw,
}

impl Locale {
    pub fn code(&self) -> String {
        match self {
            Locale::System => sys_locale::get_locale().unwrap_or("en".to_string()),
            _ => serde_json::to_value(self)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string(),
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            Locale::System => "System default",
            Locale::En => "English",
            Locale::Es => "Español",
            Locale::Fr => "Français",
            Locale::ZhCn => "简体中文",
            Locale::ZhTw => "繁體中文",
        }
    }

    pub fn all() -> Vec<Locale> {
        vec![
            Locale::System,
            Locale::En,
            Locale::Es,
            Locale::Fr,
            Locale::ZhCn,
            Locale::ZhTw,
        ]
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Preferences {
    pub locale: Locale,
}

fn get_config_path() -> PathBuf {
    let data = dirs::config_dir().expect("Could not resolve config directory");
    data.join("Nextania")
        .join("Harmony")
        .join("preferences.json")
}

impl Preferences {
    pub fn load() -> Self {
        let config_path = get_config_path();
        if !config_path.exists() {
            fs::create_dir_all(config_path.parent().unwrap())
                .expect("Could not create config directory");
            let file = File::create(config_path.clone()).expect("Could not create config file");
            let writer = BufWriter::new(file);
            serde_json::to_writer_pretty(writer, &Preferences::default()).unwrap();
        }
        let file = File::open(config_path).expect("Failed to open config file");
        let reader = BufReader::new(file);
        let preferences: Preferences =
            serde_json::from_reader(reader).expect("Failed to read config file");
        PREFERENCES
            .set(ArcSwap::from_pointee(preferences.clone()))
            .expect("Failed to set preferences");
        preferences
    }

    pub fn get() -> Guard<Arc<Self>, DefaultStrategy> {
        PREFERENCES.get().expect("Preferences not loaded").load()
    }

    pub fn get_clone() -> Self {
        Preferences::clone(&PREFERENCES.get().expect("Preferences not loaded").load())
    }

    pub fn set(self) {
        PREFERENCES
            .get()
            .expect("Preferences not loaded")
            .store(Arc::new(self))
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        let config_path = get_config_path();
        if !config_path.exists() {
            fs::create_dir_all(config_path.parent().unwrap())?;
        }
        let file = File::create(config_path.clone())?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, self)?;
        Ok(())
    }
}

// items to be stored in encrypted db
// contains sensitive information such as messages, user data
pub struct Store {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Keys {
    token: String,
    encryption_key: Vec<u8>,
}

impl Keys {
    pub fn load() -> Option<Self> {
        let entry = Entry::new("harmony_desktop", "user_token").ok()?;
        let secret = entry.get_secret().ok()?;
        serde_cbor_2::from_slice(&secret).ok()
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let entry = Entry::new("harmony_desktop", "user_token")?;
        let serialized =
            serde_cbor_2::to_vec(self).map_err(|e| format!("Failed to serialize keys: {e}"))?;
        entry.set_secret(&serialized)?;
        Ok(())
    }
}
