// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Plugin entrypoint for project groups (ADR-049).
//!
//! Emits no plugin events (DF2-5): the `project.*` event contracts that
//! external plugins and webhooks consume stay exactly as they were. The
//! audit log is the only trail of group changes.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use sea_orm::DatabaseConnection;
use temps_core::plugin::{
    PluginContext, PluginError, PluginRoutes, ServiceRegistrationContext, TempsPlugin,
};
use utoipa::openapi::OpenApi;
use utoipa::OpenApi as OpenApiTrait;

use crate::handlers::{router, ProjectGroupsApiDoc, ProjectGroupsAppState};
use crate::service::ProjectGroupService;

pub struct ProjectGroupsPlugin;

impl ProjectGroupsPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ProjectGroupsPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl TempsPlugin for ProjectGroupsPlugin {
    fn name(&self) -> &'static str {
        "project-groups"
    }

    fn register_services<'a>(
        &'a self,
        context: &'a ServiceRegistrationContext,
    ) -> Pin<Box<dyn Future<Output = Result<(), PluginError>> + Send + 'a>> {
        Box::pin(async move {
            let db = context.require_service::<DatabaseConnection>();
            context.register_service(Arc::new(ProjectGroupService::new(db)));
            tracing::debug!("project-groups: services registered");
            Ok(())
        })
    }

    fn configure_routes(&self, context: &PluginContext) -> Option<PluginRoutes> {
        // Resolved here, not in `register_services`: the checker is
        // registered by another plugin (teams), and only `configure_routes`
        // runs after every plugin has registered its services, whatever the
        // registration order.
        let state = Arc::new(ProjectGroupsAppState {
            service: context.require_service::<ProjectGroupService>(),
            audit: context.require_service::<dyn temps_core::AuditLogger>(),
            project_access_checker: context.get_service::<dyn temps_core::ProjectAccessChecker>(),
        });
        Some(PluginRoutes::new(router(state)))
    }

    fn openapi_schema(&self) -> Option<OpenApi> {
        Some(<ProjectGroupsApiDoc as OpenApiTrait>::openapi())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_name_is_stable() {
        assert_eq!(ProjectGroupsPlugin::new().name(), "project-groups");
    }
}
