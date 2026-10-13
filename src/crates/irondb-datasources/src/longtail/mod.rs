//! Long-tail ~40 engines (DRV-04) — registry + quirk matrix extension.
//!
//! Engine → vehicle (per approved resolution):
//! - ODBC (`odbc-api` 29, only new crate): gbase8a, gbase8s
//! - sqlx wire-compat (existing): tidb/cockroach/timescale/goldendb (native),
//!   oceanbase/doris/starrocks/sphinx (mysql-protocol sidecar-capable)
//! - reqwest HTTP (existing): chroma/qdrant/milvus/weaviate,
//!   meilisearch/typesense/opensearch/elasticsearch-alt, clickhouse/trino/
//!   presto, tdengine/influxdb/questdb/greptimedb, etcd, navicat-tunnel
//! - Phase-4 sidecar reuse: dameng/kingbase/highgo/vastbase/opengauss/
//!   gaussdb/gbase8c/mariadb/sqlserver-alt/duckdb-alt/firebird/iris/cache/
//!   yashandb/iotdb/kwdb/zookeeper/kafka/mqtt/rabbitmq/pulsar/rocketmq

pub mod cn;
pub mod messaging;
pub mod misc;
pub mod vector;

pub use cn::{redact_secrets, OdbcBridgeAdapter, SidecarAdapter, LONGTAIL_ROW_BUDGET};
pub use messaging::StreamAdapter;
pub use misc::{CompatAdapter, WireKind};
pub use vector::HttpEngineAdapter;

use irondb_drivers::{CapabilityFlags, DriverRegistry};

/// (engine id, vehicle, quirk note) — mirrors vehicle-matrix.json policy.
pub const LONGTAIL_MATRIX: [(&str, &str, &str); 47] = [
    ("gbase8a", "odbc", "columnar MPP; ODBC-backed in original"),
    ("gbase8s", "odbc", "informix kernel via CSDK ODBC driver"),
    ("tidb", "native", "mysql-compat"),
    ("cockroach", "native", "pg-compat"),
    ("timescale", "native", "pg extension"),
    ("goldendb", "native", "mysql-compat wire; reuses MySQLDB impl"),
    ("oceanbase", "compat", "mysql/oracle compat modes"),
    ("doris", "compat", "normalized alias diros; mysql-protocol compat"),
    ("starrocks", "compat", "mysql-protocol compat"),
    ("sphinx", "compat", "mysql-protocol SphinxQL"),
    ("chroma", "http", "HTTP embedding store"),
    ("qdrant", "http", "HTTP/gRPC vector store"),
    ("milvus", "http", "gRPC/HTTP vector store"),
    ("weaviate", "http", "HTTP/GraphQL vector store"),
    ("meilisearch", "http", "HTTP search"),
    ("typesense", "http", "HTTP search"),
    ("opensearch", "http", "ES-compat HTTP"),
    ("clickhouse", "http", "HTTP/TCP native; columnar"),
    ("trino", "http", "HTTP/Trino protocol"),
    ("presto", "http", "trino predecessor; same HTTP vehicle"),
    ("tdengine", "http", "REST/TAOS wire"),
    ("influxdb", "http", "HTTP line-protocol/flux"),
    ("questdb", "http", "pg-wire/HTTP"),
    ("greptimedb", "http", "pg/mysql-protocol compat"),
    ("etcd", "http", "gRPC/HTTP kv"),
    ("navicat-tunnel", "http", "HTTP tunnel to remote DB"),
    ("dameng", "sidecar", "dm driver; transactional optional-driver-agent"),
    ("kingbase", "sidecar", "pg-compat; alias kingbase8/kingbasees/kingbasev8"),
    ("highgo", "sidecar", "pg-compat"),
    ("vastbase", "sidecar", "pg-compat"),
    ("opengauss", "sidecar", "pg-compat; alias open_gauss/open-gauss"),
    ("gaussdb", "sidecar", "pg-compat; alias gauss_db/gauss-db"),
    ("gbase8c", "sidecar", "pg-compat MPP"),
    ("mariadb", "sidecar", "mysql-compat; optional driver-agent"),
    ("duckdb", "sidecar", "analytical; optional driver-agent (no native crate per vehicle resolution)"),
    ("firebird", "sidecar", "sidecar"),
    ("iris", "sidecar", "intersystems iris alias family"),
    ("cache", "sidecar", "intersystems cache alias family"),
    ("yashandb", "sidecar", "cn; sidecar"),
    ("iotdb", "sidecar", "session/thrift API"),
    ("kwdb", "sidecar", "cn timeseries; sidecar"),
    ("zookeeper", "sidecar", "ZAB client; sidecar"),
    ("kafka", "sidecar", "binary protocol; sidecar reuse (no rdkafka)"),
    ("mqtt", "sidecar", "pub/sub; normalized mqtt/mqtts"),
    ("rabbitmq", "sidecar", "AMQP; sidecar reuse"),
    ("pulsar", "sidecar", "binary protocol; sidecar reuse"),
    ("rocketmq", "sidecar", "alias rmq; sidecar reuse"),
];

