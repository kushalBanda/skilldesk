# Changelog

All notable changes to SkillDesk are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org).

## Unreleased

### Added

- App updates: SkillDesk checks GitHub Releases once per launch and installs signed updates from Settings. The check can be turned off.
- The release build carries the `skilldesk` command, installable from Settings.
- README, contributing guide, security policy, code of conduct, and issue templates.

## 0.1.0

First public version.

### Added

- One inventory across Claude Code, Cursor, Codex, OpenCode, Gemini CLI, and GitHub Copilot CLI: skills, plugins, hooks, MCP servers, subagents, commands, memory, sessions, and disk use.
- Skills support for 50 more agents: detect, list, and link skills into each agent's folder.
- Dashboard and a ranked Fix list: broken and slow hooks, dangling skill links, skills that never ran, ghost plugins, unused marketplaces, MCP servers never called, duplicate memories, secrets in history, and reclaimable disk space.
- Secrets screen: finds API keys, tokens, and private keys in agent history, masked, with Touch ID to reveal and a redact action.
- Marketplace: search and install skills from skills.sh and GitHub.
- Sessions: search past prompts across every agent and resume a session.
- Cleanup: disk use by agent, with old caches and logs moved to the Trash.
- History and Undo for every change.
- Menu-bar tray with the fix count and top fixes.
- `skilldesk` command-line tool with JSON output.
