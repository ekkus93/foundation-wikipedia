//! Future desktop topic-pack builder CLI; operations are not implemented yet.
use std::env;

fn main() {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("--help" | "-h") | None => {
            println!("wiki-pack: Foundation Wikipedia topic-pack builder (bootstrap)");
            println!("Usage: wiki-pack --help");
            println!("Pack import/build/verify/export are not yet implemented.");
        }
        Some(flag) => {
            eprintln!("Unsupported operation: {flag}. Pack commands are not yet implemented.");
            std::process::exit(2);
        }
    }
}
