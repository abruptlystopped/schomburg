# Dogfood

```sh
cargo run -p schomburg-cli -- record generate --db /Users/ketones/Documents/projects/schomburg/.schomburg/machine.sqlite3 --folder '/Users/ketones/Documents/Schomburg Record'
find '/Users/ketones/Documents/Schomburg Record' -type f
cargo run -p schomburg-cli -- record open --folder '/Users/ketones/Documents/Schomburg Record'
```
