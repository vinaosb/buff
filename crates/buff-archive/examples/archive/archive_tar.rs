// T39 example: roundtrip a directory through an uncompressed TAR.

use buff_archive::{Archive, Format};

fn main() {
    let root = std::env::temp_dir().join(format!(
        "buff_archive_tar_example-{}",
        std::process::id(),
    ));
    let src = root.join("src");
    let archive_path = root.join("out.tar");
    let extracted = root.join("extracted");

    std::fs::create_dir_all(src.join("sub")).expect("mkdir src/sub");
    std::fs::write(src.join("a.txt"), "alpha").expect("write a.txt");
    std::fs::write(src.join("sub/b.txt"), "beta").expect("write sub/b.txt");

    Archive::compress_dir(&src, &archive_path, Format::Tar).expect("compress_dir tar");
    println!(
        "compressed {} -> {} ({} bytes)",
        src.display(),
        archive_path.display(),
        std::fs::metadata(&archive_path).map(|m| m.len()).unwrap_or(0),
    );

    Archive::extract(&archive_path, &extracted).expect("extract tar");
    let recovered =
        std::fs::read_to_string(extracted.join("a.txt")).expect("read extracted a.txt");
    println!("recovered a.txt = {recovered:?}");

    let _ = std::fs::remove_dir_all(&root);
}
