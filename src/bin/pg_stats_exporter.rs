//!
//! A PostgreSQL metrics exporter for Prometheus.
//!
use anyhow::{anyhow, bail};
use clap::{Arg, Command};
use pg_stats_exporter::{
    postgres_connection::{parse_host_port, PgConnectionConfig},
    routes, tcp_listener,
};
use routes::State;
use std::sync::Arc;

const DEFAULT_PG_STATS_EXPORTER_API: &str = "127.0.0.1:9753";

fn main() -> anyhow::Result<()> {
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

    let (host, port) = parse_host_port(postgres).expect("Unable to parse `postgres`");
    let port = port.unwrap_or(5432);
    let postgres = PgConnectionConfig::new_host_port(host, port)
        .set_user(Some(user))
        .set_dbname(Some(dbname));
    if !postgres.can_connect() {
        bail!("Failed to connect to {}", postgres.raw_address());
    }

    let state = Arc::new(State {
        pgnode: Box::leak(Box::new(postgres)),
    });

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .thread_name("http server")
        // if you change the number of worker threads please change the constant below
        .enable_all()
        .build()?;

    runtime.block_on(async {
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

        anyhow::Ok(())
    })
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
