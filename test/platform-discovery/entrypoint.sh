#!/bin/sh
set -eu
# Only the public CA and a fake token are shared with the server.
# Use a separate leaf certificate: rustls correctly rejects a CA as a server leaf.
umask 077
openssl req -x509 -newkey rsa:2048 -nodes -days 30 \
  -keyout /tmp/ca.key -out /serviceaccount/ca.crt \
  -subj /CN=discovery-demo-ca \
  -addext 'basicConstraints=critical,CA:TRUE' 2>/dev/null
openssl req -newkey rsa:2048 -nodes -keyout /tmp/mock.key \
  -out /tmp/mock.csr -subj /CN=platform-mock 2>/dev/null
cat > /tmp/leaf.ext <<'EXT'
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
subjectAltName=DNS:platform-mock,DNS:localhost
EXT
openssl x509 -req -in /tmp/mock.csr -CA /serviceaccount/ca.crt \
  -CAkey /tmp/ca.key -set_serial 1 -days 30 -out /tmp/mock.crt \
  -extfile /tmp/leaf.ext 2>/dev/null
printf 'mock-service-account' > /serviceaccount/token
printf 'discovery-demo' > /serviceaccount/namespace
chmod 644 /serviceaccount/ca.crt /serviceaccount/token /serviceaccount/namespace
exec python3 /app/mock.py
