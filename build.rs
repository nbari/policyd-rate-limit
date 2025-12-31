fn main() {
    if let Err(err) = built::write_built_file() {
        eprintln!("Failed to acquire build-time information: {err}");
        std::process::exit(1);
    }
}
