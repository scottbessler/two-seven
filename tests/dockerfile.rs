const DOCKERFILE: &str = include_str!("../Dockerfile");
#[test]
fn same_debian_suite() {
    let images: Vec<_> = DOCKERFILE
        .lines()
        .filter(|l| l.trim_start().starts_with("FROM "))
        .collect();
    assert!(images.iter().all(|l| l.contains("bookworm")));
}
