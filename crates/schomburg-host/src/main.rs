use std::io::{self, BufRead, Write};
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 || args[0] != "--db" {
        eprintln!("usage: schomburg-host --db <database-path>");
        std::process::exit(2);
    }
    let mut host = match schomburg_host::Host::open(&args[1]) {
        Ok(host) => host,
        Err(error) => {
            eprintln!("host startup failed: {error}");
            std::process::exit(1)
        }
    };
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for line in io::stdin().lock().lines() {
        match line {
            Ok(line) => {
                let response = host.handle_line(&line);
                if writeln!(out, "{response}")
                    .and_then(|_| out.flush())
                    .is_err()
                {
                    break;
                }
                if host.shutdown_requested() {
                    break;
                }
            }
            Err(error) => {
                eprintln!("stdin read failed: {error}");
                break;
            }
        }
    }
}
