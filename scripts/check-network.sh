#!/bin/sh
# ADR 0016 (amends 0010, 0013): network crates and where they may be used.
set -eu
fail=0

tree=$(cargo tree --workspace -e normal --prefix none)
for c in ureq git2 tauri-plugin-updater keyring; do
  if echo "$tree" | grep -q "^$c v"; then echo "banned crate: $c"; fail=1; fi
done

# reqwest only in skilldesk-core; hyper only through reqwest.
users=$(cargo tree -i reqwest -e normal --depth 1 --prefix none 2>/dev/null | tail -n +2 | awk '{print $1}' | sort -u)
for u in $users; do
  [ "$u" = "skilldesk-core" ] || { echo "reqwest used by $u"; fail=1; }
done
parents=$(cargo tree -i hyper -e normal --depth 1 --prefix none 2>/dev/null | tail -n +2 | awk '{print $1}' | sort -u)
for p in $parents; do
  case "$p" in reqwest|hyper-util|hyper-rustls) ;; *) echo "hyper used by $p"; fail=1 ;; esac
done

# Every request lives in crates/core/src/net/.
if grep -rn "reqwest::" crates src-tauri/src --include='*.rs' | grep -v "^crates/core/src/net/"; then
  echo "reqwest:: used outside crates/core/src/net/"; fail=1
fi

[ "$fail" = 0 ] && echo "network checks ok"
exit "$fail"
