#!/usr/bin/env bash
# Isolated loopback-only fixture. No user SSH keys, no existing daemon config.
set -euo pipefail
d=$(mktemp -d)
pid=
cleanup() { if [[ -n "$pid" ]]; then sudo kill "$pid" 2>/dev/null || true; fi; rm -rf "$d"; }
trap cleanup EXIT
mkdir -p "$d/files/folder"
printf 'hello SFTP\n' > "$d/files/folder/test.txt"
printf 'another file\n' > "$d/files/other.txt"
ssh-keygen -q -t ed25519 -N '' -f "$d/host"
ssh-keygen -q -t ed25519 -N '' -f "$d/client"
cp "$d/client.pub" "$d/authorized_keys"
port=$(python3 -c 'import socket;s=socket.socket();s.bind(("127.0.0.1",0));print(s.getsockname()[1]);s.close()')
cat > "$d/sshd_config" <<EOF
Port $port
ListenAddress 127.0.0.1
HostKey $d/host
PidFile $d/sshd.pid
AuthorizedKeysFile $d/authorized_keys
StrictModes no
PasswordAuthentication no
KbdInteractiveAuthentication no
PubkeyAuthentication yes
PermitRootLogin no
UsePAM yes
AllowUsers $(id -un)
Subsystem sftp internal-sftp
LogLevel ERROR
EOF
sudo mkdir -p /run/sshd
sudo /usr/sbin/sshd -t -f "$d/sshd_config"
sudo /usr/sbin/sshd -f "$d/sshd_config"
pid=$(cat "$d/sshd.pid")
export DIFF_EX_SFTP_TEST_HOST=127.0.0.1 DIFF_EX_SFTP_TEST_PORT="$port" DIFF_EX_SFTP_TEST_USER="$(id -un)" DIFF_EX_SFTP_TEST_PIN="$(ssh-keygen -lf "$d/host.pub" -E sha256 | awk '{print $2}')" DIFF_EX_SFTP_TEST_KEY="$d/client" DIFF_EX_SFTP_TEST_PATH="$d/files"
cargo test --locked -p diff-ex-core remote::tests::sftp_fixture_if_configured -- --exact
