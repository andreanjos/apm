fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../migrations");
    println!("cargo:rerun-if-changed=../../migrations/20260331000000_initial.sql");
    println!("cargo:rerun-if-changed=../../migrations/20260410000000_bundle_id_submissions.sql");
}
