#!/usr/bin/env bash

: "${TESTBOX:?set TESTBOX to an authorized SSH target}"
: "${XP_TESTBOX_REMOTE_ROOT:?set XP_TESTBOX_REMOTE_ROOT to a writable remote root}"

TESTBOX_REMOTE_ROOT="${XP_TESTBOX_REMOTE_ROOT%/}"
case "$TESTBOX_REMOTE_ROOT" in
  ""|/|*/..|*/../*)
    echo "XP_TESTBOX_REMOTE_ROOT must be a dedicated absolute directory" >&2
    exit 2
    ;;
  *[!A-Za-z0-9._/-]*)
    echo "XP_TESTBOX_REMOTE_ROOT must contain only safe path characters" >&2
    exit 2
    ;;
  /*) ;;
  *)
    echo "XP_TESTBOX_REMOTE_ROOT must be absolute" >&2
    exit 2
    ;;
esac
