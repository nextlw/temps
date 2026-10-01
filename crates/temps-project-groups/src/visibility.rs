// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Who sees which group, as pure functions (ADR-049, DF2-3).
//!
//! A group has no access rules of its own: what a caller sees is derived
//! from the projects they can reach through the platform's
//! `ProjectAccessChecker`. Keeping the rule here, away from the handlers,
//! means every endpoint applies the same one and it is tested without a
//! database or a checker.

use std::collections::BTreeSet;

use crate::service::ProjectGroupWithMembers;

/// Which projects the caller may reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceAccess {
    /// Instance administrators, and instances with no access checker
    /// registered: nothing is hidden.
    Unrestricted,
    /// Only these project ids are reachable; every other member is hidden.
    Only(BTreeSet<i32>),
}

impl ServiceAccess {
    pub fn can_access(&self, project_id: i32) -> bool {
        match self {
            Self::Unrestricted => true,
            Self::Only(allowed) => allowed.contains(&project_id),
        }
    }
}

/// The group as this caller may see it, or `None` when it must not appear.
///
/// A group whose members are *all* hidden disappears (its name would
/// otherwise reveal that something the caller cannot see exists); a group
/// with no members at all stays visible. Hidden ids are removed so neither
/// `service_ids` nor `service_count` leaks them.
pub fn visible_group(
    mut group: ProjectGroupWithMembers,
    access: &ServiceAccess,
) -> Option<ProjectGroupWithMembers> {
    if group.project_ids.is_empty() {
        return Some(group);
    }
    group.project_ids.retain(|id| access.can_access(*id));
    if group.project_ids.is_empty() {
        None
    } else {
        Some(group)
    }
}

/// [`visible_group`] over a listing, keeping the input order.
pub fn visible_groups(
    groups: Vec<ProjectGroupWithMembers>,
    access: &ServiceAccess,
) -> Vec<ProjectGroupWithMembers> {
    groups
        .into_iter()
        .filter_map(|group| visible_group(group, access))
        .collect()
}

/// Whether the caller may rename, edit or delete the group.
///
/// Only when no member is hidden: those changes affect every service in the
/// group, including ones the caller is gated out of. Must be evaluated on
/// the unfiltered group, never on the output of [`visible_group`].
pub fn can_manage(group: &ProjectGroupWithMembers, access: &ServiceAccess) -> bool {
    group.project_ids.iter().all(|id| access.can_access(*id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use temps_entities::project_groups;

    fn group(id: i32, project_ids: Vec<i32>) -> ProjectGroupWithMembers {
        let now = Utc::now();
        ProjectGroupWithMembers {
            group: project_groups::Model {
                id,
                name: format!("Group {id}"),
                slug: format!("group-{id}"),
                description: None,
                created_at: now,
                updated_at: now,
            },
            project_ids,
        }
    }

    fn only(ids: &[i32]) -> ServiceAccess {
        ServiceAccess::Only(ids.iter().copied().collect())
    }

    #[test]
    fn unrestricted_sees_everything_and_manages_everything() {
        let groups = vec![group(1, vec![1, 2]), group(2, vec![])];
        let visible = visible_groups(groups.clone(), &ServiceAccess::Unrestricted);
        assert_eq!(visible, groups);
        assert!(groups
            .iter()
            .all(|g| can_manage(g, &ServiceAccess::Unrestricted)));
    }

    #[test]
    fn group_with_every_member_hidden_disappears() {
        assert_eq!(visible_group(group(1, vec![5, 6]), &only(&[7])), None);
    }

    #[test]
    fn empty_group_stays_visible_to_a_restricted_caller() {
        let visible = visible_group(group(1, vec![]), &only(&[]));
        assert_eq!(visible.map(|g| g.group.id), Some(1));
    }

    #[test]
    fn hidden_members_are_removed_from_ids_and_count() {
        let visible = visible_group(group(1, vec![3, 4, 9]), &only(&[4, 9, 12]))
            .expect("one member is reachable");
        assert_eq!(visible.project_ids, vec![4, 9]);
    }

    #[test]
    fn listing_keeps_order_and_drops_only_fully_hidden_groups() {
        let groups = vec![group(1, vec![1]), group(2, vec![2]), group(3, vec![])];
        let ids: Vec<i32> = visible_groups(groups, &only(&[2]))
            .into_iter()
            .map(|g| g.group.id)
            .collect();
        assert_eq!(ids, vec![2, 3]);
    }

    #[test]
    fn a_single_hidden_member_blocks_management() {
        let g = group(1, vec![1, 2]);
        assert!(!can_manage(&g, &only(&[1])));
        assert!(can_manage(&g, &only(&[1, 2])));
        assert!(can_manage(&group(2, vec![]), &only(&[])));
    }
}
