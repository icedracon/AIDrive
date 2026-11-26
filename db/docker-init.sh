# Sourced or executed once from /docker-entrypoint-initdb.d on an empty Compose volume.
PGUSER="$POSTGRES_USER" psql -X -v ON_ERROR_STOP=1 -d postgres -v db_name=aidrive -f /aidrive/db/bootstrap.sql
