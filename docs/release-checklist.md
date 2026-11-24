# Release checklist (draft)

A draft for a first public release. Nothing here has been done yet except
where marked. No remote exists, nothing is pushed or published.

## Decisions for the owner

- [x] Final name: graft (crates `graft-rules`, `graft-rules-core`,
      `graft-rules-watch`), command `graft`. Renamed in 4.0.0 (3.x was
      Bogo-volution, crate `bogovolution`). `graft` and `graft-core` on
      crates.io belong to an unrelated project; the three `graft-rules` names
      were free on 2026-11-23 (a free name is not reserved until the first
      publish).
- [ ] Where to host the repository. Then set `repository` in the root
      `Cargo.toml` (a TODO comment marks the spot).
- [ ] Create the crates.io account and publish, in order: `graft-rules-core`,
      `graft-rules-watch`, `graft-rules` (`cargo package --workspace`
      already builds all three without publishing).
- [ ] Whether to ask Petri's creator before announcing (see the draft message
      outside the repo), and how to word the credit.
- [ ] Whether the paid providers ship in the first release, given they were
      never run against a real endpoint.

## Code and tests (all local, free)

- [x] `cargo test`, `cargo clippy --all-targets -- -D warnings`,
      `cargo fmt --check` green.
- [x] Viewer: `npm test`, `npm run check`, `npm run build`, `npm run smoke`.
- [x] Plugin: `integrations/claude-code/tests/run-all.sh`.
- [x] Escape tests with `GRAFT_REQUIRE_BACKENDS=bwrap,docker,podman`
      on one Debian 13 machine.
- [ ] Escape tests on a second machine, ideally with systemd and cgroup
      delegation (rootless podman with real cgroup limits), and on macOS with
      docker or podman.
- [ ] `claude plugin validate` for the plugin and the marketplace (the
      `claude` CLI was not installed where this was built).
- [ ] `cargo install --path crates/cli --locked` on a clean machine.
      (Done once into a temp folder on the build machine.)
- [ ] Check the minimum Rust version and state it in `Cargo.toml`
      (`rust-version`). Only 1.100.0 was tested.
- [ ] Run `cargo deny` or a similar license and advisory check on the
      dependency tree (new in 2.5.0: `ureq`, `rustls`, `ring`,
      `webpki-roots` under CDLA-Permissive-2.0).

## Docs

- [x] README: install, quickstart, findings, Petri credit.
- [x] CHANGELOG.md.
- [x] docs/writeup.md.
- [x] Replace `<this repo>` placeholders with the real URL (https://github.com/ayaan-cs/graft).
- [ ] Read the README top to bottom once more; it is long. Consider moving the
      sandbox and plugin sections into `docs/`.
- [ ] Regenerate screenshots if the viewer changed (`npm run screenshots`).

## Release steps (only after the decisions above)

- [ ] Create the remote and push `main` (needs the owner).
- [ ] Tag `v3.0.0` (or the chosen first public version) on the release commit.
- [ ] Attach no binaries at first; build from source only.
- [ ] Post the announcement and credit Petri (needs the owner).
