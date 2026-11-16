# AGENTS.md

Copybook is a keyboard-first clipboard history for Windows 10/11, built with Tauri v2.
Windows only: no macOS or Linux code, dependencies or docs.

## Who owns what

| Area | Owner | Paths |
| --- | --- | --- |
| Frontend: UI, styles, icons, anything about how things look | Claude | `src/`, `*.html`, `vite.config.ts`, `tsconfig.json`, `package.json`, `src-tauri/icons/` |
| Backend: Rust, Tauri config, capabilities, tests, CI | Codex | `src-tauri/` (except `icons/`), `.github/` |
| The contract between them | Claude writes, both follow | `docs/contract.md` |

`docs/contract.md` is the single source of truth for commands, events, types, settings and
behaviour. If the contract is wrong or incomplete, stop and say so in your report instead of
improvising a different interface. The TypeScript mirror is `src/shared/contract.ts`.

## Rules for every Codex task

* Never `git push`, never touch remotes, never ssh / deploy anywhere.
* Do not commit. Leave all changes in the working tree; Claude reviews with `git diff`.
* Change nothing outside this repository. Do not edit frontend files (table above). If the
  backend needs something from the frontend, write it in your report.
* Write original code. EcoPaste (Apache-2.0) may be read for approach, not copied.
* Before reporting, run and include the commands and their real output (summarised when long):
  `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, plus any
  build or smoke test the task asks for. "Tests passed" without output does not count.

## Backend conventions

* Rust stable, edition 2021, crate `copybook` (lib `copybook_lib`) in `src-tauri/`.
* `#[tauri::command]` functions stay thin: validate, call into modules, map errors to
  `CmdError { code, message }`.
* No `unwrap()`/`expect()` on anything that can fail at runtime (I/O, clipboard, Win32,
  SQLite, user input). Log with `log::` and keep the app running.
* Win32 via the `windows` crate. Keep `unsafe` blocks small, each with a comment on why it
  is sound.
* SQLite via `rusqlite` with the `bundled` feature (FTS5 + trigram available). Schema changes
  go through numbered migrations, never by editing a released migration.
* Unit-test pure logic (detection, secrets, search parsing, settings validation, storage on a
  temp dir). Tests that need a real desktop clipboard are `#[ignore]` and run locally with
  `cargo test -- --ignored`.

## Commands

```bash
npm install                      # frontend deps (Node 24, npm)
npm run dev                      # Vite only, browser preview with mock data
npm run tauri dev                # full app
npm run tauri build              # NSIS installer in src-tauri/target/release/bundle/nsis
cd src-tauri && cargo test       # backend tests
```
