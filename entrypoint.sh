#!/usr/bin/env bash

echo "Setting uid/gid..."
[ -n "${MYUID}" ] && usermod -u "${MYUID}" "${USER}"
[ -n "${MYGID}" ] && groupmod -g "${MYGID}" "${USER}"

exec supervisord -c /srv/supervisor.conf
