// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Typed models for the Coolify REST API (`/api/v1`).
//!
//! Field sets were captured from a live Coolify v4 instance (July 2026) —
//! everything optional is `Option` with `serde(default)` so newer/older
//! Coolify versions that add or drop fields still deserialize.

use serde::Deserialize;

/// `GET /api/v1/servers` element
#[derive(Debug, Clone, Deserialize)]
pub struct CoolifyServer {
    pub uuid: String,
    pub name: String,
    #[serde(default)]
    pub ip: Option<String>,
}

/// `GET /api/v1/projects` element
#[derive(Debug, Clone, Deserialize)]
pub struct CoolifyProject {
    pub uuid: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// `GET /api/v1/projects/{uuid}` — includes the project's environments
#[derive(Debug, Clone, Deserialize)]
pub struct CoolifyProjectDetail {
    pub uuid: String,
    pub name: String,
    #[serde(default)]
    pub environments: Vec<CoolifyEnvironmentRef>,
}

/// Environment reference inside a project detail
#[derive(Debug, Clone, Deserialize)]
pub struct CoolifyEnvironmentRef {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub uuid: Option<String>,
}

/// `GET /api/v1/applications` element
#[derive(Debug, Clone, Deserialize)]
pub struct CoolifyApplication {
    pub uuid: String,
    pub name: String,
    #[serde(default)]
    pub environment_id: Option<i64>,
    #[serde(default)]
    pub git_repository: Option<String>,
    #[serde(default)]
    pub git_branch: Option<String>,
    #[serde(default)]
    pub build_pack: Option<String>,
    /// Set when the repository is cloned with an SSH deploy key.
    ///
    /// Null does NOT mean the repository is public: an app wired through
    /// Coolify's GitHub App integration clones a private repository with no
    /// deploy key at all. Reading this field as a visibility signal is what
    /// produced import plans that called private repositories public.
    #[serde(default)]
    pub private_key_id: Option<i64>,
    /// Full URL(s); Coolify separates multiple domains with commas
    #[serde(default)]
    pub fqdn: Option<String>,
    /// Exposed port(s) as a string, comma-separated (e.g. "3000" or "3000,9090")
    #[serde(default)]
    pub ports_exposes: Option<String>,
    #[serde(default)]
    pub docker_registry_image_name: Option<String>,
    #[serde(default)]
    pub docker_registry_image_tag: Option<String>,
    /// e.g. "running:unknown", "exited:unhealthy"
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub base_directory: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

impl CoolifyApplication {
    /// Whether this application deploys a prebuilt registry image
    /// (`build_pack == "dockerimage"`). For those, `git_repository` holds a
    /// placeholder value and must be ignored.
    pub fn is_image_based(&self) -> bool {
        self.build_pack.as_deref() == Some("dockerimage")
    }

    /// Full image reference for image-based applications
    pub fn image_reference(&self) -> Option<String> {
        let name = self.docker_registry_image_name.as_deref()?;
        if name.is_empty() {
            return None;
        }
        let tag = self
            .docker_registry_image_tag
            .as_deref()
            .filter(|t| !t.is_empty())
            .unwrap_or("latest");
        Some(format!("{}:{}", name, tag))
    }
}

/// `GET /api/v1/databases` element (standalone databases)
#[derive(Debug, Clone, Deserialize)]
pub struct CoolifyDatabase {
    pub uuid: String,
    pub name: String,
    /// e.g. "standalone-postgresql", "standalone-mysql", "standalone-redis"
    #[serde(default)]
    pub database_type: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub is_public: Option<bool>,
    #[serde(default)]
    pub public_port: Option<i32>,
    /// Connection URL on the instance's internal docker network
    #[serde(default)]
    pub internal_db_url: Option<String>,
    /// Connection URL reachable from outside (only meaningful when public)
    #[serde(default)]
    pub external_db_url: Option<String>,
    #[serde(default)]
    pub environment_id: Option<i64>,
    #[serde(default)]
    pub postgres_user: Option<String>,
    #[serde(default)]
    pub postgres_db: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

impl CoolifyDatabase {
    /// Connection URL usable from outside the source box, if any.
    pub fn reachable_url(&self) -> Option<&str> {
        if self.is_public.unwrap_or(false) {
            self.external_db_url.as_deref()
        } else {
            None
        }
    }

