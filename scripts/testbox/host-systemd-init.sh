#!/bin/sh
set -eu

if [ -f /tls/ca.pem ]; then
  install -Dm0644 /tls/ca.pem /usr/local/share/ca-certificates/xp-testbox-ca.crt
  update-ca-certificates
fi

mkdir -p /etc/xp
printf '%s\n' 'XP_BIND=0.0.0.0:62416' > /etc/xp/xp.env
exec /sbin/init
