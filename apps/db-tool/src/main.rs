use sqlx::{
    Connection, Executor, PgConnection,
    migrate::{Migrate, Migrator},
    postgres::PgConnectOptions,
};
use std::{
    env,
    error::Error,
    path::{Path, PathBuf},
    process::{Command, Output},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn output(command: &mut Command) -> Result<String> {
    let Output { status, stdout, .. } = command.output()?;
    if !status.success() {
        return Err("schema reference/dump command failed; check local PostgreSQL tools and connection settings".into());
    }
    Ok(String::from_utf8(stdout)?)
}

fn normalize_dump(dump: &str) -> String {
    dump.lines()
        .filter(|line| {
            !line.starts_with("-- Dumped from database version ")
                && !line.starts_with("-- Dumped by pg_dump version ")
                && !line.starts_with("\\restrict ")
                && !line.starts_with("\\unrestrict ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

async fn adopt_initial(
    connection: &mut PgConnection,
    root: &Path,
    migrator: &Migrator,
) -> Result<()> {
    let first = migrator.iter().next().ok_or("no migrations found")?;
    if first.version != 1 {
        return Err("only the original version 1 schema can be adopted".into());
    }
    let ledger: bool =
        sqlx::query_scalar("SELECT to_regclass('public._sqlx_migrations') IS NOT NULL")
            .fetch_one(&mut *connection)
            .await?;
    if ledger {
        return Err("a migration ledger already exists; use normal migration validation".into());
    }
    let expected = output(Command::new("bash").arg(root.join("scripts/db-reference.sh")))?;
    let mut transaction = connection.begin().await?;
    // Keep existing tables stable while pg_dump reads the exported snapshot.
    transaction.execute("LOCK TABLE public.users, public.workspaces, public.memberships, public.files, public.memories, public.memory_sources IN SHARE ROW EXCLUSIVE MODE").await?;
    let snapshot: String = sqlx::query_scalar("SELECT pg_export_snapshot()")
        .fetch_one(&mut *transaction)
        .await?;
    let actual = output(Command::new("pg_dump").args([
        "--schema-only",
        "--lock-wait-timeout=30s",
        "--snapshot",
        &snapshot,
    ]))?;
    if normalize_dump(&actual) != normalize_dump(&expected) {
        return Err("legacy schema differs from migration 1 (including owners, grants and policies); adoption refused".into());
    }
    transaction.ensure_migrations_table().await?;
    sqlx::query("INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) VALUES ($1, $2, true, $3, 0)")
        .bind(first.version).bind(first.description.as_ref()).bind(first.checksum.as_ref())
        .execute(&mut *transaction).await?;
    transaction.commit().await?;
    println!("Verified and adopted initial schema; existing rows preserved");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let root = PathBuf::from(
        args.next()
            .ok_or("usage: aidrive-db <repository> [--adopt-initial]")?,
    )
    .canonicalize()?;
    let mode = args.next();
    if args.next().is_some()
        || mode
            .as_ref()
            .is_some_and(|value| value != "--adopt-initial")
    {
        return Err("usage: aidrive-db <repository> [--adopt-initial]".into());
    }
    let database = env::var("PGDATABASE").unwrap_or_default();
    if !database.starts_with("aidrive")
        || !database
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        || (database != "aidrive" && !database.starts_with("aidrive_"))
    {
        return Err("PGDATABASE must be an explicit aidrive database name".into());
    }
    if ["PGSERVICE", "PGSERVICEFILE", "PGHOSTADDR", "PGOPTIONS"]
        .iter()
        .any(|key| env::var_os(key).is_some())
    {
        return Err("use explicit PGHOST/PGPORT/PGUSER/PGPASSWORD settings, not service/hostaddr/options overrides".into());
    }
    let mut connection = PgConnection::connect_with(&PgConnectOptions::new()).await?;
    let authorized: bool =
        sqlx::query_scalar("SELECT pg_has_role(current_user, 'aidrive_owner', 'MEMBER')")
            .fetch_one(&mut connection)
            .await?;
    if !authorized {
        return Err("migration connection must be able to SET ROLE aidrive_owner".into());
    }
    connection
        .execute("SET ROLE aidrive_owner; SET search_path TO public; SET lock_timeout TO '30s'")
        .await?;
    let mut migrator = Migrator::new(root.join("migrations")).await?;
    connection.lock().await?;
    // One lock covers legacy adoption and SQLx validation/application together.
    migrator.set_locking(false);
    if mode.is_some() {
        adopt_initial(&mut connection, &root, &migrator).await?;
    } else {
        let untracked: bool = sqlx::query_scalar("SELECT to_regclass('public.users') IS NOT NULL AND to_regclass('public._sqlx_migrations') IS NULL")
            .fetch_one(&mut connection).await?;
        if untracked {
            return Err("existing untracked schema: stop application writers and run --adopt-initial for verified adoption".into());
        }
    }
    migrator.run(&mut connection).await?;
    connection.unlock().await?;
    connection.close().await?;
    println!("Migrations validated and applied");
    Ok(())
}
