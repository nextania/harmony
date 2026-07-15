use core_api::{AvatarUrl, PublicUser};
use harmony_types::users::{Presence, UserProfile};

#[derive(Clone)]
pub struct User {
    base: PublicUser,
    profile: UserProfile,
}

impl User {
    pub(crate) fn new(base: PublicUser, profile: UserProfile) -> Self {
        Self { base, profile }
    }

    pub fn id(&self) -> &str {
        &self.base.id
    }

    pub fn username(&self) -> &str {
        &self.base.username
    }

    pub fn display_name(&self) -> &str {
        &self.base.display_name
    }

    pub fn description(&self) -> &str {
        &self.base.description
    }

    pub fn avatar(&self) -> Option<&AvatarUrl> {
        self.base.avatar.as_ref()
    }

    pub fn presence(&self) -> Option<&Presence> {
        self.profile.presence.as_ref()
    }

    pub fn base(&self) -> &PublicUser {
        &self.base
    }

    pub fn profile(&self) -> &UserProfile {
        &self.profile
    }
}