    /// Database version parsed from the image tag (e.g. "postgres:16-alpine" -> "16")
    pub fn version_from_image(&self) -> Option<String> {
        let image = self.image.as_deref()?;
        let tag = image.split(':').nth(1)?;
        let version: String = tag
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        (!version.is_empty()).then_some(version)
    }
}

/// `GET /api/v1/applications/{uuid}/envs` element
#[derive(Debug, Clone, Deserialize)]
pub struct CoolifyEnvVar {
    pub key: String,
    #[serde(default)]
    pub value: Option<String>,
    /// Resolved value (Coolify templates like `{{team.VAR}}` expanded)
    #[serde(default)]
    pub real_value: Option<String>,
    #[serde(default)]
    pub is_preview: bool,
    /// Coolify's "shown once" flag marks operator-designated secrets
    #[serde(default)]
    pub is_shown_once: bool,
    /// Set by Coolify itself (e.g. build instructions), not by the user
    #[serde(default)]
    pub is_coolify: bool,
}

impl CoolifyEnvVar {
    /// The value to migrate: prefer the resolved value, unquoted.
    pub fn effective_value(&self) -> &str {
        unwrap_quoted(
            self.real_value
                .as_deref()
                .or(self.value.as_deref())
                .unwrap_or(""),
        )
    }
}

/// Strip the quotes Coolify stores around a value.
///
/// Coolify keeps environment values in `.env` syntax, so the API hands back
/// `'production'` and `'postgres://user:pw@host/db'` — quotes included. Copied
/// verbatim they reach the container as part of the value, and the failure is
/// silent and misleading: a Go service reading a quoted `DATABASE_URL` does not
/// report a bad URL, it falls through to libpq's defaults and dies trying to
/// reach a unix socket as the wrong user. Numeric values happen to survive
/// because Coolify stores them bare, which makes the broken ones look arbitrary.
///
/// Only an actual quoted literal is unwrapped: the same quote character on both
/// ends, and no occurrence of it in between. A value that contains its own quote
/// is left exactly as it was — a passphrase ending in an apostrophe is not a
/// quoted literal, and mangling it would be worse than the quotes.
fn unwrap_quoted(raw: &str) -> &str {
    let bytes = raw.as_bytes();
    if bytes.len() < 2 {
        return raw;
    }

    let quote = bytes[0];
    if (quote != b'\'' && quote != b'"') || bytes[bytes.len() - 1] != quote {
        return raw;
    }

    // Both quotes are ASCII, so these indices are always char boundaries.
    let inner = &raw[1..raw.len() - 1];
    if inner.as_bytes().contains(&quote) {
        return raw;
    }

    inner
}

#[cfg(test)]
mod tests {
    use super::*;

    // Trimmed real responses captured from a live Coolify v4.x instance.
    const APP_FIXTURE: &str = r#"{"uuid":"aouhx3o71r5o38pvjw3gd24q","name":"lab-shop","environment_id":2,"git_repository":"heroku/node-js-getting-started","git_branch":"main","build_pack":"nixpacks","fqdn":"http://aouhx3o71r5o38pvjw3gd24q.167.233.223.170.sslip.io","ports_exposes":"5006","docker_registry_image_name":null,"docker_registry_image_tag":null,"status":"running:unknown","base_directory":"/","extra_field_from_future_version":true}"#;
    const IMAGE_APP_FIXTURE: &str = r#"{"uuid":"j81evy0of02xzeai6htmlyic","name":"lab-whoami","environment_id":2,"git_repository":"coollabsio/coolify","git_branch":"main","build_pack":"dockerimage","fqdn":"http://j81evy0of02xzeai6htmlyic.167.233.223.170.sslip.io","ports_exposes":"80","docker_registry_image_name":"traefik/whoami","docker_registry_image_tag":"latest","status":"running:unknown"}"#;
    const DB_FIXTURE: &str = r#"{"uuid":"e8xndfxu3it2lsuyug1hk6sd","name":"lab-db","database_type":"standalone-postgresql","image":"postgres:16-alpine","is_public":true,"public_port":5432,"internal_db_url":"postgres://postgres:pw@e8xndfxu3it2lsuyug1hk6sd:5432/postgres","external_db_url":"postgres://postgres:pw@167.233.223.170:5432/postgres","environment_id":2,"postgres_user":"postgres","postgres_db":"postgres"}"#;
    const ENV_FIXTURE: &str = r#"{"uuid":"y2ch741lj780w598jtu24dc1","comment":null,"is_buildtime":true,"is_coolify":false,"is_preview":false,"is_shown_once":false,"key":"NIXPACKS_NODE_VERSION","real_value":"22","value":"22","version":"4.1.2"}"#;
    const PROJECT_DETAIL_FIXTURE: &str = r#"{"uuid":"feyuvv97uak39yyfqbcx4n8n","name":"My first project","environments":[{"id":1,"name":"production","uuid":"sqjmx129h4y0kgpw3giem2ne"}]}"#;

    #[test]
    fn parses_git_application_fixture() {
        let app: CoolifyApplication = serde_json::from_str(APP_FIXTURE).unwrap();
        assert_eq!(app.uuid, "aouhx3o71r5o38pvjw3gd24q");
        assert_eq!(app.environment_id, Some(2));
        assert_eq!(
            app.git_repository.as_deref(),
            Some("heroku/node-js-getting-started")
        );
        assert!(!app.is_image_based());
        assert_eq!(app.image_reference(), None);
    }

