// sqlx::migrate! embeds ../../migrations at compile time; without this a new
// migration alone would not rebuild the binary (sqlx's documented build script).
fn main() {
    println!("cargo:rerun-if-changed=../../migrations");
}
