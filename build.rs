fn main() {
    println!("cargo:rerun-if-changed=src/research/migrations");
    println!("cargo:rerun-if-changed=src/review/migrations");
}
