fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match schomburg_cli::execute(&arguments) {
        Ok(output) => print!("{output}"),
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    }
}
