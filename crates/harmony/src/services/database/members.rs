use std::collections::HashMap;

use mongodb::bson::doc;
use serde::{Deserialize, Serialize};

use crate::{
    errors::Result,
    services::permissions::{Permission, PermissionSet},
};

use super::{
    channels::{Channel, EntityType},
    roles::Role,
    spaces::Space,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Member {
    pub id: String,
    pub space_id: String,
    pub roles: Vec<String>,
}

impl Member {
    pub async fn get_permissions(&self) -> Result<PermissionSet> {
        let space = Space::get(&self.space_id).await?;
        if space.owner == self.id {
            Ok(PermissionSet::all())
        } else {
            let mut calculated_permissions = PermissionSet::from(space.base_permissions);
            let roles_sorted = self.roles.clone();
            let futures = roles_sorted.iter().map(Role::get);
            let mut roles = futures_util::future::try_join_all(futures).await?;
            roles.sort_by(|a, b| a.position.cmp(&b.position));
            roles.reverse();
            let default = calculated_permissions.to_vec();
            for role in roles {
                let role_permissions: PermissionSet = role.permissions.into();
                if role_permissions.has_permission(Permission::Administrator) {
                    calculated_permissions = PermissionSet::all();
                    break;
                }
                for permission in &default {
                    if !role_permissions.has_permission(*permission) {
                        calculated_permissions.remove_permission(*permission);
                    }
                }
                calculated_permissions.combine(role_permissions);
            }
            Ok(calculated_permissions)
        }
    }

    // PermissionOverrideState (0, 1, 2)
    pub async fn get_permission_in_channel(
        &self,
        channel: &Channel,
        permission: Permission,
    ) -> Result<bool> {
        let space = Space::get(&self.space_id).await?;
        if space.owner == self.id {
            return Ok(true);
        }
        let mut has_permission = false;
        // TODO: Do we really need to order the roles?
        let permissions = self.get_permissions().await?;
        if permissions.has_permission(permission) {
            has_permission = true;
        }
        match channel {
            Channel::InformationChannel { permissions, .. }
            | Channel::AnnouncementChannel { permissions, .. }
            | Channel::StandardChannel { permissions, .. } => {
                let mut role_overrides = permissions
                    .iter()
                    .filter(|p| p.entity_type == EntityType::Role)
                    .filter(|p| self.roles.contains(&p.id))
                    .collect::<Vec<_>>();
                let mut map = HashMap::new();
                for role in &role_overrides {
                    map.insert(role.id.clone(), Role::get(&role.id).await?);
                }
                role_overrides.sort_by(|a, b| {
                    map.get(&a.id)
                        .unwrap()
                        .position
                        .cmp(&map.get(&b.id).unwrap().position)
                });
                role_overrides.reverse();
                for role_override in role_overrides {
                    if role_override.allow.has_permission(permission) {
                        has_permission = true;
                    }
                    if role_override.deny.has_permission(permission) {
                        has_permission = false;
                    }
                }

                let member_override = permissions
                    .iter()
                    .find(|p| p.id == self.id && p.entity_type == EntityType::Member);
                if let Some(member_override) = member_override {
                    if member_override.allow.has_permission(permission) {
                        has_permission = true;
                    }
                    if member_override.deny.has_permission(permission) {
                        has_permission = false;
                    }
                }

                Ok(has_permission)
            }
            _ => Ok(false), // FIXME: Need to handle private channels
        }
    }

    pub async fn is_owner(&self) -> Result<bool> {
        let space = Space::get(&self.space_id).await?;
        Ok(space.owner == self.id)
    }

    pub async fn get(id: &String, space_id: &String) -> Result<Member> {
        let database = super::get_database();
        let member = database
            .collection::<Member>("members")
            .find_one(doc! {
                "id": id,
                "space_id": space_id,
            })
            .await?
            .ok_or(crate::errors::Error::NotFound)?;
        Ok(member)
    }

    pub async fn delete(&self, space_id: &String) -> Result<()> {
        let database = super::get_database();
        database
            .collection::<Member>("members")
            .delete_one(doc! {
                "id": self.id.clone(),
                "space_id": space_id
            })
            .await?;
        Ok(())
    }
}
