## What changed

<!-- One or two sentences. Link the issue: Closes #123 -->

## How I tested it

<!-- Commands you ran, screens you checked. Screenshots for UI changes. -->

## Checklist

- [ ] `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` pass
- [ ] `npm run build` and `npm test` pass
- [ ] Bindings regenerated (`npm run bindings`) if a Rust type crossing into the UI changed
- [ ] Writes go through a plan, back up first, and show in History
- [ ] No new network use outside the Marketplace and the updater
- [ ] `CHANGELOG.md` updated for anything a user would notice
