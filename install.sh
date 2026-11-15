#!/bin/bash
# Installs or updates SkillDesk from the latest GitHub release:
#   curl -fsSL https://raw.githubusercontent.com/kushalBanda/skilldesk/main/install.sh | bash
# curl does not set the quarantine flag that browsers do, so the unnotarized
# app opens without the "is damaged" dialog.
set -euo pipefail

REPO="kushalBanda/skilldesk"
APP="SkillDesk.app"

if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
  B=$'\033[1m' D=$'\033[2m' G=$'\033[32m' R=$'\033[31m' X=$'\033[0m'
else
  B="" D="" G="" R="" X=""
fi
ok()   { printf '  %s✓%s %s\n' "$G" "$X" "$*"; }
fail() { printf '  %s✗%s %s\n\n' "$R" "$X" "$*" >&2; exit 1; }

printf '\n  %sSkillDesk%s %s· everything your coding agents know and keep%s\n\n' "$B" "$X" "$D" "$X"

[ "$(uname -s)" = "Darwin" ] || fail "SkillDesk runs on macOS only."
os=$(sw_vers -productVersion)
[ "${os%%.*}" -ge 12 ] || fail "SkillDesk needs macOS 12 or later. This Mac has $os."
case "$(uname -m)" in arm64) chip="Apple silicon" ;; *) chip="Intel" ;; esac
ok "macOS $os, $chip"

if [ -w /Applications ]; then dest=/Applications; else dest="$HOME/Applications"; mkdir -p "$dest"; fi

release=$(curl -sL -w '\n%{http_code}' "https://api.github.com/repos/$REPO/releases/latest") \
  || fail "Couldn't reach GitHub. Check your connection and run this again."
code=${release##*$'\n'}
release=${release%$'\n'*}
case "$code" in
  200) ;;
  404) fail "No SkillDesk release is published yet. Watch github.com/$REPO/releases." ;;
  403|429) fail "GitHub is limiting requests from this network. Try again in a few minutes." ;;
  *) fail "GitHub answered $code. Try again in a few minutes." ;;
esac
version=$(printf '%s' "$release" | sed -n 's/.*"tag_name": *"v\{0,1\}\([^"]*\)".*/\1/p' | head -1)
url=$(printf '%s' "$release" \
  | grep -o '"browser_download_url": *"[^"]*\.app\.tar\.gz"' \
  | sed 's/.*"\(https[^"]*\)"/\1/' | head -1)
[ -n "$url" ] || fail "The latest release has no app download yet. Try again in a few minutes."

installed=""
if [ -d "$dest/$APP" ]; then
  installed=$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$dest/$APP/Contents/Info.plist" 2>/dev/null || true)
fi

if [ -n "$version" ] && [ "$installed" = "$version" ]; then
  ok "SkillDesk $version is already installed and up to date"
else
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT

  curl -fsSL "$url" -o "$tmp/app.tar.gz" || fail "The download failed. Run this again to retry."
  size=$(du -m "$tmp/app.tar.gz" | cut -f1)
  ok "Downloaded SkillDesk ${version:-latest} (${size} MB)"

  tar -xzf "$tmp/app.tar.gz" -C "$tmp"
  [ -d "$tmp/$APP" ] || fail "The download didn't contain $APP. Please open an issue on github.com/$REPO."

  if pgrep -xq SkillDesk; then
    osascript -e 'quit app "SkillDesk"' >/dev/null 2>&1 || true
    for _ in 1 2 3 4 5 6 7 8 9 10; do pgrep -xq SkillDesk || break; sleep 0.5; done
  fi

  # Stage next to the target so the final mv is a rename on the same volume.
  rm -rf "$dest/.$APP.new"
  mv "$tmp/$APP" "$dest/.$APP.new"
  rm -rf "${dest:?}/$APP"
  mv "$dest/.$APP.new" "$dest/$APP"
  xattr -dr com.apple.quarantine "$dest/$APP" 2>/dev/null || true

  if [ -n "$installed" ]; then
    ok "Updated $installed → ${version:-latest} in $dest"
  else
    ok "Installed to $dest/$APP"
  fi
fi

open "$dest/$APP"
ok "Opened SkillDesk"

if [ -n "$installed" ]; then echo; exit 0; fi

cat <<EOF

  ${B}What happens next${X}
  SkillDesk is scanning every coding agent on this Mac. The first scan
  takes about two minutes. Then the ${B}Fix list${X} shows what to clean up.
  Nothing about your files leaves your Mac.

  ${D}Command line${X}  Settings › Command-line tool
  ${D}Update${X}        Run this command again. The app also updates itself.
  ${D}Uninstall${X}     Move SkillDesk to the Trash

EOF
