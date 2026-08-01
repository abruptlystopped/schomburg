# Troubleshooting

- If `rustc` or `cargo` is missing, install Rust with rustup and reload the shell environment.
- Cargo dependency downloads need network access.
- `connect '<SOURCE_ID>'` needs a real ID copied from `sources`, not the literal placeholder.
- Use `connections` to inspect connection state and last error; use `git status` before changing the repository.
- Old disposable development databases may contain Git events from before presentation metadata changes. Reset only a disposable `.schomburg/dev.sqlite3`; do not confuse it with the machine database.
- npm authentication, remote configuration, divergent histories, and README merge conflicts are Git/npm workflow issues, separate from the local Rust CLI.
