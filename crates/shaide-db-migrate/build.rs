fn main() {
    println!("cargo:rerun-if-changed=../shaide-db/migrations");
}
