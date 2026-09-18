#!/bin/sh
# This is free and unencumbered software released into the public domain.

# Rank order deliberately differs from lexical URI order. This fixture supports
# both pagination modes, but rejects --limit to exercise runner-side enforcement.
sort=rank
offset=0
before=
after=
inserted=
for argument in "$@"; do
    case "$argument" in
        --sort=*) sort=${argument#--sort=} ;;
        --offset=*) offset=${argument#--offset=} ;;
        --before=*) before=${argument#--before=} ;;
        --after=*) after=${argument#--after=} ;;
        --inserted) inserted='urn:item:inserted' ;;
        --limit=*) printf '%s\n' '--limit is unsupported' >&2; exit 64 ;;
        example:collection) ;;
        *) printf '%s\n' 'unexpected argument' >&2; exit 64 ;;
    esac
done
case "$sort" in
    rank) entries="urn:item:alpha $inserted urn:item:zeta urn:item:beta urn:item:omega" ;;
    -rank) entries="urn:item:omega urn:item:beta urn:item:zeta $inserted urn:item:alpha" ;;
    *) exit 64 ;;
esac
active=0
[ -z "$after" ] && active=1
for id in $entries; do
    [ "$id" = "$before" ] && break
    if [ "$active" -eq 0 ]; then
        [ "$id" = "$after" ] && active=1
        continue
    fi
    if [ "$offset" -gt 0 ]; then
        offset=$((offset - 1))
        continue
    fi
    printf '{"@id":"%s"}\n' "$id"
done
