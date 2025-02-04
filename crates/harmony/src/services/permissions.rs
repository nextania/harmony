use serde::{Deserialize, Serialize};

use super::database::{members::Member, roles::Role, spaces::Space};
use crate::errors::Result;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[repr(i64)]
pub enum Permission {
    Administrator = 0x1,             // 1 << 0
    ViewChannels = 0x2,              // 1 << 1
    CreateInvite = 0x4,              // 1 << 2
    SuspendMembers = 0x8,            // 1 << 3
    KickMembers = 0x10,              // 1 << 4
    BanMembers = 0x20,               // 1 << 5
    ManageChannels = 0x40,           // 1 << 6
    ManageChannelPermissions = 0x80, // 1 << 7
    ManageInvites = 0x100,           // 1 << 8
    ManageRoles = 0x200,             // 1 << 9
    ManageSpace = 0x400,             // 1 << 10
    SendMessages = 0x800,            // 1 << 11
    SendMultimediaMessages = 0x1000, // 1 << 12
    EmbedMessages = 0x2000,          // 1 << 13
    ManageMessages = 0x4000,         // 1 << 14
    MentionAll = 0x8000,             // 1 << 15
    UseReactions = 0x10000,          // 1 << 16
    StartCalls = 0x20000,            // 1 << 17
    JoinCalls = 0x40000,             // 1 << 18
    ManageCalls = 0x80000,           // 1 << 19
    Speak = 0x100000,                // 1 << 20
    Video = 0x200000,                // 1 << 21
    Screenshare = 0x400000,          // 1 << 22
}

#[derive(Clone, Debug)]
pub struct PermissionSet {
    permissions: i64,
}

impl Permission {
    pub fn iter() -> impl Iterator<Item = Self> {
        [
            Permission::Administrator,
            Permission::ViewChannels,
            Permission::CreateInvite,
            Permission::SuspendMembers,
            Permission::KickMembers,
            Permission::BanMembers,
            Permission::ManageChannels,
            Permission::ManageChannelPermissions,
            Permission::ManageInvites,
            Permission::ManageRoles,
            Permission::ManageSpace,
            Permission::SendMessages,
            Permission::SendMultimediaMessages,
            Permission::EmbedMessages,
            Permission::ManageMessages,
            Permission::MentionAll,
            Permission::UseReactions,
            Permission::StartCalls,
            Permission::JoinCalls,
            Permission::ManageCalls,
            Permission::Speak,
            Permission::Video,
            Permission::Screenshare,
        ]
        .iter()
        .copied()
    }

    pub fn iter_channel() -> impl Iterator<Item = Self> {
        [
            Permission::ViewChannels,
            Permission::ManageChannels,
            Permission::ManageChannelPermissions,
            Permission::SendMessages,
            Permission::SendMultimediaMessages,
            Permission::EmbedMessages,
            Permission::ManageMessages,
            Permission::MentionAll,
            Permission::UseReactions,
            Permission::StartCalls,
            Permission::JoinCalls,
            Permission::ManageCalls,
            Permission::Speak,
            Permission::Video,
            Permission::Screenshare,
        ]
        .iter()
        .copied()
    }
}

impl Serialize for PermissionSet {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_i64(self.permissions)
    }
}

impl<'de> Deserialize<'de> for PermissionSet {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let permissions = i64::deserialize(deserializer)?;
        Ok(PermissionSet { permissions })
    }
}

impl PermissionSet {
    pub fn new() -> Self {
        Self { permissions: 0 }
    }

    pub fn all() -> Self {
        Self {
            permissions: i64::MAX,
        }
    }

    pub fn to_i64(&self) -> i64 {
        self.permissions
    }

    pub fn to_vec(&self) -> Vec<Permission> {
        let mut permissions = Vec::new();
        for permission in Permission::iter() {
            if self.has_permission(permission) {
                permissions.push(permission);
            }
        }
        permissions
    }

    pub fn has_permission(&self, permission: Permission) -> bool {
        self.permissions & permission as i64 != 0
    }

    pub fn add_permission(&mut self, permission: Permission) {
        self.permissions |= permission as i64;
    }

    pub fn remove_permission(&mut self, permission: Permission) {
        self.permissions &= !(permission as i64);
    }

    pub fn combine(&mut self, other: PermissionSet) {
        self.permissions |= other.permissions;
    }
}

impl Default for PermissionSet {
    fn default() -> Self {
        Self::new()
    }
}

impl From<i64> for PermissionSet {
    fn from(permissions: i64) -> Self {
        Self { permissions }
    }
}

pub async fn can_modify_role(member: &Member, role: &Role) -> Result<bool> {
    let space = Space::get(&member.space_id).await?;
    if space.owner == member.id {
        return Ok(true);
    }
    let member_roles = member.roles.clone();
    let futures = member_roles.iter().map(Role::get);
    let mut roles = futures_util::future::try_join_all(futures).await?;
    roles.sort_by(|a, b| a.position.cmp(&b.position));
    roles.reverse();
    let permissions = member.get_permissions().await?;
    if !permissions.has_permission(Permission::ManageRoles) {
        return Ok(false);
    }
    if role.position < roles[0].position {
        return Ok(true);
    }
    Ok(false)
}
