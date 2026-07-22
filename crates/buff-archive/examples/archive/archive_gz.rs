// T39 example: roundtrip a directory through a .tar.gz (Tar + Gzip).
//
// Demonstrates the codec-wrapping-tar path: `compress_dir(Gz)` first
// packs the directory into a TAR, then Gzip-compresses the byte
// stream. `extract` auto-detects the format from the file extension
// and reverses the pipeline.

use buff_archive::{Archive, Format};

fn main() {
    let root = std::env::temp_dir().join(format!(
        "buff_archive_gz_example-{}",
        std::process::id(),
    ));
    let src = root.join("src");
    let archive_path = root.join("out.tar.gz");
    let extracted = root.join("extracted");

    std::fs::create_dir_all(src.join("sub")).expect("mkdir src/sub");
    std::fs::write(src.join("a.txt"), "alpha").expect("write a.txt");
    std::fs::write(src.join("sub/b.txt"), "beta").expect("write sub/b.txt");

    Archive::compress_dir(&src, &archive_path, Format::Gz).expect("compress_dir gz");
    println!(
        "compressed {} -> {} ({} bytes)",
        src.display(),
        archive_path.display(),
        std::fs::metadata(&archive_path).map(|m| m.len()).unwrap_or(0),
    );

    Archive::extract(&archive_path, &extracted).expect("extract gz");
    let recovered =
        std::fs::read_to_string(extracted.join("sub/b.txt")).expect("read extracted sub/b.txt");
    println!("recovered sub/b.txt = {recovered:?}");

    let _ = std::fs::remove_dir_all(&root);
}
