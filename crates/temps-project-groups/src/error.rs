// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use axum::http::StatusCode;
use temps_core::problemdetails::{self, Problem};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProjectGroupError {
    #[error("Project group {group_id} not found")]
    NotFound { group_id: i32 },

    #[error("Project group '{slug}' not found")]
    NotFoundBySlug { slug: String },

    #[error("Project {project_id} not found")]
    ProjectNotFound { project_id: i32 },

    #[error("Project {project_id} is not in project group {group_id}")]
    NotAMember { group_id: i32, project_id: i32 },

    #[error("Project group slug '{slug}' is already taken")]
    SlugConflict { slug: String },

    #[error("Validation error: {message}")]
    Validation { message: String },

    /// `context` names the operation, so the log line says what failed
    /// without a backtrace.
    #[error("Database error while {context}: {source}")]
    Database {
        context: &'static str,
        #[source]
        source: sea_orm::DbErr,
    },
}

impl ProjectGroupError {
    /// `map_err` adapter that attaches what the service was doing.
    pub(crate) fn db(context: &'static str) -> impl FnOnce(sea_orm::DbErr) -> Self {
        move |source| Self::Database { context, source }
    }
}

impl From<ProjectGroupError> for Problem {
    fn from(error: ProjectGroupError) -> Self {
        match error {
            ProjectGroupError::NotFound { .. }
            | ProjectGroupError::NotFoundBySlug { .. }
            | ProjectGroupError::ProjectNotFound { .. }
            | ProjectGroupError::NotAMember { .. } => problemdetails::new(StatusCode::NOT_FOUND)
                .with_title("Resource Not Found")
                .with_detail(error.to_string()),

            ProjectGroupError::Validation { .. } => problemdetails::new(StatusCode::BAD_REQUEST)
                .with_title("Validation Error")
                .with_detail(error.to_string()),

            ProjectGroupError::SlugConflict { .. } => problemdetails::new(StatusCode::CONFLICT)
                .with_title("Resource Conflict")
                .with_detail(error.to_string()),

            // The SQL detail goes to the log, not the response: the caller
            // can do nothing with it and it describes the schema.
            ProjectGroupError::Database { .. } => {
                tracing::error!(error = %error, "project groups: database failure");
                problemdetails::new(StatusCode::INTERNAL_SERVER_ERROR)
                    .with_title("Internal Server Error")
                    .with_detail("A database error occurred; please try again")
            }
        }
    }
}
