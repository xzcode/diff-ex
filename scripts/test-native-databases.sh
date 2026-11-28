#!/usr/bin/env bash
# Isolated loopback containers with generated fixture credentials.
set -euo pipefail
engine="$1"
id="diff-ex-db-$engine-$$"
password=$(python3 -c 'import secrets;print(secrets.token_hex(20))')
cleanup(){ docker rm -f "$id" >/dev/null 2>&1 || true; }
trap cleanup EXIT
case "$engine" in
  mysql|mariadb)
    image=mysql:8.4
    if [[ "$engine" == mariadb ]]; then image=mariadb:11.4; fi
    docker run -d --rm --name "$id" -p 127.0.0.1::3306 -e MYSQL_ROOT_PASSWORD="$password" "$image" >/dev/null
    client=mysql;admin=mysqladmin
    if [[ "$engine" == mariadb ]];then client=mariadb;admin=mariadb-admin;fi
    for _ in $(seq 1 90);do
      if docker exec "$id" "$admin" ping -h 127.0.0.1 -uroot "-p$password" --silent >/dev/null 2>&1 && docker exec "$id" "$client" -uroot "-p$password" -e 'SELECT 1' >/dev/null 2>&1;then break;fi
      sleep 2
    done
    docker exec -i "$id" "$client" -uroot "-p$password" <<SQL
CREATE DATABASE diff_left;
CREATE DATABASE diff_right;
CREATE TABLE diff_left.items(id INT PRIMARY KEY,note VARCHAR(50));
CREATE TABLE diff_right.items(id INT PRIMARY KEY,note VARCHAR(50));
CREATE INDEX by_note ON diff_left.items(note);
INSERT INTO diff_left.items VALUES(1,'old'),(2,NULL),(3,'same');
INSERT INTO diff_right.items VALUES(1,'new'),(4,'added'),(3,'same');
CREATE USER 'reader'@'%' IDENTIFIED BY '$password';
GRANT SELECT, SHOW VIEW ON diff_left.* TO 'reader'@'%';
GRANT SELECT, SHOW VIEW ON diff_right.* TO 'reader'@'%';
SQL
    port=$(docker port "$id" 3306/tcp|sed 's/.*://')
    driver=mysql
    ;;
  postgres)
    docker run -d --rm --name "$id" -p 127.0.0.1::5432 -e POSTGRES_PASSWORD="$password" postgres:17 >/dev/null
    for _ in $(seq 1 90);do if docker exec "$id" pg_isready -U postgres -h 127.0.0.1 >/dev/null 2>&1;then break;fi;sleep 2;done
    docker exec -i "$id" psql -U postgres -v ON_ERROR_STOP=1 <<SQL
CREATE ROLE reader LOGIN PASSWORD '$password';
CREATE DATABASE diff_left;
CREATE DATABASE diff_right;
SQL
    for database in diff_left diff_right;do
      docker exec -i "$id" psql -U postgres -d "$database" -v ON_ERROR_STOP=1 <<SQL
CREATE TABLE items(id integer PRIMARY KEY,note text);
GRANT CONNECT ON DATABASE $database TO reader;
GRANT USAGE ON SCHEMA public TO reader;
GRANT SELECT ON items TO reader;
SQL
    done
    docker exec "$id" psql -U postgres -d diff_left -v ON_ERROR_STOP=1 -c "CREATE INDEX by_note ON items(note); INSERT INTO items VALUES(1,'old'),(2,NULL),(3,'same');"
    docker exec "$id" psql -U postgres -d diff_right -v ON_ERROR_STOP=1 -c "INSERT INTO items VALUES(1,'new'),(4,'added'),(3,'same');"
    port=$(docker port "$id" 5432/tcp|sed 's/.*://')
    driver=postgres
    ;;
  mongodb)
    docker run -d --rm --name "$id" -p 127.0.0.1::27017 -e MONGO_INITDB_ROOT_USERNAME=fixture -e MONGO_INITDB_ROOT_PASSWORD="$password" mongo:8.0 >/dev/null
    for _ in $(seq 1 90);do if docker exec "$id" mongosh --quiet --username fixture --password "$password" --authenticationDatabase admin --eval 'db.runCommand({ping:1}).ok' >/dev/null 2>&1;then break;fi;sleep 2;done
    docker exec "$id" mongosh --quiet --username fixture --password "$password" --authenticationDatabase admin --eval "
      db.getSiblingDB('diff_left').items.insertMany([{_id:1,note:'old'},{_id:2,note:null},{_id:3,note:'same'}]);
      db.getSiblingDB('diff_right').items.insertMany([{_id:1,note:'new'},{_id:4,note:'added'},{_id:3,note:'same'}]);
      db.getSiblingDB('diff_left').items.createIndex({note:1},{name:'by_note'});
      db.getSiblingDB('admin').createUser({user:'reader',pwd:'$password',roles:[{role:'read',db:'diff_left'},{role:'read',db:'diff_right'}]});
    "
    port=$(docker port "$id" 27017/tcp|sed 's/.*://')
    driver=mongodb
    ;;
  *) echo "Unsupported fixture engine" >&2;exit 2;;
esac
DIFF_EX_DB_ENGINE="$driver" DIFF_EX_DB_PORT="$port" DIFF_EX_DB_USER=reader DIFF_EX_DB_PASSWORD="$password" \
  cargo test --locked -p diff-ex-core database::live_tests::isolated_database_fixture -- --exact
