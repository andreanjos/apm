fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../migrations");
    println!("cargo:rerun-if-changed=../../migrations/20260331000000_initial.sql");
    println!("cargo:rerun-if-changed=../../migrations/20260401090000_authentication.sql");
    println!("cargo:rerun-if-changed=../../migrations/20260401100000_purchasing.sql");
    println!("cargo:rerun-if-changed=../../migrations/20260402000000_licenses.sql");
    println!("cargo:rerun-if-changed=../../migrations/20260402010000_discovery_storefront.sql");
    println!("cargo:rerun-if-changed=../../migrations/20260402020000_agent_purchase_policies.sql");
}
