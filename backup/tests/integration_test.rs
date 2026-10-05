//! Интеграционные тесты бэкапера: реальное создание архивов во временных
//! каталогах, проверка имён/содержимого файлов внутри архива и корректности
//! записанных контрольных сумм.

mod common;

use std::error::Error;
use std::fs;

use backup::{
    ArchiveStrategy, CHECKSUMS_FILE_NAME, TarGzStrategy, ZipStrategy, collect_files,
    save_checksums,
};

use common::{cleanup, tar_gz_entries, temp_dir, write_file, zip_names, zip_read_to_string};

/// Готовит источник: два файла в корне и один во вложенном каталоге.
fn prepare_source(tag: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let root = temp_dir(tag);
    let src = root.join("src");
    let dest = root.join("dest");
    fs::create_dir_all(&src).expect("не удалось создать src");
    fs::create_dir_all(&dest).expect("не удалось создать dest");

    write_file(&src, "alpha.txt", "alpha");
    write_file(&src, "beta.txt", "beta");
    write_file(&src, "nested/gamma.txt", "gamma");

    (src, dest)
}

#[test]
fn collect_files_is_recursive_and_skips_checksums() -> Result<(), Box<dyn Error>> {
    let (src, dest) = prepare_source("collect");

    let files = collect_files(&src)?;
    let names: Vec<String> = files
        .iter()
        .map(|p| p.strip_prefix(&src).unwrap().to_string_lossy().replace('\\', "/"))
        .collect();
    assert_eq!(names, vec!["alpha.txt", "beta.txt", "nested/gamma.txt"]);

    // Появление файла сумм не должно менять результат обхода.
    fs::write(src.join(CHECKSUMS_FILE_NAME), "мусор")?;
    let after = collect_files(&src)?;
    assert_eq!(after.len(), 3, "файл сумм не должен попадать в обход");
    assert!(
        !after.iter().any(|p| p.file_name().unwrap() == CHECKSUMS_FILE_NAME),
        "checksums.sha256 исключён из обхода"
    );

    cleanup(&dest);
    cleanup(src.parent().unwrap());
    Ok(())
}

#[test]
fn zip_strategy_creates_readable_archive_with_structure() -> Result<(), Box<dyn Error>> {
    let (src, dest) = prepare_source("zip");
    let archive = dest.join("backup.zip");
    let archive = archive.to_str().ok_or("некорректный путь архива")?;

    ZipStrategy.create_archive(&src, archive)?;

    let metadata = fs::metadata(archive)?;
    assert!(metadata.len() > 0, "zip-архив не должен быть пустым");

    assert_eq!(zip_names(std::path::Path::new(archive)), vec![
        "alpha.txt",
        "beta.txt",
        "nested/gamma.txt"
    ]);
    assert_eq!(zip_read_to_string(std::path::Path::new(archive), "nested/gamma.txt"), "gamma");

    cleanup(src.parent().unwrap());
    Ok(())
}

#[test]
fn tar_gz_strategy_finishes_archive() -> Result<(), Box<dyn Error>> {
    let (src, dest) = prepare_source("targz");
    let archive = dest.join("backup.tar.gz");
    let archive = archive.to_str().ok_or("некорректный путь архива")?;

    TarGzStrategy.create_archive(&src, archive)?;

    let metadata = fs::metadata(archive)?;
    // Признак незавершённого gzip-потока: трейлер (8 байт) не дописан.
    assert!(metadata.len() > 8, "gzip-трейлер не дописан, архив не финализирован");
    let raw = fs::read(archive)?;
    assert!(raw.len() >= 2, "архив слишком мал");
    assert_eq!(&raw[..2], &[0x1f, 0x8b], "нет gzip-заголовка");

    let entries = tar_gz_entries(std::path::Path::new(archive));
    let names: Vec<String> = entries.iter().map(|(name, _)| name.clone()).collect();
    assert_eq!(names, vec!["alpha.txt", "beta.txt", "nested/gamma.txt"]);
    let gamma = entries
        .iter()
        .find(|(name, _)| name == "nested/gamma.txt")
        .map(|(_, content)| content.clone())
        .unwrap_or_default();
    assert_eq!(gamma, "gamma");

    cleanup(src.parent().unwrap());
    Ok(())
}

#[test]
fn refuses_to_create_archive_inside_source() -> Result<(), Box<dyn Error>> {
    let (src, _dest) = prepare_source("guard");
    let inside = src.join("backup.zip");
    let inside = inside.to_str().ok_or("некорректный путь архива")?;

    let result = ZipStrategy.create_archive(&src, inside);
    assert!(result.is_err(), "архив внутри источника должен отвергаться");

    cleanup(src.parent().unwrap());
    Ok(())
}

#[test]
fn checksums_file_is_correct_and_excludes_itself() -> Result<(), Box<dyn Error>> {
    let (src, dest) = prepare_source("hashes");
    let checksums = src.join(CHECKSUMS_FILE_NAME);

    // Прогон дважды: второй раз файл сумм уже существует в источнике.
    save_checksums(&src, &checksums)?;
    save_checksums(&src, &checksums)?;

    let content = fs::read_to_string(&checksums)?;
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 3, "ожидаются ровно 3 файла источника: {content}");

    // Формат `sha256  <путь относительно src>` с прямыми слэшами.
    let expected = [
        (sha256_hex(b"alpha"), "alpha.txt"),
        (sha256_hex(b"beta"), "beta.txt"),
        (sha256_hex(b"gamma"), "nested/gamma.txt"),
    ];
    for (hash, name) in expected {
        let line = format!("{hash}  {name}");
        assert!(lines.contains(&line.as_str()), "нет строки `{line}` в:\n{content}");
    }
    assert!(
        lines.iter().all(|line| !line.contains(CHECKSUMS_FILE_NAME)),
        "файл сумм не должен хешировать сам себя:\n{content}"
    );

    cleanup(&dest);
    cleanup(src.parent().unwrap());
    Ok(())
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(data))
}

#[test]
fn strategies_report_supported_formats() {
    let zip = ZipStrategy;
    let tar_gz = TarGzStrategy;

    assert!(zip.supports_format("zip"));
    assert!(zip.supports_format("ZIP"));
    assert!(!zip.supports_format("tar.gz"));

    assert!(tar_gz.supports_format("tar.gz"));
    assert!(tar_gz.supports_format("TGZ"));
    assert!(!tar_gz.supports_format("zip"));
}
