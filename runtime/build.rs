// rstest's `#[files]` glob in tests/compiler_tests.rs is expanded at compile
// time, so without this cargo has no reason to rebuild when a .eta is added and
// the new test is silently ignored.
fn main() {
    println!("cargo:rerun-if-changed=tests/integration_tests");
}
