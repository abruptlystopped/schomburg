# Dogfood

```sh
cargo run -p schomburg-cli -- record generate --db /Users/ketones/Documents/projects/schomburg/.schomburg/machine.sqlite3 --folder '/Users/ketones/Documents/Schomburg Record'
find '/Users/ketones/Documents/Schomburg Record' -type f
cargo run -p schomburg-cli -- record open --folder '/Users/ketones/Documents/Schomburg Record'
```

Configure future reconciliation without starting it:

```sh
cargo run -p schomburg-cli -- record-folder set --folder '/Users/ketones/Documents/Schomburg Record' --db /Users/ketones/Documents/projects/schomburg/.schomburg/machine.sqlite3
cargo run -p schomburg-cli -- schedule set --time 18:00 --days weekdays --db /Users/ketones/Documents/projects/schomburg/.schomburg/machine.sqlite3
cargo run -p schomburg-cli -- monitoring on --db /Users/ketones/Documents/projects/schomburg/.schomburg/machine.sqlite3
cargo run -p schomburg-cli -- schedule show --db /Users/ketones/Documents/projects/schomburg/.schomburg/machine.sqlite3
```
