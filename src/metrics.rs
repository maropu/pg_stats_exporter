use postgres::{Client, Error};
use prometheus::{core::Collector, IntGauge};

use crate::postgres_connection::PgConnectionConfig;

// A definithin of `statsinfo.cpustats` is as follows:
//
//  CREATE FUNCTION statsinfo.cpustats
//  (
//  	IN  prev_cpustats	statsinfo.cpustats_type,
//  	OUT cpu_id			text,
//  	OUT cpu_user		bigint,
//  	OUT cpu_system		bigint,
//  	OUT cpu_idle		bigint,
//  	OUT cpu_iowait		bigint,
//  	OUT overflow_user	smallint,
//  	OUT overflow_system	smallint,
//  	OUT overflow_idle	smallint,
//  	OUT overflow_iowait	smallint
//  )
//  RETURNS SETOF record
//  AS 'MODULE_PATHNAME', 'statsinfo_cpustats'
//  LANGUAGE C STRICT;
//
// https://github.com/ossc-db/pg_statsinfo/blob/15.1/agent/lib/pg_statsinfo.sql.in#L127-L142
fn get_cpustats(conn: &mut Client) -> Result<Vec<prometheus::proto::MetricFamily>, Error> {
    // TODO: Checks if the query below always returns a single row
    let row = conn.query_one(
        "
        SELECT
            stats.cpu_id,
            stats.cpu_system,
            stats.cpu_idle,
            stats.cpu_iowait
        FROM
            statsinfo.cpustats() AS stats
        LIMIT 1
    ",
        &[],
    )?;

    let mut metrics: Vec<prometheus::proto::MetricFamily> = vec![];

    let cpu_id: String = row.get(0);
    let stat_prefix = format!("cpustats_{}", cpu_id);

    let mut append_stat = |value: i64, stat_name: &str, help: &str| {
        // TODO: Is it okay to create a new `IntGauge` on the fly?
        let m = IntGauge::new(format!("{}_{}", stat_prefix, stat_name), help).unwrap();
        m.set(value);
        metrics.append(&mut m.collect());
    };

    // TODO: How do we push `row.get` inside `append_stat`?
    append_stat(
        row.get(1),
        "cpu_system",
        "The amount of time CPUs spent in running the operating system functions",
    );
    append_stat(
        row.get(2),
        "cpu_idle",
        "The amount of time CPUs weren't  busy",
    );
    append_stat(
        row.get(3),
        "cpu_iowait",
        "The amount of time CPUs where idle during which the system had pending I/O requests",
    );

    Ok(metrics)
}

// A definithin of `statsinfo.tablespace` is as follows:
//
//  CREATE FUNCTION statsinfo.tablespaces(
//  	OUT oid oid,
//  	OUT name text,
//  	OUT location text,
//  	OUT device text,
//  	OUT avail bigint,
//  	OUT total bigint,
//  	OUT spcoptions text[])
//  RETURNS SETOF record
//  AS 'MODULE_PATHNAME', 'statsinfo_tablespaces'
//  LANGUAGE C STRICT;
//
// https://github.com/ossc-db/pg_statsinfo/blob/15.1/agent/lib/pg_statsinfo.sql.in#L84-L97
fn get_tablespaces_stats(conn: &mut Client) -> Result<Vec<prometheus::proto::MetricFamily>, Error> {
    let row = conn.query(
        "
        SELECT
            stats.name,
            stats.location,
            stats.avail,
            stats.total
        FROM
            statsinfo.tablespaces() AS stats
    ",
        &[],
    )?;

    let mut metrics: Vec<prometheus::proto::MetricFamily> = vec![];

    let mut append_stat = |value: i64, stat_name: &str, help: &str| {
        // TODO: Is it okay to create a new `IntGauge` on the fly?
        let m = IntGauge::new(stat_name, help).unwrap();
        m.set(value);
        metrics.append(&mut m.collect());
    };

    for row in row.iter() {
        let name: String = row.get(0);
        let stat_prefix = format!("tablespaces_{}", name);
        let location: String = row.get(1);

        // TODO: How do we push `row.get` inside `append_stat`?
        append_stat(
            row.get(2),
            &format!("{}_avail", stat_prefix),
            &format!("Available space in {}", location),
        );
        append_stat(
            row.get(3),
            &format!("{}_total", stat_prefix),
            &format!("Total space in {}", location),
        );
    }

    Ok(metrics)
}

// TODO: Adds more methods for the other metrics of `pg_statsinfo`

/// Gathers all Prometheus metrics via a PostgreSQL connection.
pub fn gather(postgres: &PgConnectionConfig) -> Vec<prometheus::proto::MetricFamily> {
    let mut metrics: Vec<prometheus::proto::MetricFamily> = vec![];

    let mut conn = postgres
        .connect_no_tls()
        .unwrap_or_else(|_| panic!("Failed to connect to {}", postgres.raw_address()));
    metrics.append(&mut get_cpustats(&mut conn).unwrap());
    metrics.append(&mut get_tablespaces_stats(&mut conn).unwrap());
    metrics
}

// TODO: Add tests for the functions in this file