    #[test]
    fn parses_image_application_and_ignores_placeholder_git() {
        let app: CoolifyApplication = serde_json::from_str(IMAGE_APP_FIXTURE).unwrap();
        assert!(app.is_image_based());
        assert_eq!(
            app.image_reference().as_deref(),
            Some("traefik/whoami:latest")
        );
    }

    #[test]
    fn parses_database_fixture_with_reachability_and_version() {
        let db: CoolifyDatabase = serde_json::from_str(DB_FIXTURE).unwrap();
        assert_eq!(
            db.reachable_url(),
            Some("postgres://postgres:pw@167.233.223.170:5432/postgres")
        );
        assert_eq!(db.version_from_image().as_deref(), Some("16"));
        assert_eq!(db.database_type.as_deref(), Some("standalone-postgresql"));
    }

    #[test]
    fn private_database_has_no_reachable_url() {
        let mut db: CoolifyDatabase = serde_json::from_str(DB_FIXTURE).unwrap();
        db.is_public = Some(false);
        assert_eq!(db.reachable_url(), None);
    }

    #[test]
    fn parses_env_var_fixture() {
        let env: CoolifyEnvVar = serde_json::from_str(ENV_FIXTURE).unwrap();
        assert_eq!(env.key, "NIXPACKS_NODE_VERSION");
        assert_eq!(env.effective_value(), "22");
        assert!(!env.is_shown_once);
    }

    /// The bug this fixes, as it actually arrived: Coolify hands back `.env`
    /// syntax, so the value carries its own quotes. Copied verbatim they became
    /// part of the value inside the container, and a Go service reading a quoted
    /// `DATABASE_URL` did not report a bad URL — it fell through to libpq's
    /// defaults and died reaching for a unix socket as the wrong user.
    #[test]
    fn a_quoted_value_arrives_unquoted() {
        let quoted = |raw: &str| CoolifyEnvVar {
            key: "K".to_string(),
            value: Some(raw.to_string()),
            real_value: None,
            is_preview: false,
            is_shown_once: false,
            is_coolify: false,
        };

        assert_eq!(quoted("'production'").effective_value(), "production");
        assert_eq!(
            quoted("'postgres://crm:pw@db:5432/crm'").effective_value(),
            "postgres://crm:pw@db:5432/crm"
        );
        assert_eq!(quoted("\"15m\"").effective_value(), "15m");
        // Bare values were never broken and must stay untouched, which is why
        // the numeric ones survived and made the failures look arbitrary.
        assert_eq!(quoted("8080").effective_value(), "8080");
    }

    /// Only a real quoted literal is unwrapped. A value containing its own
    /// quote is not one, and stripping its ends would silently corrupt a
    /// credential — a worse outcome than leaving the quotes on.
    #[test]
    fn a_value_holding_its_own_quote_is_left_alone() {
        let raw = |v: &str| CoolifyEnvVar {
            key: "K".to_string(),
            value: Some(v.to_string()),
            real_value: None,
            is_preview: false,
            is_shown_once: false,
            is_coolify: false,
        };

        // Opens and closes with a quote, but holds one too: not a literal.
        assert_eq!(raw("'it's'").effective_value(), "'it's'");
        // Mismatched ends.
        assert_eq!(raw("'mixed\"").effective_value(), "'mixed\"");
        // Only one end quoted.
        assert_eq!(raw("'half").effective_value(), "'half");
        assert_eq!(raw("half'").effective_value(), "half'");
        // Degenerate inputs must not panic or over-trim.
        assert_eq!(raw("'").effective_value(), "'");
        assert_eq!(raw("''").effective_value(), "");
        assert_eq!(raw("").effective_value(), "");
    }

    /// Unwrapping happens after the resolved value wins, so a templated value
    /// Coolify expanded is unquoted too rather than only the raw one.
    #[test]
    fn the_resolved_value_is_unquoted_as_well() {
        let env = CoolifyEnvVar {
            key: "K".to_string(),
            value: Some("'{{team.DB}}'".to_string()),
            real_value: Some("'postgres://resolved'".to_string()),
            is_preview: false,
            is_shown_once: false,
            is_coolify: false,
        };
        assert_eq!(env.effective_value(), "postgres://resolved");
    }

    /// Multi-byte content must survive: the quotes are ASCII, but what they
    /// wrap need not be, and slicing on the wrong boundary would panic.
    #[test]
    fn a_quoted_value_with_multibyte_content_is_unwrapped_safely() {
        let env = CoolifyEnvVar {
            key: "K".to_string(),
            value: Some("'produção — ção'".to_string()),
            real_value: None,
            is_preview: false,
            is_shown_once: false,
            is_coolify: false,
        };
        assert_eq!(env.effective_value(), "produção — ção");
    }

    #[test]
    fn parses_project_detail_fixture() {
        let project: CoolifyProjectDetail = serde_json::from_str(PROJECT_DETAIL_FIXTURE).unwrap();
        assert_eq!(project.environments.len(), 1);
        assert_eq!(project.environments[0].id, 1);
        assert_eq!(project.environments[0].name, "production");
    }
}
