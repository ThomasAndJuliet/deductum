#!/usr/bin/env bash
# Fixed-data transport from native Windows; argument contents are never evaluated.
fail() { printf '%s\n' 'Invalid Deductum Isabelle argument payload' >&2; exit 2; }
[[ $# == 1 && ${#1} -le 16384 && $1 =~ ^[A-Za-z0-9+/]+={0,2}$ ]] || fail
decode() {
  printf '%s' "$1" | /usr/bin/base64 --decode || return
  # This separate final field proves the decoded data ended with a NUL.
  printf '%s' '__DEDUCTUM_ARGV_END__'
}
mapfile -d '' -t fields < <(decode "$1")
decoder=$!
wait "$decoder" || fail
[[ ${#fields[@]} -ge 3 && ${fields[-1]} == '__DEDUCTUM_ARGV_END__' ]] || fail
[[ ${fields[0]} =~ ^[1-9][0-9]{0,5}$ ]] || fail
count=${fields[0]}
(( ${#fields[@]} == count + 2 )) || fail
args=("${fields[@]:1:count}")
exec /usr/bin/bash "${args[@]}"
