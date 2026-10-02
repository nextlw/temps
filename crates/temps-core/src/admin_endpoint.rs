// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Endereço que o **processo do control plane** usa para falar com um serviço
//! gerenciado (SQL de administração, navegador de dados, métricas, checagens).
//!
//! Os parâmetros de um serviço guardam `host=localhost` e a porta publicada no
//! host (`127.0.0.1:<porta>`). Isso só vale quando o control plane roda direto
//! no host. Quando ele roda em container (o `temps-app` do compose), `localhost`
//! é o próprio container do control plane: toda conexão com `localhost:<porta
//! publicada>` dá `Connection refused`. Nesse caso o serviço é alcançado pelo
//! nome do container dele, na porta interna, pela rede Docker que os dois
//! compartilham — o mesmo endereço que o deploy já injeta nos apps.
//!
//! A regra mora só aqui:
//!
//! 1. o control plane está em container (`TEMPS_EXECUTION_ENV=docker`,
//!    `/.dockerenv` ou `/run/.containerenv`) **e**
//! 2. o nome do container do serviço resolve a partir deste processo, com
//!    timeout curto
//!
//! → `<nome do container>:<porta interna>`. Em qualquer outro caso →
//! `config.host:config.port`, como antes.
//!
//! A resolução usa o nome absoluto (`<nome>.`) para que nenhum domínio de busca
//! do `resolv.conf` entre na conta: só o DNS da rede Docker responde por um nome
//! de uma etiqueta só. O nome só resolve enquanto o container existe e está numa
//! rede em comum, então um container recriado com outro nome é visto na próxima
//! chamada; não há cache de resultado, só do ambiente do processo.
//!
//! Quem conecta depois resolve o nome de novo. A escada TLS do Postgres
//! (`TransportPolicy::ManagedContainer` em `temps-query-postgres`) também usa a
//! forma absoluta. Os outros clientes (sqlx, redis, mongodb) recebem o nome sem
//! o ponto; dentro do container o `resolv.conf` do Docker traz `ndots:0`, então
//! o nome é consultado como está antes de qualquer domínio de busca e chega à
//! mesma resposta enquanto o container existir.
//!
//! Os nomes de container e as portas internas dos engines que o control plane
//! nomeia fora do próprio provider (navegador de dados, sondas, métricas) também
//! moram aqui, para que haja uma só derivação.
//!
//! Containers que rodam com `network_mode=host` (o de transferência do
//! `services populate`, por exemplo) estão na rede do host e continuam usando
//! `127.0.0.1:<porta publicada>`; eles não passam por aqui.

use std::sync::OnceLock;
use std::time::Duration;

/// Teto da resolução do nome do container. No host o gate de ambiente evita a
/// consulta; em container ela vai para o DNS embutido do Docker, que responde
/// em milissegundos.
pub const ADMIN_DNS_TIMEOUT: Duration = Duration::from_millis(500);

/// Porta do Postgres dentro do container do serviço.
pub const POSTGRES_INTERNAL_PORT: &str = "5432";
/// Porta do Redis dentro do container do serviço.
pub const REDIS_INTERNAL_PORT: &str = "6379";
/// Porta do MongoDB dentro do container do serviço.
pub const MONGODB_INTERNAL_PORT: &str = "27017";

fn managed_container_name(prefix: &str, service_name: &str, imported: Option<&str>) -> String {
    match imported.map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) => name.to_string(),
        None => format!("{prefix}{service_name}"),
    }
}

/// Container de um Postgres gerenciado: o nome real de um serviço importado ou
/// `postgres-{serviço}`.
pub fn postgres_container_name(service_name: &str, imported: Option<&str>) -> String {
    managed_container_name("postgres-", service_name, imported)
}

/// Container de um Redis gerenciado: o nome real de um serviço importado ou
/// `redis-{serviço}`.
pub fn redis_container_name(service_name: &str, imported: Option<&str>) -> String {
    managed_container_name("redis-", service_name, imported)
}

/// Container de um MongoDB gerenciado: o nome real de um serviço importado ou
/// `temps-mongodb-{serviço}`.
pub fn mongodb_container_name(service_name: &str, imported: Option<&str>) -> String {
    managed_container_name("temps-mongodb-", service_name, imported)
}

/// Por onde o control plane alcança o serviço.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminRoute {
    /// Nome do container e porta interna, pela rede Docker compartilhada.
    ContainerNetwork,
    /// `host`/`port` dos parâmetros do serviço (porta publicada no host).
    PublishedPort,
}

/// Endereço de administração escolhido para um serviço gerenciado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminEndpoint {
    pub host: String,
    pub port: String,
    pub route: AdminRoute,
}

impl AdminEndpoint {
    /// A porta como número, para os clientes que pedem `u16`.
    pub fn port_number(&self) -> Option<u16> {
        self.port.trim().parse().ok()
    }

    /// `true` quando o endereço é o nome do container na rede Docker.
    pub fn via_container_network(&self) -> bool {
        self.route == AdminRoute::ContainerNetwork
    }
}

/// Decide o endereço a partir do resultado da resolução. Pura, para teste.
pub fn choose_admin_endpoint(
    container_resolves: bool,
    container_name: &str,
    internal_port: &str,
    host: &str,
    port: &str,
) -> AdminEndpoint {
    if container_resolves {
        AdminEndpoint {
            host: container_name.to_string(),
            port: internal_port.to_string(),
            route: AdminRoute::ContainerNetwork,
        }
    } else {
        AdminEndpoint {
            host: host.to_string(),
            port: port.to_string(),
            route: AdminRoute::PublishedPort,
        }
    }
}

