# Contributing to SkillDesk

Thanks for helping. SkillDesk reads and changes files that people's coding agents depend on, so the bar for a change is simple: it must never lose a user's data, leak a secret, or write somewhere it promised only to read.

## Before you start

- Bug? Open an issue with the bug template. Include your macOS version, SkillDesk version, and which agents you use.
- New feature or a new agent? Open an issue first so we can agree on the shape before you write code.
- Security problem? Don't open an issue. Follow [SECURITY.md](SECURITY.md).

## Set up

You need macOS 12 or later, [Rust](https://rustup.rs) 1.89 or later, and Node 22.

```sh
git clone https://github.com/kushalBanda/skilldesk.git
cd skilldesk
npm ci
npm run app:dev
```

`npm run app:dev` starts Vite on port 1420 and opens the app with hot reload. The first Rust build takes a few minutes.

SkillDesk keeps its own database in `~/Library/Application Support/SkillDesk`. Point it elsewhere while you work, so a dev build never touches your real history:

```sh
SKILLDESK_DATA_DIR=/tmp/skilldesk-dev npm run app:dev
```

## Layout

| Path | What lives there |
| --- | --- |
| `crates/core` | Every rule, scan, parser and write. No Tauri types. |
| `crates/cli` | The `skilldesk` command. Same core, same database. |
| `src-tauri` | The Tauri shell: thin commands over the core, the tray, the window. |
| `src` | The React UI. All copy lives in `src/strings.ts`. |
| `src/ipc/bindings` | TypeScript types generated from Rust. Don't edit by hand. |

## Rules the code keeps

- **The core owns the logic.** The UI never touches files. A Tauri command calls one core function and returns its result.
- **Every write is a plan first.** Writes go through `crates/core/src/actions`: plan, preflight, apply, then a History record. A file that gets replaced or removed goes to the Trash or a backup first, so Undo can put it back.
- **Agent databases are read-only.** Cursor's `state.vscdb` and other agent stores open read-only and immutable. SkillDesk never writes them.
- **Secret values never leave the core.** The store keeps a salted hash plus the known prefix. The UI gets the masked form. The only path to the full value is Touch ID.
- **Local by default.** Only the Marketplace (`crates/core/src/net`) and the app updater use the network. `scripts/check-network.sh` fails CI if a network crate shows up anywhere else.

## Checks

Run these before you push. CI runs the same ones.

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
./scripts/check-network.sh
npm run build
npm test
```

If you change a Rust type that crosses into the UI, regenerate the bindings and commit them:

```sh
npm run bindings
```

CI fails when `src/ipc/bindings` is out of date.

### What to test

Tests guard the promises above. Add one when your change touches:

- a write (can it lose data? does Undo restore it?),
- secret handling (can a value reach the store, a log, or the UI?),
- an agent file SkillDesk must only read.

For UI and layout changes, run the app and check it by hand. Screenshots in the PR help.

## Adding an agent

Most agents need one line in `crates/core/src/adapters/builtin.rs`: a key, a display name, its skills folder and the folder that proves it's installed. That gives the agent skills support (detect, list, link). Full support (hooks, MCP, sessions, memory) needs parsers in `crates/core/src/parse` and `crates/core/src/sessions`; open an issue first.

Add the agent's logo to `src/assets/agents` if one is available under a license that allows it.

## Branches and commits

- `main` is always releasable. Never push to it directly; every change lands through a pull request.
- Name a branch after its type and topic: `feat/session-export`, `fix/cursor-mcp-path`, `docs/readme-install`, `chore/bump-tauri`, `ci/cache-npm`.
- Write commits in [Conventional Commits](https://www.conventionalcommits.org) form: `type(scope): summary`, in the imperative, under 72 characters. Types: `feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `ci`, `chore`. Scope is optional: `feat(updater): …`, `fix(secrets): …`.
- One logical change per commit, so each one builds and can be reverted alone.
- Rebase on `main` instead of merging `main` into your branch.

## Pull requests

- Keep a PR to one change. Small PRs get reviewed faster.
- Use a Conventional Commits title, the same as a commit: it becomes the merge commit on `main`.
- PRs merge with **squash** when the commits are work-in-progress, or **rebase** when each commit stands alone.
- Say what you tested and how.
- Add a line under `Unreleased` in [CHANGELOG.md](CHANGELOG.md) for anything a user would notice.

## License

By contributing, you agree your work is released under the [MIT License](LICENSE).
