//!
//! A PostgreSQL metrics exporter for Prometheus.
//!
use anyhow::anyhow;
use clap::{Arg, Command};
use hyper;
use pg_stats_exporter::{db::PostgresNode, tcp_listener, routes};
use routes::State;
use routerify;
use std::sync::Arc;
use tokio;

const DEFAULT_PG_STATS_EXPORTER_API: &str = "127.0.0.1:9753";

#[tokio::main(flavor = "multi_thread")]
async fn main() -> anyhow::Result<()> {
    let arg_matches = cli().get_matches();

    let postgres = arg_matches
        .get_one::<String>("postgres")
        .map(|s| s.as_str())
        .unwrap_or("127.0.0.1:5432")
        .to_string();

    let user = arg_matches
        .get_one::<String>("user")
        .map(|s| s.as_str())
        .unwrap_or("docker")
        .to_string();

    let dbname = arg_matches
        .get_one::<String>("dbname")
        .map(|s| s.as_str())
        .unwrap_or("postgres")
        .to_string();

    let state = Arc::new(State {
        pgnode: Box::leak(Box::new(PostgresNode {
            addr: postgres,
            user: user,
            dbname: dbname,
        })),
    });

    let http_listener = tcp_listener::bind(DEFAULT_PG_STATS_EXPORTER_API)?;
    let router = routes::make_router(state)?
        .build()
        .map_err(|err| anyhow!(err))?;
    let service = routerify::RouterService::new(router).unwrap();
    let server = hyper::Server::from_tcp(http_listener)?
        .serve(service)
        .with_graceful_shutdown(shutdown_watcher());

    // Run the server until shutdown requested
    if let Err(e) = server.await {
        eprintln!("Server error: {}", e);
    }

    Ok(())
}

async fn shutdown_watcher() {
    // Wait for the CTRL+C signal
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install CTRL+C signal handler");
}

fn cli() -> Command {
    Command::new("PostgreSQL metrics exporter")
        .version("0.1.0")
        .arg(
            Arg::new("postgres")
                .long("postgres")
                .help("PostgreSQL address to collect metrics"),
        )
        .arg(
            Arg::new("user")
                .long("user")
                .help("PosgreSQL user used to access a `postgres` address"),
        )
        .arg(
            Arg::new("dbname")
                .long("dbname")
                .help("PostgreSQL database name used to access a `postgres` address"),
        )
}

#[test]
fn verify_cli() {
    cli().debug_assert();
}
