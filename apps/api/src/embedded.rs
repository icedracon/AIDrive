// `aidrive-api --embedded-db`: a private PostgreSQL bundled into this binary,
// kept under the user's data folder, bootstrapped like db/bootstrap.sql and
// migrated on every start. The API still connects as the restricted
// aidrive_app role; the superuser password never leaves the data folder.
use argon2::password_hash::rand_core::{OsRng, RngCore};
use postgresql_embedded::{PostgreSQL, Settings, VersionReq};
use sqlx::{
    Connection, Executor, PgConnection,
    migrate::{Migrate, Migrator},
    postgres::PgConnectOptions,
};
use std::{env, fs, io, path::PathBuf, str::FromStr, time::Duration};

pub const DATABASE: &str = "aidrive";
const DEFAULT_PORT: u16 = 5544;

pub struct Embedded {
    server: PostgreSQL,
    pub app: PgConnectOptions,
}

pub fn data_root() -> PathBuf {
    crate::local::data_root()
}

fn secret() -> String {
    let mut bytes = [0u8; 24];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// Reuses the stored secret so restarts keep working; user-only on Unix,
// including files left by an earlier version with wider permissions.
fn stored_secret(path: &PathBuf) -> io::Result<String> {
    if let Ok(existing) = fs::read_to_string(path) {
        let existing = existing.trim().to_owned();
        if !existing.is_empty() {
            private(path, 0o600)?;
            return Ok(existing);
        }
    }
    let value = secret();
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?
            .write_all(value.as_bytes())?;
    }
    #[cfg(not(unix))]
    fs::write(path, &value)?;
    private(path, 0o600)?;
    Ok(value)
}

#[cfg(unix)]
fn private(path: &std::path::Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn private(_: &std::path::Path, _: u32) -> io::Result<()> {
    Ok(())
}

// initdb runs with --auth=password (clear text on the wire); scram-sha-256
// keeps the password off the connection. Returns whether the file changed.
fn require_scram(data_dir: &std::path::Path) -> io::Result<bool> {
    let path = data_dir.join("pg_hba.conf");
    let Ok(text) = fs::read_to_string(&path) else {
        return Ok(false);
    };
    let (fixed, changed) = scram_hba(&text);
    if changed {
        fs::write(&path, fixed)?;
    }
    Ok(changed)
}

fn scram_hba(text: &str) -> (String, bool) {
    let mut changed = false;
    let lines: Vec<String> = text
        .lines()
        .map(|line| match line.trim_end().strip_suffix("password") {
            Some(head)
                if !line.trim_start().starts_with('#') && head.ends_with(char::is_whitespace) =>
            {
                changed = true;
                format!("{head}scram-sha-256")
            }
            _ => line.to_owned(),
        })
        .collect();
    (lines.join("\n") + "\n", changed)
}

fn bundled_version() -> String {
    Settings::default()
        .version
        .to_string()
        .trim_start_matches('=')
        .to_owned()
}

pub async fn start() -> Result<Embedded, Box<dyn std::error::Error>> {
    let root = data_root().join("postgres");
    fs::create_dir_all(&root)?;
    private(&root, 0o700)?;
    let port = match env::var("AIDRIVE_EMBEDDED_PG_PORT") {
        Ok(p) => p.parse()?,
        Err(_) => DEFAULT_PORT,
    };
    let version = bundled_version();
    let password_file = root.join("superuser.pw");
    let password = stored_secret(&password_file)?;
    let mut settings = Settings {
        installation_dir: root.join("server").join(&version),
        data_dir: root.join("data"),
        password_file,
        password: password.clone(),
        host: "127.0.0.1".to_owned(),
        port,
        temporary: false,
        timeout: Some(Duration::from_secs(60)),
        version: VersionReq::from_str(&format!("={version}"))?,
        ..Settings::default()
    };
    settings
        .configuration
        .insert("listen_addresses".into(), "127.0.0.1".into());
    #[cfg(unix)]
    {
        let socket = root.join("socket");
        fs::create_dir_all(&socket)?;
        settings.socket_dir = Some(socket);
    }
    let admin = PgConnectOptions::new()
        .host("127.0.0.1")
        .port(port)
        .username(postgresql_embedded::BOOTSTRAP_SUPERUSER)
        .password(&password);
    // On Unix the admin and app connections use the private socket folder
    // (mode 0700), which another OS account cannot serve.
    #[cfg(unix)]
    let admin = admin.socket(root.join("socket"));
    let data_dir = root.join("data");
    let mut server = PostgreSQL::new(settings);
    server.setup().await?;
    let hba_changed = require_scram(&data_dir)?;
    // A previous API process that was killed leaves its server running.
    // Adopt it only if it is the server of our own data folder. On Windows
    // there is no private socket, so before sending any password the
    // listener on the port must be the postmaster recorded in our data folder.
    let probe = if ours_listening(&data_dir, port) {
        tokio::time::timeout(
            Duration::from_secs(3),
            PgConnection::connect_with(&admin.clone().database("postgres")),
        )
        .await
        .ok()
    } else {
        None
    };
    if let Some(Ok(mut c)) = probe {
        let served: String = sqlx::query_scalar("SELECT current_setting('data_directory')")
            .fetch_one(&mut c)
            .await?;
        if !same_dir(&served, &data_dir) {
            return Err(format!("127.0.0.1:{port} is served by another PostgreSQL ({served}); stop it or set AIDRIVE_EMBEDDED_PG_PORT").into());
        }
        if hba_changed {
            c.execute("SELECT pg_reload_conf()").await?;
        }
        c.close().await?;
        tracing::info!(%version, port, "embedded PostgreSQL already running; reusing it");
    } else {
        server
            .start()
            .await
            .map_err(|e| format!("embedded PostgreSQL {version} did not start on 127.0.0.1:{port} ({e}); is another instance using the port? Set AIDRIVE_EMBEDDED_PG_PORT to change it"))?;
        tracing::info!(%version, port, data = %root.display(), "embedded PostgreSQL started");
    }
    let app_password = stored_secret(&root.join("app.pw"))?;
    bootstrap(&admin, &app_password).await?;
    migrate(&admin).await?;

    Ok(Embedded {
        server,
        app: admin
            .clone()
            .username("aidrive_app")
            .password(&app_password)
            .database(DATABASE),
    })
}

// Same roles and database as db/bootstrap.sql. The database uses PostgreSQL's
// builtin C.UTF-8 provider, so lower() folds Armenian on every OS (readiness
// checks it) without depending on the host's locales.
async fn bootstrap(admin: &PgConnectOptions, app_password: &str) -> Result<(), sqlx::Error> {
    let mut c = PgConnection::connect_with(&admin.clone().database("postgres")).await?;
    c.execute("DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'aidrive_owner') THEN CREATE ROLE aidrive_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS; END IF; END $$")
        .await?;
    let has_app: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'aidrive_app')")
            .fetch_one(&mut c)
            .await?;
    let password: String = sqlx::query_scalar("SELECT quote_literal($1)")
        .bind(app_password)
        .fetch_one(&mut c)
        .await?;
    if has_app {
        c.execute(format!("ALTER ROLE aidrive_app PASSWORD {password}").as_str())
            .await?;
    } else {
        c.execute(format!("CREATE ROLE aidrive_app LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOINHERIT PASSWORD {password}").as_str())
            .await?;
    }
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_database WHERE datname = $1)")
            .bind(DATABASE)
            .fetch_one(&mut c)
            .await?;
    if !exists {
        c.execute(format!("CREATE DATABASE {DATABASE} OWNER aidrive_owner TEMPLATE template0 ENCODING 'UTF8' LOCALE 'C' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'").as_str())
            .await?;
        c.execute(format!("REVOKE ALL ON DATABASE {DATABASE} FROM PUBLIC; GRANT CONNECT ON DATABASE {DATABASE} TO aidrive_app").as_str())
            .await?;
    }
    c.close().await
}

