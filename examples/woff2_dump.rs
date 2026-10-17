//! woff2_dump FILE.woff2 OUTDIR: write the decoded font as OUTDIR/font.ttf (our `to_sfnt`) and every rebuilt table as
//! OUTDIR/<tag>.bin — the input of tools/check_woff2_vs_fonttools.py (black-box oracle, glyph-level comparison).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let w = omni_spec::woff2::decode(&std::fs::read(&a[1]).unwrap(), 256 << 20).unwrap();
    std::fs::create_dir_all(&a[2]).unwrap();
    std::fs::write(format!("{}/font.ttf", a[2]), omni_spec::woff2::to_sfnt(&w).unwrap()).unwrap();
    for t in &w.tables {
        std::fs::write(format!("{}/{}.bin", a[2], String::from_utf8_lossy(&t.tag).replace('/', "_").replace(' ', "_")), &t.data).unwrap();
    }
}
