use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::types::InstanceUuid;
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, TS, Debug)]
#[ts(export)]
pub struct UserPermission {
    pub can_view_instance: HashSet<InstanceUuid>,
    pub can_start_instance: HashSet<InstanceUuid>,
    pub can_stop_instance: HashSet<InstanceUuid>,
    pub can_access_instance_console: HashSet<InstanceUuid>,
    pub can_access_instance_setting: HashSet<InstanceUuid>,
    pub can_read_instance_resource: HashSet<InstanceUuid>,
    // unsafe permission, owner exclusive unless explicitly granted
    pub can_write_instance_resource: HashSet<InstanceUuid>,
    // unsafe permission, owner exclusive unless explicitly granted
    pub can_access_instance_macro: HashSet<InstanceUuid>,
    pub can_read_instance_file: HashSet<InstanceUuid>,
    // unsafe permission, owner exclusive unless explicitly granted
    pub can_write_instance_file: HashSet<InstanceUuid>,

    pub can_create_instance: bool,
    pub can_delete_instance: bool,
    pub can_read_global_file: bool,
    // unsafe permission, owner exclusive unless explicitly granted
    pub can_write_global_file: bool,
    // owner exclusive unless explicitly granted
    pub can_manage_permission: bool,
    #[serde(default)]
    pub can_install_extension: bool,
}

impl UserPermission {
    pub fn new() -> Self {
        UserPermission {
            can_view_instance: HashSet::new(),
            can_start_instance: HashSet::new(),
            can_stop_instance: HashSet::new(),
            can_access_instance_console: HashSet::new(),
            can_access_instance_setting: HashSet::new(),
            can_read_instance_resource: HashSet::new(),
            can_write_instance_resource: HashSet::new(),
            can_access_instance_macro: HashSet::new(),
            can_read_instance_file: HashSet::new(),
            can_write_instance_file: HashSet::new(),
            can_create_instance: false,
            can_delete_instance: false,
            can_read_global_file: false,
            can_write_global_file: false,
            can_manage_permission: false,
            can_install_extension: false,
        }
    }

    /// Permissions that let a non-owner change the host or run code.
    pub fn has_unsafe(&self) -> bool {
        !self.can_write_instance_resource.is_empty()
            || !self.can_access_instance_macro.is_empty()
            || !self.can_write_instance_file.is_empty()
            || self.can_read_global_file
            || self.can_write_global_file
            || self.can_manage_permission
            || self.can_install_extension
    }

    /// Drop every grant that [`Self::has_unsafe`] reports.
    pub fn strip_unsafe(&mut self) {
        self.can_write_instance_resource.clear();
        self.can_access_instance_macro.clear();
        self.can_write_instance_file.clear();
        self.can_read_global_file = false;
        self.can_write_global_file = false;
        self.can_manage_permission = false;
        self.can_install_extension = false;
    }
}

impl Default for UserPermission {
    fn default() -> Self {
        Self::new()
    }
}
