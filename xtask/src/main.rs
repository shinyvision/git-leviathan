fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("gen-stubs") => println!("gen-stubs: populated in Task 3.2"),
        Some("gen-docs")  => println!("gen-docs: populated in Task 6.3"),
        _ => {
            eprintln!("usage: cargo xtask <gen-stubs|gen-docs>");
            std::process::exit(2);
        }
    }
}
