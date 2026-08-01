# macOS Dogfood

Use explicit development paths when launching the shell:

```sh
SCHOMBURG_HOST_PATH="$PWD/target/debug/schomburg-host" SCHOMBURG_DB_PATH="$PWD/.schomburg/machine.sqlite3" swift run --package-path apps/schomburg-macos
```
