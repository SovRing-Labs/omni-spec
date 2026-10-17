//! Print the document model of a .docx: `cargo run --example docx_dump FILE`.
fn main() {
    let path = std::env::args().nth(1).expect("usage: docx_dump FILE.docx");
    let d = omni_spec::ooxml::docx::read(&std::fs::read(&path).expect("read")).expect("docx");
    println!("title: {:?}", d.title);
    for b in &d.blocks {
        println!("{b:?}");
    }
    for (i, im) in d.images.iter().enumerate() {
        println!("image {i}: {} {} {} bytes alt={:?} {}x{} emu", im.name, im.content_type, im.data.len(), im.alt, im.width_emu, im.height_emu);
    }
    for (i, n) in d.notes.iter().enumerate() {
        println!("note {}: {n:?}", i + 1);
    }
}
