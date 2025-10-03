use std::{
    io::{self, Write},
};

use structopt::StructOpt;

/*#[derive(StructOpt, Debug)]
struct Cli {
    pattern: String,
    path: String,
}*/

fn main() {
    let mut stdout = io::stdout();

    //let args = Cli::from_args();

    writeln!(stdout, "Hello!");

}