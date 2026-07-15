fn main() {
    // SQLx embeds migrations at compile time. Tell Cargo to rebuild when a
    // migration is added or edited so local watch mode picks it up reliably.
    println!("cargo:rerun-if-changed=migrations");
}
