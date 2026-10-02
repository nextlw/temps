// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use async_trait::async_trait;
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue::Set, ConnectionTrait, DbErr};
use serde::{Deserialize, Serialize};
use temps_core::DBDateTime;

/// One copy of an external PostgreSQL database into a database of an existing
/// managed PostgreSQL service ("populate").
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "service_populate_runs")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// The managed PostgreSQL service that owns the destination database.
    pub service_id: i32,
    /// Destination database inside the service.
    pub database_name: String,
    /// Source URL with user and password replaced by `***`. The real URL is
    /// never persisted.
    pub source_url_masked: String,
    /// Whether the destination database was dropped and recreated first.
    pub replace_existing: bool,
    /// Whether a replace was allowed to terminate the destination's sessions.
    pub disconnect_clients: bool,
    /// "running" | "completed" | "failed"
    pub status: String,
    /// Client image the transfer container ran (`postgres:<major>-alpine`).
    pub client_image: String,
    pub error_message: Option<String>,
    /// `pg_database_size` of the destination after a successful copy.
    pub database_size_bytes: Option<i64>,
    pub started_at: DBDateTime,
    pub finished_at: Option<DBDateTime>,
    pub created_by: Option<i32>,
    pub created_at: DBDateTime,
    pub updated_at: DBDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::external_services::Entity",
        from = "Column::ServiceId",
        to = "super::external_services::Column::Id",
        on_delete = "Cascade"
    )]
    Service,
    #[sea_orm(
        belongs_to = "super::users::Entity",
        from = "Column::CreatedBy",
        to = "super::users::Column::Id",
        on_delete = "SetNull"
    )]
    CreatedBy,
}

impl Related<super::external_services::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Service.def()
    }
}

#[async_trait]
impl ActiveModelBehavior for ActiveModel {
    async fn before_save<C>(mut self, _db: &C, insert: bool) -> Result<Self, DbErr>
    where
        C: ConnectionTrait,
    {
        let now = chrono::Utc::now();
        if insert && self.created_at.is_not_set() {
            self.created_at = Set(now);
        }
        self.updated_at = Set(now);
        Ok(self)
    }
}
