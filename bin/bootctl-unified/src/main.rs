fn main() {
    if let Err(e) = bootctl_unified::run(generic_bootctl_backends::service) {
        eprintln!("generic-bootctl: {e}");
        std::process::exit(1);
    }
}
