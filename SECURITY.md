# Security policy

SkillDesk reads agent history, settings and credentials on your Mac, and it can change agent files. Security reports get priority over everything else.

## Report a vulnerability

Please don't open a public issue. Use GitHub's private reporting instead:

1. Go to the [Security tab](https://github.com/kushalBanda/skilldesk/security) of this repo.
2. Click **Report a vulnerability**.
3. Describe the problem, the SkillDesk version, your macOS version, and steps to reproduce it.

You'll get a reply within 7 days. Once a fix ships, the advisory is published with credit to you, unless you'd rather stay anonymous.

## Supported versions

Only the latest release gets security fixes. The app updates itself, so staying current takes one click.

## What counts

These are in scope:

- A secret value reaching SkillDesk's database, a log, the UI, or the network unmasked.
- A write that bypasses the plan, the Trash backup, or Undo.
- A write to a file SkillDesk promises only to read, such as an agent's database.
- A network request outside the Marketplace and the updater, or to a host outside the allow list in `crates/core/src/net/mod.rs`.
- A Marketplace download that can write outside the chosen skills folder (path traversal, symlinks, zip slip).
- An update that installs without a valid signature.

These are out of scope:

- Problems that need an attacker who already controls your user account.
- Secrets that SkillDesk shows after a successful Touch ID or password check. That's the feature.
- Bugs in the agents themselves (Claude Code, Cursor, and so on). Report those to their vendors.

## How SkillDesk protects you

- **Local by default.** No telemetry, no account. The network is used only when you open the Marketplace and when the app checks GitHub Releases for an update. You can turn the update check off in Settings.
- **Secrets stay masked.** The database stores a salted hash and the known prefix of each value, never the value itself.
- **Signed updates.** Every update is signed with a minisign key. The app refuses one that doesn't verify against the public key built into it.
- **Read-only agent databases.** Agent SQLite stores open read-only and immutable.
- **Undo for every write.** Files go to the Trash or a backup first, and History records what changed.
