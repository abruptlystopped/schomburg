# First run

Use `/Users/ketones/Documents/projects/schomburg` as the repository root, `/Users/ketones/Documents/projects` as a scan root, and `/Users/ketones/Documents/projects/schomburg/.schomburg/machine.sqlite3` as the machine database.

```sh
cargo run -p schomburg-cli -- init --db /Users/ketones/Documents/projects/schomburg/.schomburg/machine.sqlite3
cargo run -p schomburg-cli -- discover --root /Users/ketones/Documents/projects --db /Users/ketones/Documents/projects/schomburg/.schomburg/machine.sqlite3
cargo run -p schomburg-cli -- sources --db /Users/ketones/Documents/projects/schomburg/.schomburg/machine.sqlite3
cargo run -p schomburg-cli -- connect '<SOURCE_ID>' --db /Users/ketones/Documents/projects/schomburg/.schomburg/machine.sqlite3
cargo run -p schomburg-cli -- connections --db /Users/ketones/Documents/projects/schomburg/.schomburg/machine.sqlite3
cargo run -p schomburg-cli -- collect --db /Users/ketones/Documents/projects/schomburg/.schomburg/machine.sqlite3
cargo run -p schomburg-cli -- events --db /Users/ketones/Documents/projects/schomburg/.schomburg/machine.sqlite3
cargo run -p schomburg-cli -- event '<EVENT_ID>' --db /Users/ketones/Documents/projects/schomburg/.schomburg/machine.sqlite3
cargo run -p schomburg-cli -- pause '<CONNECTION_ID>' --db /Users/ketones/Documents/projects/schomburg/.schomburg/machine.sqlite3
cargo run -p schomburg-cli -- resume '<CONNECTION_ID>' --db /Users/ketones/Documents/projects/schomburg/.schomburg/machine.sqlite3
cargo run -p schomburg-cli -- disconnect '<CONNECTION_ID>' --db /Users/ketones/Documents/projects/schomburg/.schomburg/machine.sqlite3
```

Replace placeholders with actual long IDs printed by the CLI. Discovery imports nothing; only connected sources collect. Repeated collection is idempotent. Disconnect preserves prior evidence.
