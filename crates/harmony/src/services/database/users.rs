use futures_util::StreamExt;
use mongodb::bson::{self, doc};
use serde::{Deserialize, Serialize};

use super::{channels::Channel, invites::Invite, spaces::Space};
use crate::errors::{Error, Result};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Status {
    Online = 0,
    Idle = 1,
    Busy = 2,
    Invisible = 3,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Presence {
    status: Status,
    message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Relationship {
    Friend = 0,
    Blocked = 1,
    Requested = 2,
    Pending = 3,
}

// TODO: allow disabling of friend requests
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Affinity {
    id: String,
    relationship: Relationship,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AffinityExtended {
    id: String,
    relationship: Relationship,
    user: User,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct User {
    pub id: String,
    pub profile_banner: Option<String>, // TODO: Make use of file handling
    pub profile_description: String,
    pub affinities: Vec<Affinity>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub online: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence: Option<Presence>,
}

impl User {
    pub async fn get_spaces(&self) -> Result<Vec<Space>> {
        let spaces = super::get_database().collection::<Space>("spaces");
        let spaces = spaces
            .find(doc! {
                "members": {
                    "$in": [&self.id],
                },
            })
            .await?;
        let mut spaces: Vec<Space> = spaces
            .filter_map(|space| async { space.ok() })
            .collect()
            .await;
        spaces.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(spaces)
    }

    pub async fn in_space(&self, space_id: &String) -> Result<bool> {
        let spaces = super::get_database().collection::<Space>("spaces");
        let space = spaces
            .find_one(doc! {
                "id": space_id,
                "members": {
                    "$in": [&self.id],
                },
            })
            .await?;
        Ok(space.is_some())
    }
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
                if members.contains(&self.id) {
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Channel::InformationChannel { space_id, .. } => self.in_space(space_id).await,
            Channel::AnnouncementChannel { space_id, .. } => self.in_space(space_id).await,
            Channel::StandardChannel { space_id, .. } => self.in_space(space_id).await,
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
            affinities: Vec::new(),
            online: None,
            presence: None,
        };
        users.insert_one(user.clone()).await?;
        Ok(user)
    }

    pub async fn add_friend(&self, friend_id: &String) -> Result<()> {
        let users = super::get_database().collection::<User>("users");
        User::get(friend_id).await?;
        let affinity = self.affinities.iter().find(|a| &a.id == friend_id);
        if let Some(affinity) = affinity {
            match affinity.relationship {
                Relationship::Friend => Err(Error::AlreadyFriends),
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
                                    "affinities.$[affinity].relationship": bson::to_bson(&Relationship::Friend)?
                                }
                            }).with_options(
                            Some(mongodb::options::UpdateOptions::builder()
                                .array_filters(vec![doc! {
                                    "affinity.id": &friend_id
                                }])
                                .build()),
                        )
                        .await?;
                    users
                        .update_one(
                            doc! {
                                "id": &friend_id
                            },
                            doc! {
                                "$set": {
                                    "affinities.$[affinity].relationship": bson::to_bson(&Relationship::Friend)?
                                }
                            }).with_options(
                            Some(mongodb::options::UpdateOptions::builder()
                                .array_filters(vec![doc! {
                                    "affinity.id": &self.id
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
                            "affinities": {
                                "id": friend_id,
                                "relationship": bson::to_bson(&Relationship::Requested)?
                            }
                        }
                    },
                )
                .await?;
            users
                .update_one(
                    doc! {
                        "id": &friend_id
                    },
                    doc! {
                        "$push": {
                            "affinities": {
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

    pub async fn remove_friend(&self, friend_id: &String) -> Result<()> {
        let users = super::get_database().collection::<User>("users");
        User::get(friend_id).await?;
        let affinity = self.affinities.iter().find(|a| &a.id == friend_id);
        if let Some(affinity) = affinity {
            match affinity.relationship {
                // remove friend
                Relationship::Friend => {
                    users
                        .update_one(
                            doc! {
                                "id": &self.id
                            },
                            doc! {
                                "$pull": {
                                    "affinities": {
                                        "id": friend_id
                                    }
                                }
                            },
                        )
                        .await?;
                    users
                        .update_one(
                            doc! {
                                "id": friend_id
                            },
                            doc! {
                                "$pull": {
                                    "affinities": {
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
                                    "affinities": {
                                        "id": friend_id
                                    }
                                }
                            },
                        )
                        .await?;
                    users
                        .update_one(
                            doc! {
                                "id": friend_id
                            },
                            doc! {
                                "$pull": {
                                    "affinities": {
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
                                    "affinities": {
                                        "id": friend_id
                                    }
                                }
                            },
                        )
                        .await?;
                    users
                        .update_one(
                            doc! {
                                "id": friend_id
                            },
                            doc! {
                                "$pull": {
                                    "affinities": {
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

    pub async fn get_friends(&self) -> Result<Vec<User>> {
        let users = super::get_database().collection::<User>("users");
        let friends = self
            .affinities
            .iter()
            .map(|affinity| async {
                if affinity.relationship == Relationship::Friend {
                    let user = users
                        .find_one(doc! {
                            "id": &affinity.id
                        })
                        .await.ok()?;
                    match user {
                        Some(user) => Some(user),
                        None => None,
                    }
                } else {
                    None
                }
            });
        let friends: Vec<User> = futures_util::future::join_all(friends)
            .await
            .iter()
            .filter_map(|friend| friend.clone())
            .collect();
        Ok(friends)
    }

    pub async fn get_affinities(&self) -> Result<Vec<AffinityExtended>> {
        let users = super::get_database().collection::<User>("users");
        let affinities = self
            .affinities
            .iter()
            .map(|affinity| async {
                let user = users
                    .find_one(doc! {
                        "id": &affinity.id
                    })
                    .await.ok()?;
                match user {
                    Some(user) => Some(AffinityExtended {
                        id: affinity.id.clone(),
                        relationship: affinity.relationship.clone(),
                        user,
                    }),
                    None => None,
                }
            });
        let affinities: Vec<AffinityExtended> = futures_util::future::join_all(affinities)
            .await
            .iter()
            .filter_map(|affinity| affinity.clone())
            .collect();
        Ok(affinities)
    }

    pub async fn accept_invite(&self, invite_code: &String) -> Result<Space> {
        let invites = super::get_database().collection::<Invite>("invites");
        let spaces = super::get_database().collection::<Space>("spaces");
        let invite = invites
            .find_one_and_update(
                doc! {
                    "id": invite_code,
                },
                doc! {
                    "$push": {
                        "uses": &self.id,
                    }
                },
            )
            .await?;
        let invite = match invite {
            Some(invite) => invite,
            None => return Err(Error::NotFound),
        };
        let space = spaces
            .find_one(doc! {
                "id": invite.space_id,
            })
            .await?;
        let space = match space {
            Some(space) => space,
            None => return Err(Error::NotFound),
        };
        Ok(space)
    }

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
