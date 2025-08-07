use futures_util::StreamExt;
use mongodb::bson::{self, doc};
use serde::{Deserialize, Serialize};

use super::channels::Channel;
use crate::errors::{Error, Result};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Status {
    Online = 0,
    Idle = 1,
    Busy = 2,
    BusyNotify = 3,
    Invisible = 4,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Presence {
    status: Status,
    message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Relationship {
    Established = 0,
    Blocked = 1,
    Requested = 2,
    Pending = 3,
}

// TODO: allow disabling of friend requests
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Contact {
    id: String,
    relationship: Relationship,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ContactExtended {
    id: String,
    relationship: Relationship,
    user: User,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct User {
    pub id: String,
    pub profile_banner: Option<String>, // TODO: Make use of file handling
    pub profile_description: String,
    pub contacts: Vec<Contact>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub online: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence: Option<Presence>,
}

impl User {
    pub async fn in_channel(&self, channel: &Channel) -> Result<bool> {
        match channel {
            Channel::PrivateChannel {
                initiator_id,
                target_id,
                ..
            } => {
                if initiator_id == &self.id || target_id == &self.id {
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Channel::GroupChannel { members, .. } => {
                Ok(members.iter().any(|member| member.id == self.id))
            }
        }
    }

    pub async fn get(id: &String) -> Result<User> {
        let users = super::get_database().collection::<User>("users");
        let user = users
            .find_one(doc! {
                "id": id
            })
            .await?;
        match user {
            Some(user) => Ok(user),
            None => Err(Error::NotFound),
        }
    }
    pub async fn get_by_username(username: &String) -> Result<User> {
        let users = super::get_database().collection::<User>("users");
        let user = users
            .find_one(doc! {
                "username": username
            })
            .await?;
        match user {
            Some(user) => Ok(user),
            None => Err(Error::NotFound),
        }
    }

    pub async fn create(id: String) -> Result<User> {
        let users = super::get_database().collection::<User>("users");
        let user = User {
            id,
            profile_banner: None,
            profile_description: String::new(),
            contacts: Vec::new(),
            online: None,
            presence: None,
        };
        users.insert_one(user.clone()).await?;
        Ok(user)
    }

    pub async fn add_contact(&self, contact_id: &String) -> Result<()> {
        let users = super::get_database().collection::<User>("users");
        User::get(contact_id).await?;
        let contact = self.contacts.iter().find(|a| &a.id == contact_id);
        if let Some(contact) = contact {
            match contact.relationship {
                Relationship::Established => Err(Error::AlreadyEstablished),
                Relationship::Blocked => Err(Error::Blocked),
                Relationship::Requested => Err(Error::AlreadyRequested),
                Relationship::Pending => {
                    users
                        .update_one(
                            doc! {
                                "id": &self.id
                            },
                            doc! {
                                "$set": {
                                    "contacts.$[contact].relationship": bson::to_bson(&Relationship::Established)?
                                }
                            }).with_options(
                            Some(mongodb::options::UpdateOptions::builder()
                                .array_filters(vec![doc! {
                                    "contact.id": &contact_id
                                }])
                                .build()),
                        )
                        .await?;
                    users
                        .update_one(
                            doc! {
                                "id": &contact_id
                            },
                            doc! {
                                "$set": {
                                    "contacts.$[contact].relationship": bson::to_bson(&Relationship::Established)?
                                }
                            }).with_options(
                            Some(mongodb::options::UpdateOptions::builder()
                                .array_filters(vec![doc! {
                                    "contact.id": &self.id
                                }])
                                .build()),
                        )
                        .await?;
                    Ok(())
                }
            }
        } else {
            users
                .update_one(
                    doc! {
                        "id": &self.id
                    },
                    doc! {
                        "$push": {
                            "contacts": {
                                "id": contact_id,
                                "relationship": bson::to_bson(&Relationship::Requested)?
                            }
                        }
                    },
                )
                .await?;
            users
                .update_one(
                    doc! {
                        "id": &contact_id
                    },
                    doc! {
                        "$push": {
                            "contacts": {
                                "id": &self.id,
                                "relationship": bson::to_bson(&Relationship::Pending)?
                            }
                        }
                    },
                )
                .await?;
            Ok(())
        }
    }

    pub async fn remove_contact(&self, contact_id: &String) -> Result<()> {
        let users = super::get_database().collection::<User>("users");
        User::get(contact_id).await?;
        let contact = self.contacts.iter().find(|a| &a.id == contact_id);
        if let Some(contact) = contact {
            match contact.relationship {
                // remove contact
                Relationship::Established => {
                    users
                        .update_one(
                            doc! {
                                "id": &self.id
                            },
                            doc! {
                                "$pull": {
                                    "contacts": {
                                        "id": contact_id
                                    }
                                }
                            },
                        )
                        .await?;
                    users
                        .update_one(
                            doc! {
                                "id": contact_id
                            },
                            doc! {
                                "$pull": {
                                    "contacts": {
                                        "id": &self.id
                                    }
                                }
                            },
                        )
                        .await?;
                    Ok(())
                }
                Relationship::Blocked => Err(Error::Blocked),
                // revoke friend request
                Relationship::Requested => {
                    users
                        .update_one(
                            doc! {
                                "id": &self.id
                            },
                            doc! {
                                "$pull": {
                                    "contacts": {
                                        "id": contact_id
                                    }
                                }
                            },
                        )
                        .await?;
                    users
                        .update_one(
                            doc! {
                                "id": contact_id
                            },
                            doc! {
                                "$pull": {
                                    "contacts": {
                                        "id": &self.id
                                    }
                                }
                            },
                        )
                        .await?;
                    Ok(())
                }
                // deny friend request
                Relationship::Pending => {
                    users
                        .update_one(
                            doc! {
                                "id": &self.id
                            },
                            doc! {
                                "$pull": {
                                    "contacts": {
                                        "id": contact_id
                                    }
                                }
                            },
                        )
                        .await?;
                    users
                        .update_one(
                            doc! {
                                "id": contact_id
                            },
                            doc! {
                                "$pull": {
                                    "contacts": {
                                        "id": &self.id
                                    }
                                }
                            },
                        )
                        .await?;
                    Ok(())
                }
            }
        } else {
            Err(Error::NotFound)
        }
    }

    pub async fn get_established_contacts(&self) -> Result<Vec<User>> {
        let users = super::get_database().collection::<User>("users");
        let contacts = self.contacts.iter().map(|contact| async {
            if contact.relationship == Relationship::Established {
                let user = users
                    .find_one(doc! {
                        "id": &contact.id
                    })
                    .await
                    .ok()?;
                match user {
                    Some(user) => Some(user),
                    None => None,
                }
            } else {
                None
            }
        });
        let contacts: Vec<User> = futures_util::future::join_all(contacts)
            .await
            .iter()
            .filter_map(|contact| contact.clone())
            .collect();
        Ok(contacts)
    }

    pub async fn get_contacts(&self) -> Result<Vec<ContactExtended>> {
        let users = super::get_database().collection::<User>("users");
        let contacts = self.contacts.iter().map(|contact| async {
            let user = users
                .find_one(doc! {
                    "id": &contact.id
                })
                .await
                .ok()?;
            match user {
                Some(user) => Some(ContactExtended {
                    id: contact.id.clone(),
                    relationship: contact.relationship.clone(),
                    user,
                }),
                None => None,
            }
        });
        let contacts: Vec<ContactExtended> = futures_util::future::join_all(contacts)
            .await
            .iter()
            .filter_map(|contact| contact.clone())
            .collect();
        Ok(contacts)
    }

    // pub async fn accept_invite(&self, invite_code: &String) -> Result<Space> {
    //     let invites = super::get_database().collection::<Invite>("invites");
    //     let spaces = super::get_database().collection::<Space>("spaces");
    //     let invite = invites
    //         .find_one_and_update(
    //             doc! {
    //                 "id": invite_code,
    //             },
    //             doc! {
    //                 "$push": {
    //                     "uses": &self.id,
    //                 }
    //             },
    //         )
    //         .await?;
    //     let invite = match invite {
    //         Some(invite) => invite,
    //         None => return Err(Error::NotFound),
    //     };
    //     let space = spaces
    //         .find_one(doc! {
    //             "id": invite.space_id,
    //         })
    //         .await?;
    //     let space = match space {
    //         Some(space) => space,
    //         None => return Err(Error::NotFound),
    //     };
    //     Ok(space)
    // }

    pub async fn get_channels(&self) -> Result<Vec<Channel>> {
        let channels = super::get_database().collection::<Channel>("channels");
        let channels = channels
            .find(doc! {
                "$or": [
                    {
                        "initiator_id": &self.id
                    },
                    {
                        "target_id": &self.id
                    },
                    {
                        "members": {
                            "$in": [&self.id],
                        }
                    }
                ]
            })
            .await?;
        let channels: Vec<Channel> = channels
            .filter_map(|channel| async { channel.ok() })
            .collect()
            .await;
        Ok(channels)
    }
}
