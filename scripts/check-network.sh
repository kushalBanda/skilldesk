#!/bin/sh
# Network crates and where they may be used. SkillDesk is local by default:
# only the Marketplace (crates/core/src/net) and the app updater go online.
set -eu
fail=0

tree=$(cargo tree --workspace -e normal --prefix none)
for c in ureq git2 keyring; do
  if echo "$tree" | grep -q "^$c v"; then echo "banned crate: $c"; fail=1; fi
done

# reqwest only in skilldesk-core and the updater plugin; hyper only through reqwest.
users=$(cargo tree -i reqwest -e normal --depth 1 --prefix none 2>/dev/null | tail -n +2 | awk '{print $1}' | sort -u)
for u in $users; do
  case "$u" in skilldesk-core|tauri-plugin-updater) ;; *) echo "reqwest used by $u"; fail=1 ;; esac
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
