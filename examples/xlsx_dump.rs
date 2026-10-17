//! Print every sheet of an .xlsx as tab-separated cells with their types: `cargo run --example xlsx_dump FILE`.
fn main() {
    let path = std::env::args().nth(1).expect("usage: xlsx_dump FILE.xlsx");
    let b = std::fs::read(&path).expect("read");
    let wb = omni_spec::ooxml::xlsx::read(&b).expect("xlsx");
    for s in &wb.sheets {
        println!("== {} ({})", s.name, s.state);
        for r in &s.rows {
            println!("{}", r.iter().map(|c| format!("{c:?}")).collect::<Vec<_>>().join("\t"));
        }
    }
}
