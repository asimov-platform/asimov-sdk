#!/bin/sh
# This is free and unencumbered software released into the public domain.

# This test lister accepts --limit but deliberately fails to enforce it.
# Execute the supplied test body so the runner must enforce its own line cap.
case "${1-}" in
    --limit=*) shift ;;
    *) printf '%s\n' 'expected --limit=COUNT' >&2; exit 64 ;;
esac
if [ "${1-}" != '-c' ]; then
    printf '%s\n' 'expected -c SCRIPT' >&2
    exit 64
fi
shift
exec /bin/sh -c "$1"