/// O processo do control plane roda dentro de um container?
///
/// `TEMPS_EXECUTION_ENV=docker` (o que o compose oficial define) ou os
/// marcadores que o Docker e o Podman criam na raiz do container. Os arquivos
/// são lidos uma vez por processo; a variável vem do contexto de runtime já
/// inicializado em [`crate::runtime`].
pub fn control_plane_in_container() -> bool {
    static CONTAINER_MARKER: OnceLock<bool> = OnceLock::new();
    let marker = *CONTAINER_MARKER.get_or_init(|| {
        std::path::Path::new("/.dockerenv").exists()
            || std::path::Path::new("/run/.containerenv").exists()
    });
    marker
        || crate::runtime::execution_environment_compatibility()
            == crate::runtime::ExecutionEnvironment::Docker
}

/// O nome resolve a partir deste processo dentro de `timeout`?
///
/// Consulta a forma absoluta (`<nome>.`) para não aplicar domínios de busca.
pub async fn container_name_resolves(container_name: &str, timeout: Duration) -> bool {
    let name = container_name.trim().trim_end_matches('.');
    if name.is_empty() {
        return false;
    }
    let absolute = format!("{name}.");
    let lookup = tokio::time::timeout(timeout, tokio::net::lookup_host((absolute, 0))).await;
    match lookup {
        Ok(Ok(mut addresses)) => addresses.next().is_some(),
        _ => false,
    }
}

/// Endereço de administração de um serviço gerenciado. Ver a doc do módulo.
pub async fn resolve_admin_endpoint(
    container_name: &str,
    internal_port: &str,
    host: &str,
    port: &str,
) -> AdminEndpoint {
    resolve_admin_endpoint_with(
        control_plane_in_container(),
        ADMIN_DNS_TIMEOUT,
        container_name,
        internal_port,
        host,
        port,
    )
    .await
}

/// [`resolve_admin_endpoint`] com o ambiente e o timeout explícitos.
pub async fn resolve_admin_endpoint_with(
    in_container: bool,
    timeout: Duration,
    container_name: &str,
    internal_port: &str,
    host: &str,
    port: &str,
) -> AdminEndpoint {
    let resolves = in_container && container_name_resolves(container_name, timeout).await;
    choose_admin_endpoint(resolves, container_name, internal_port, host, port)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nomes_de_container_seguem_o_provider_ou_o_importado() {
        assert_eq!(postgres_container_name("app", None), "postgres-app");
        assert_eq!(redis_container_name("cache", None), "redis-cache");
        assert_eq!(mongodb_container_name("docs", None), "temps-mongodb-docs");
        assert_eq!(
            postgres_container_name("app", Some("legacy-pg")),
            "legacy-pg"
        );
        assert_eq!(postgres_container_name("app", Some("  ")), "postgres-app");
    }

    #[test]
    fn nome_que_resolve_usa_container_e_porta_interna() {
        let endpoint = choose_admin_endpoint(true, "postgres-app-db", "5432", "localhost", "5433");
        assert_eq!(endpoint.host, "postgres-app-db");
        assert_eq!(endpoint.port, "5432");
        assert_eq!(endpoint.port_number(), Some(5432));
        assert!(endpoint.via_container_network());
    }

    #[test]
    fn nome_que_nao_resolve_usa_host_e_porta() {
        let endpoint = choose_admin_endpoint(false, "postgres-app-db", "5432", "localhost", "5433");
        assert_eq!(endpoint.host, "localhost");
        assert_eq!(endpoint.port, "5433");
        assert_eq!(endpoint.route, AdminRoute::PublishedPort);
    }

    /// `localhost` resolve em qualquer máquina: faz o papel do container vivo
    /// na rede compartilhada.
    #[tokio::test]
    async fn em_container_com_nome_resolvivel_vai_pela_rede_docker() {
        let endpoint = resolve_admin_endpoint_with(
            true,
            Duration::from_secs(2),
            "localhost",
            "5432",
            "127.0.0.1",
            "5433",
        )
        .await;
        assert_eq!(endpoint.host, "localhost");
        assert_eq!(endpoint.port, "5432");
        assert!(endpoint.via_container_network());
    }

    /// `.invalid` nunca resolve (RFC 6761): é o control plane no host, onde o
    /// nome do container não existe.
    #[tokio::test]
    async fn nome_inexistente_cai_no_host_e_porta() {
        let endpoint = resolve_admin_endpoint_with(
            true,
            Duration::from_secs(2),
            "postgres-nao-existe.invalid",
            "5432",
            "localhost",
            "5433",
        )
        .await;
        assert_eq!(endpoint.host, "localhost");
        assert_eq!(endpoint.port, "5433");
        assert_eq!(endpoint.route, AdminRoute::PublishedPort);
    }

    /// Fora de container nem consulta o DNS: o resultado é o de sempre, mesmo
    /// que o nome resolvesse.
    #[tokio::test]
    async fn fora_de_container_mantem_host_e_porta() {
        let endpoint = resolve_admin_endpoint_with(
            false,
            Duration::from_secs(2),
            "localhost",
            "5432",
            "localhost",
            "5433",
        )
        .await;
        assert_eq!(endpoint.port, "5433");
        assert_eq!(endpoint.route, AdminRoute::PublishedPort);
    }

    #[tokio::test]
    async fn nome_vazio_nao_resolve() {
        assert!(!container_name_resolves("", Duration::from_secs(1)).await);
        assert!(!container_name_resolves(" . ", Duration::from_secs(1)).await);
    }
}
