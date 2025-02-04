use mongodb::bson::doc;
use serde::{Deserialize, Serialize};

use crate::errors::{Error, Result};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Event {
    id: String,
    name: String,
    description: String,
    event_type: String,
    creator_id: String,
    full_day: bool,
    start_time: Option<i32>,
    end_time: Option<i32>,
    day: Option<i32>,
    recurrence: Option<Recurrence>,
    time_zone: String,
    location: String,
    external_location: bool,
    external_link: Option<String>,
    rsvp_attendees: Option<Vec<Attendee>>,
}

pub enum RecurringDay {
    Sunday = 0x1,
    Monday = 0x2,
    Tuesday = 0x4,
    Wednesday = 0x8,
    Thursday = 0x10,
    Friday = 0x20,
    Saturday = 0x40,
}

#[derive(Clone, Debug)]
pub struct RecurringDays {
    days: i32,
}

impl RecurringDays {
    pub fn new() -> Self {
        Self { days: 0 }
    }

    pub fn add_day(&mut self, day: RecurringDay) {
        self.days |= day as i32;
    }

    pub fn remove_day(&mut self, day: RecurringDay) {
        self.days &= !(day as i32);
    }

    pub fn contains(&self, day: RecurringDay) -> bool {
        self.days & (day as i32) != 0
    }
}

impl Default for RecurringDays {
    fn default() -> Self {
        Self::new()
    }
}

impl Serialize for RecurringDays {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_i32(self.days)
    }
}

impl<'de> Deserialize<'de> for RecurringDays {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let days = i32::deserialize(deserializer)?;
        Ok(RecurringDays { days })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AnnualRecurringDay {
    day: i32,
    month: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Recurrence {
    Daily {
        interval: i32,
        end_date: Option<i32>,
    },
    Weekly {
        interval: i32,
        end_date: Option<i32>,
        days: RecurringDays,
    },
    Monthly {
        interval: i32,
        end_date: Option<i32>,
        days: Vec<i32>,
    },
    Yearly {
        interval: i32,
        end_date: Option<i32>,
        days: Vec<AnnualRecurringDay>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Attendee {
    id: String,
    status: AttendeeStatus,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AttendeeStatus {
    Awaiting,
    Accepted,
    Declined,
    Tentative,
}

pub async fn get_event(event_id: String) -> Result<Event> {
    let database = super::get_database();
    let event = database
        .collection::<Event>("events")
        .find_one(doc! {
            "id": event_id,
        })
        .await?;
    match event {
        Some(event) => Ok(event),
        None => Err(Error::NotFound),
    }
}
