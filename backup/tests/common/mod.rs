//! Вспомогательные функции для интеграционных тестов: временные каталоги и
//! чтение архивов. Тесты работают только в изолированных временных каталогах
//! и не зависят от файлов внутри репозитория.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use flate2::read::GzDecoder;
use tar::Archive;
use zip::ZipArchive;

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Уникальный временный каталог (имя содержит pid и счётчик).
pub fn temp_dir(tag: &str) -> PathBuf {
    let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("backup-it-{}-{tag}-{unique}", std::process::id()));
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("не удалось очистить временный каталог");
    }
    fs::create_dir_all(&dir).expect("не удалось создать временный каталог");
    dir
}

/// Записывает файл, создавая при необходимости все родительские каталоги.
pub fn write_file(root: &Path, relative: &str, content: &str) -> PathBuf {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("не удалось создать каталог");
    }
    fs::write(&path, content).expect("не удалось записать файл");
    path
}

/// Имена файлов внутри zip-архива (отсортированы).
pub fn zip_names(path: &Path) -> Vec<String> {
    let file = File::open(path).expect("не удалось открыть zip-архив");
    let mut archive = ZipArchive::new(file).expect("zip-архив не читается");
    let mut names = Vec::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index).expect("не удалось прочитать запись zip");
        if entry.is_file() {
            names.push(entry.name().to_string());
        }
    }
    names.sort();
    names
}

/// Содержимое файла внутри zip-архива.
pub fn zip_read_to_string(path: &Path, name: &str) -> String {
    let file = File::open(path).expect("не удалось открыть zip-архив");
    let mut archive = ZipArchive::new(file).expect("zip-архив не читается");
    let mut entry = archive.by_name(name).expect("файл не найден в zip-архиве");
    let mut content = String::new();
    entry.read_to_string(&mut content).expect("не удалось прочитать файл из zip");
    content
}

/// Пары (имя, содержимое) для обычных файлов tar.gz-архива.
pub fn tar_gz_entries(path: &Path) -> Vec<(String, String)> {
    let file = File::open(path).expect("не удалось открыть tar.gz-архив");
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);
    let mut entries = Vec::new();

    for entry in archive.entries().expect("tar.gz-архив не читается") {
        let mut entry = entry.expect("не удалось прочитать запись tar");
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let name = entry
            .path()
            .expect("не удалось прочитать путь записи tar")
            .to_string_lossy()
            .replace('\\', "/");
        let mut content = String::new();
        entry.read_to_string(&mut content).expect("не удалось прочитать файл из tar");
        entries.push((name, content));
    }

    entries.sort();
    entries
}

/// Удаляет временный каталог.
pub fn cleanup(dir: &Path) {
    let _ = fs::remove_dir_all(dir);
}
