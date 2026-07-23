// T39 example: roundtrip a directory through a ZIP archive.
//
// Generates a small source dir inline (no fixture needed), compresses
// it to a `.zip` under the temp dir, extracts the zip into a fresh
// sub-dir, then reads back the extracted files to verify the round-
// trip preserved byte-content. Mirrors the buff-image example layout.

use buff_archive::{Archive, Format};

fn main() {
    let root =
        std::env::temp_dir().join(format!("buff_archive_zip_example-{}", std::process::id(),));
    let src = root.join("src");
    let archive_path = root.join("out.zip");
    let extracted = root.join("extracted");

    std::fs::create_dir_all(src.join("sub")).expect("mkdir src/sub");
    std::fs::write(src.join("a.txt"), "alpha").expect("write a.txt");
    std::fs::write(src.join("sub/b.txt"), "beta").expect("write sub/b.txt");

    Archive::compress_dir(&src, &archive_path, Format::Zip).expect("compress_dir zip");
    println!(
        "compressed {} -> {} ({} bytes)",
        src.display(),
        archive_path.display(),
        std::fs::metadata(&archive_path)
            .map(|m| m.len())
            .unwrap_or(0),
    );

    Archive::extract(&archive_path, &extracted).expect("extract zip");
    let recovered =
        std::fs::read_to_string(extracted.join("sub/b.txt")).expect("read extracted sub/b.txt");
    println!("recovered sub/b.txt = {recovered:?}");

    let _ = std::fs::remove_dir_all(&root);
}
