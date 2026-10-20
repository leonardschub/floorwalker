# Contributing

Thanks for wanting to help. Floorwalker is a small, one-person project, so a short
issue first saves both of us time on anything bigger than a fix.

## Set up

Follow [Develop](README.md#develop) in the README and the
[desktop setup](desktop/README.md). You need Linux on a native Wayland session, since
the app does not start anywhere else, and the Claude Code CLI to see real sessions.

## Before you open a pull request

Run the checks from [Validation](README.md#validation):

```bash
pnpm lint
pnpm test
cd desktop/src-tauri && cargo fmt --check && cargo test --workspace && cargo clippy --workspace
```

- Keep changes focused, with tests for new behaviour. The pure rules (`floor_plan`,
  `cctv_state`, `seating` and the others) and the `agents` and `terminals` crates are
  built test-first; the world's Bevy systems only draw what those rules decide.
- Use the terms in [`GLOSSARY.md`](GLOSSARY.md). Record a decision with a real
  trade-off as an ADR in [`docs/adr/`](docs/adr/).
- Anything that acts on a real session (sitting in, KILL, WORK HARDER, New session)
  must act on the session the user chose and on no other. Say in the pull request how
  you checked that.
- AI-assisted changes are welcome, as most of this repository was written that way.
  Review them as if you wrote them, and credit the model with a `Co-Authored-By:`
  trailer as the existing commits do. [`AGENTS.md`](AGENTS.md) is where an agent
  starts.

## Licence

Floorwalker is licensed under [AGPL-3.0-or-later](LICENSE). By contributing you
agree that your contribution is licensed the same way, and that you have the right to
submit it.

Security problems go through [SECURITY.md](SECURITY.md), not a public issue.
