use async_fs as afs;
use async_zip::{Compression, ZipEntryBuilder};
use futures_lite::future::block_on;
use futures_lite::io::Cursor;
use std::process::Command;

#[test]
fn prints_first_five_lines_from_bms_in_zip() {
    let mut path = std::env::temp_dir();
    path.push(format!("nebula_tunes_test_{}_zip.zip", std::process::id()));

    let bytes = block_on(async {
        let cursor = Cursor::new(Vec::new());
        let mut writer = async_zip::base::write::ZipFileWriter::new(cursor);
        let entry1 = ZipEntryBuilder::new("song1.bms".into(), Compression::Stored);
        writer
            .write_entry_whole(entry1, b"A\nB\nC\nD\nE\nF\n")
            .await
            .unwrap();
        let entry2 = ZipEntryBuilder::new("other.txt".into(), Compression::Stored);
        writer
            .write_entry_whole(entry2, b"x\ny\nz\n")
            .await
            .unwrap();
        let cursor = writer.close().await.unwrap();
        cursor.into_inner()
    });
    block_on(afs::write(&path, &bytes)).unwrap();

    let bin = env!("CARGO_BIN_EXE_nebula-tunes");
    let out = Command::new(bin).arg(&path).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let expected = format!(
        "{}\n{}\nA\nB\nC\nD\nE\n",
        path.to_string_lossy(),
        "song1.bms"
    );
    assert_eq!(stdout, expected);

    let _ = block_on(afs::remove_file(&path));
}
