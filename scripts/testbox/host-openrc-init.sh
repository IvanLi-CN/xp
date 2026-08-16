#!/bin/sh
set -eu

if [ -f /tls/ca.pem ]; then
  install -Dm0644 /tls/ca.pem /usr/local/share/ca-certificates/xp-testbox-ca.crt
  update-ca-certificates
fi

mkdir -p /etc/xp /run/openrc
printf '%s\n' 'XP_BIND=0.0.0.0:62416' > /etc/xp/xp.env
touch /run/openrc/softlevel
exec /sbin/init
