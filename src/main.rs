use std::process;

use colored::*;

fn main() {
    let request =
        git_chain::cli::try_parse_from(std::env::args_os()).unwrap_or_else(|error| error.exit());

    match git_chain::run(request) {
        Ok(()) => {}
        Err(err) => {
            eprintln!("{} {}", "error:".red().bold(), err);
            process::exit(1);
        }
    }
}
