#[test]
fn compile() {
    let t = trybuild::TestCases::new();
    t.pass("tests/00-userdata.rs");
    t.compile_fail("tests/01-userdata-error.rs");
}
