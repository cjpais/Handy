#!/usr/bin/env bash
# Creates the self-signed code-signing certificate Voxa releases are signed with.
# One certificate for all releases keeps macOS permissions (Microphone,
# Accessibility, Input Monitoring) across updates.
#
#   bash scripts/voxa/make-signing-cert.sh
#
# Writes ~/.voxa/voxa-signing.p12 and prints the two values to paste into
# GitHub (MACOS_CERT_P12_BASE64 goes to the clipboard on a Mac). Keep the files:
# a new certificate means a one-time re-grant of the permissions.
set -euo pipefail

NAME="${1:-Voxa Code Signing}"
DIR="$HOME/.voxa"
P12="$DIR/voxa-signing.p12"

mkdir -p "$DIR"
chmod 700 "$DIR"
if [ -e "$P12" ]; then
  echo "$P12 already exists. Delete it first if you really want a new certificate." >&2
  exit 1
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

cat > "$WORK/openssl.cnf" <<CNF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $NAME
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
CNF

openssl req -x509 -newkey rsa:2048 -nodes -days 3650 \
  -config "$WORK/openssl.cnf" \
  -keyout "$WORK/key.pem" -out "$WORK/cert.pem" 2>/dev/null

PASSWORD="$(openssl rand -hex 16)"

# OpenSSL 3 needs -legacy so macOS `security import` can read the file;
# the LibreSSL that ships with macOS rejects the flag and does not need it.
if ! openssl pkcs12 -export -legacy -inkey "$WORK/key.pem" -in "$WORK/cert.pem" \
     -name "$NAME" -out "$P12" -passout "pass:$PASSWORD" 2>/dev/null; then
  openssl pkcs12 -export -inkey "$WORK/key.pem" -in "$WORK/cert.pem" \
    -name "$NAME" -out "$P12" -passout "pass:$PASSWORD"
fi
chmod 600 "$P12"

B64="$(base64 < "$P12" | tr -d '\n')"
if command -v pbcopy >/dev/null 2>&1; then
  printf '%s' "$B64" | pbcopy
  COPIED="(already copied to the clipboard)"
else
  COPIED="(printed below)"
fi

cat <<OUT

Certificate created: $P12

GitHub secrets (Settings > Secrets and variables > Actions > New repository secret):

  MACOS_CERT_P12_BASE64   the base64 of the file $COPIED
  MACOS_CERT_PASSWORD     $PASSWORD
  MACOS_CERT_NAME         $NAME     (optional)

OUT
if ! command -v pbcopy >/dev/null 2>&1; then
  echo "MACOS_CERT_P12_BASE64 value:"
  echo "$B64"
fi
