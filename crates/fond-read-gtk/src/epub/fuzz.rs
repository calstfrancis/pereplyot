use crate::fuzz_tests::Rng;

#[test]
#[ignore]
fn fuzz_epub_markup() {
    let seeds: u64 = std::env::var("FUZZ_SEEDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20000);
    for seed in 1..=seeds {
        let result = std::panic::catch_unwind(|| {
            let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let html = r.text(60);
            let id = r.text(2);
            let _ = super::footnotes::note_text_in(&html, &id);
            let _ = super::pages::tags(&html).count();
            for (tag, _) in super::pages::tags(&html) {
                let _ = super::pages::attr(tag, "id");
                let _ = super::pages::attr(tag, "epub:type");
            }
            let _ = super::pages::resolve(std::path::Path::new("OEBPS/text"), &r.text(10));
        });
        assert!(result.is_ok(), "epub markup panicked for seed {seed}");
    }
}
