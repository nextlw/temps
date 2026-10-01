// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Project groups (ADR-049): a named group of projects.
//!
//! # Naming
//!
//! The console calls a group a **Project** and the projects inside it
//! **Services** (ADR-048). Code, SQL, API and audit names keep
//! `project_group` / `project`, because "service" already means external
//! services, KV/Blob, compose services and OTel services across the
//! platform, and renaming the API would collide with upstream on every
//! merge.
//!
//! # Access
//!
//! A group carries no access rules of its own. What a caller sees is
//! derived from the projects the platform's `ProjectAccessChecker` lets
//! them reach — see [`visibility`].

pub mod error;
pub mod handlers;
pub mod plugin;
pub mod service;
pub mod visibility;

pub use error::ProjectGroupError;
pub use handlers::{ProjectGroupResponse, ProjectGroupsApiDoc, ProjectGroupsAppState};
pub use plugin::ProjectGroupsPlugin;
pub use service::{
    AssignOutcome, CreateProjectGroupRequest, ProjectGroupService, ProjectGroupWithMembers,
    UpdateProjectGroupRequest, UpdatedFields,
};
pub use visibility::{can_manage, visible_group, visible_groups, ServiceAccess};