// The migration path of apps/db-tool, with the migrations compiled in.
async fn migrate(admin: &PgConnectOptions) -> Result<(), sqlx::Error> {
    let mut c = PgConnection::connect_with(&admin.clone().database(DATABASE)).await?;
    c.execute("SET ROLE aidrive_owner; SET search_path TO public; SET lock_timeout TO '30s'")
        .await?;
    c.lock().await?;
    let mut migrator: Migrator = sqlx::migrate!("../../migrations");
    migrator.set_locking(false);
    migrator.run(&mut c).await?;
    c.unlock().await?;
    c.close().await
}

#[cfg(unix)]
fn ours_listening(_: &std::path::Path, _: u16) -> bool {
    true
}

// postmaster.pid: line 1 is the pid, line 4 the port. The pid must be the
// process listening on 127.0.0.1:port (netstat), so a server another account
// started on the port is never contacted.
#[cfg(not(unix))]
fn ours_listening(data_dir: &std::path::Path, port: u16) -> bool {
    let Ok(pid_file) = fs::read_to_string(data_dir.join("postmaster.pid")) else {
        return false;
    };
    let mut lines = pid_file.lines();
    let pid = lines.next().map(str::trim).unwrap_or_default().to_owned();
    if lines.nth(2).map(str::trim) != Some(&port.to_string()) || pid.is_empty() {
        return false;
    }
    std::process::Command::new("netstat")
        .args(["-ano", "-p", "TCP"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .is_some_and(|out| listener_pid_matches(&out, port, &pid))
}

#[cfg_attr(unix, allow(dead_code))]
fn listener_pid_matches(netstat: &str, port: u16, pid: &str) -> bool {
    let local = format!("127.0.0.1:{port}");
    netstat.lines().any(|line| {
        let cols: Vec<&str> = line.split_whitespace().collect();
        cols.len() >= 5 && cols[1] == local && cols[3] == "LISTENING" && cols[4] == pid
    })
}

fn same_dir(served: &str, ours: &std::path::Path) -> bool {
    match (fs::canonicalize(served), fs::canonicalize(ours)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

impl Embedded {
    pub async fn stop(self) {
        if let Err(e) = self.server.stop().await {
            tracing::warn!("embedded PostgreSQL did not stop cleanly: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{listener_pid_matches, scram_hba};

    #[test]
    fn only_the_recorded_postmaster_counts_as_ours() {
        let out = "  Proto  Local Address  Foreign Address  State  PID\n  TCP    127.0.0.1:5544   0.0.0.0:0   LISTENING   4242\n  TCP    0.0.0.0:5545   0.0.0.0:0   LISTENING   4242\n";
        assert!(listener_pid_matches(out, 5544, "4242"));
        assert!(!listener_pid_matches(out, 5544, "999"));
        assert!(!listener_pid_matches(out, 5545, "4242"));
    }

    #[test]
    fn hba_password_lines_become_scram() {
        let text = "# password here\nlocal   all   all   password\nhost all all 127.0.0.1/32 password\r\nhost all all ::1/128 trust\n";
        let (fixed, changed) = scram_hba(text);
        assert!(changed);
        assert!(fixed.contains("local   all   all   scram-sha-256"));
        assert!(fixed.contains("127.0.0.1/32 scram-sha-256"));
        assert!(fixed.contains("# password here"));
        assert!(fixed.contains("::1/128 trust"));
        assert!(!scram_hba(&fixed).1);
    }
}