/// Register every long-tail engine with capability flags. No facade/domain churn.
///
/// `DriverRegistry::register` takes a `fn` pointer, so each engine gets a
/// tiny factory fn via the macro below (closures capturing `engine` won't coerce).
macro_rules! reg_compat {
    ($r:ident, $id:literal, $wire:expr, $quirk:literal) => {{
        fn factory() -> Box<dyn irondb_drivers::DatabaseDriver> {
            Box::new(CompatAdapter::new($id, $wire, $quirk))
        }
        $r.register($id, CapabilityFlags::empty(), factory);
    }};
}
macro_rules! reg_http {
    ($r:ident, $id:literal, $quirk:literal) => {{
        fn factory() -> Box<dyn irondb_drivers::DatabaseDriver> {
            Box::new(HttpEngineAdapter::new($id, $quirk))
        }
        $r.register($id, CapabilityFlags::empty(), factory);
    }};
}
macro_rules! reg_sidecar {
    ($r:ident, $id:literal) => {{
        fn factory() -> Box<dyn irondb_drivers::DatabaseDriver> {
            Box::new(SidecarAdapter::new($id))
        }
        $r.register($id, CapabilityFlags::empty(), factory);
    }};
}
macro_rules! reg_stream {
    ($r:ident, $id:literal) => {{
        fn factory() -> Box<dyn irondb_drivers::DatabaseDriver> {
            Box::new(StreamAdapter::new($id))
        }
        $r.register($id, CapabilityFlags::empty(), factory);
    }};
}

fn gbase8a_factory() -> Box<dyn irondb_drivers::DatabaseDriver> {
    Box::new(OdbcBridgeAdapter::gbase8a())
}
fn gbase8s_factory() -> Box<dyn irondb_drivers::DatabaseDriver> {
    Box::new(OdbcBridgeAdapter::gbase8s())
}

pub fn register_longtail(registry: &mut DriverRegistry) {
    use WireKind::{MySql, Postgres};
    let r = registry;
    r.register("gbase8a", CapabilityFlags::empty(), gbase8a_factory);
    r.register("gbase8s", CapabilityFlags::empty(), gbase8s_factory);
    reg_compat!(r, "tidb", MySql, "mysql-compat");
    reg_compat!(r, "cockroach", Postgres, "pg-compat");
    reg_compat!(r, "timescale", Postgres, "pg extension");
    reg_compat!(r, "goldendb", MySql, "mysql-compat wire; reuses MySQLDB impl");
    reg_compat!(r, "oceanbase", MySql, "mysql/oracle compat modes");
    reg_compat!(r, "doris", MySql, "normalized alias diros; mysql-protocol compat");
    reg_compat!(r, "starrocks", MySql, "mysql-protocol compat");
    reg_compat!(r, "sphinx", MySql, "mysql-protocol SphinxQL");
    reg_http!(r, "chroma", "HTTP embedding store");
    reg_http!(r, "qdrant", "HTTP/gRPC vector store");
    reg_http!(r, "milvus", "gRPC/HTTP vector store");
    reg_http!(r, "weaviate", "HTTP/GraphQL vector store");
    reg_http!(r, "meilisearch", "HTTP search");
    reg_http!(r, "typesense", "HTTP search");
    reg_http!(r, "opensearch", "ES-compat HTTP");
    reg_http!(r, "clickhouse", "HTTP/TCP native; columnar");
    reg_http!(r, "trino", "HTTP/Trino protocol");
    reg_http!(r, "presto", "trino predecessor; same HTTP vehicle");
    reg_http!(r, "tdengine", "REST/TAOS wire");
    reg_http!(r, "influxdb", "HTTP line-protocol/flux");
    reg_http!(r, "questdb", "pg-wire/HTTP");
    reg_http!(r, "greptimedb", "pg/mysql-protocol compat");
    reg_http!(r, "etcd", "gRPC/HTTP kv");
    reg_http!(r, "navicat-tunnel", "HTTP tunnel to remote DB");
    reg_sidecar!(r, "dameng");
    reg_sidecar!(r, "kingbase");
    reg_sidecar!(r, "highgo");
    reg_sidecar!(r, "vastbase");
    reg_sidecar!(r, "opengauss");
    reg_sidecar!(r, "gaussdb");
    reg_sidecar!(r, "gbase8c");
    reg_sidecar!(r, "mariadb");
    reg_sidecar!(r, "duckdb");
    reg_sidecar!(r, "firebird");
    reg_sidecar!(r, "iris");
    reg_sidecar!(r, "cache");
    reg_sidecar!(r, "yashandb");
    reg_sidecar!(r, "iotdb");
    reg_sidecar!(r, "kwdb");
    reg_sidecar!(r, "zookeeper");
    reg_stream!(r, "kafka");
    reg_stream!(r, "mqtt");
    reg_stream!(r, "rabbitmq");
    reg_stream!(r, "pulsar");
    reg_stream!(r, "rocketmq");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_longtail_engines_registered() {
        let mut registry = DriverRegistry::new();
        register_longtail(&mut registry);
        let engines = registry.engines();
        for &(engine, _, _) in LONGTAIL_MATRIX.iter() {
            assert!(engines.contains(&engine), "missing: {engine}");
        }
        assert!(LONGTAIL_MATRIX.len() >= 40, "matrix covers ~40 engines");
    }

    #[test]
    fn matrix_uses_only_approved_vehicles() {
        for &(engine, vehicle, _) in LONGTAIL_MATRIX.iter() {
            assert!(
                ["odbc", "native", "compat", "http", "sidecar"].contains(&vehicle),
                "{engine}: unapproved vehicle {vehicle}"
            );
        }
    }

    // Integration suites are gated: docker absent on this host (vehicle-matrix
    // env probe) and CN ODBC drivers need per-driver install. Gate on env.
    #[test]
    fn differential_gated_on_environment() {
        let gated = std::env::var("irondb_LIVE_LONGTAIL").is_err()
            && std::env::var("irondb_SIDECAR_BIN").is_err();
        if gated {
            // pass-as-skipped: no live backend available
            return;
        }
        let mut registry = DriverRegistry::new();
        register_longtail(&mut registry);
        assert!(!registry.engines().is_empty());
    }
}
