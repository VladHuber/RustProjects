//! Библиотека бэкапера: стратегии архивации и общие вспомогательные функции.
//!
//! Сбор файлов вынесен в [`collect_files`], чтобы и стратегии архивации, и
//! подсчёт контрольных сумм использовали одну и ту же рекурсивную логику.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::write::GzEncoder;
use sha2::{Digest, Sha256};
use walkdir::WalkDir;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

/// Имя файла с контрольными суммами. Он всегда исключается из обхода,
/// чтобы не хешировать и не архивировать сам себя.
pub const CHECKSUMS_FILE_NAME: &str = "checksums.sha256";

/// Стратегия создания архива из содержимого каталога.
pub trait ArchiveStrategy {
    /// Создаёт архив из содержимого каталога `files_dir` в файл `output_path`.
    fn create_archive(&self, files_dir: &Path, output_path: &str) -> Result<(), String>;

    /// Проверяет, поддерживается ли формат.
    fn supports_format(&self, format: &str) -> bool;
}

/// Рекурсивно собирает пути ко всем обычным файлам внутри `dir`.
///
/// Каталоги обходятся `walkdir`, символические ссылки не разыменовываются
/// (это защищает от бесконечной рекурсии). Файл [`CHECKSUMS_FILE_NAME`]
/// пропускается, чтобы контрольные суммы не считались по самим себе.
/// Результат отсортирован — за счёт этого вывод детерминирован.
pub fn collect_files(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for entry in WalkDir::new(dir).follow_links(false) {
        let entry = entry.map_err(|e| io::Error::other(e.to_string()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.file_name().to_string_lossy() == CHECKSUMS_FILE_NAME {
            continue;
        }
        files.push(entry.into_path());
    }

    files.sort();
    Ok(files)
}

/// Путь файла внутри архива: путь относительно `base` с прямыми слэшами.
///
/// Такая запись переносима между платформами и сохраняет структуру каталогов
/// исходного дерева.
fn archive_entry_name(base: &Path, file: &Path) -> Result<String, String> {
    let relative = file.strip_prefix(base).map_err(|e| {
        format!(
            "не удалось вычислить путь {} относительно {}: {e}",
            file.display(),
            base.display()
        )
    })?;

    let mut name = String::new();
    for component in relative.components() {
        if !name.is_empty() {
            name.push('/');
        }
        name.push_str(&component.as_os_str().to_string_lossy());
    }

    if name.is_empty() {
        return Err(format!("пустое имя файла для {}", file.display()));
    }
    Ok(name)
}

/// Не даёт записать архив внутрь бэкапируемого каталога: иначе обход подхватит
/// недописанный архив и результат будет неполным.
fn ensure_output_outside_source(files_dir: &Path, output_path: &str) -> Result<(), String> {
    let output = Path::new(output_path);

    let canonical_source = fs::canonicalize(files_dir)
        .map_err(|e| format!("не удалось открыть каталог-источник {}: {e}", files_dir.display()))?;

    let parent = match output.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        // Путь без каталога — значит, файл создаётся в текущем каталоге.
        _ => Path::new("."),
    };
    let file_name = output
        .file_name()
        .ok_or_else(|| format!("в пути назначения нет имени файла: {output_path}"))?;
    // Приводим путь архива к абсолютному виду, чтобы сравнение не зависело
    // от того, задан `--src`/`--dest` абсолютно или относительно.
    let resolved_output = fs::canonicalize(parent)
        .map_err(|e| format!("не удалось открыть каталог назначения {}: {e}", parent.display()))?
        .join(file_name);

    if resolved_output.starts_with(&canonical_source) {
        return Err(format!(
            "архив {} нельзя создавать внутри каталога-источника {}",
            resolved_output.display(),
            canonical_source.display()
        ));
    }
    Ok(())
}

/// Записывает SHA-256 контрольные суммы всех файлов каталога `files_dir`
/// в файл `output`.
///
/// Обход рекурсивный (через [`collect_files`]) и не включает сам файл сумм,
/// поэтому повторный запуск не портит результат. Формат строки —
/// `sha256<два пробела><путь относительно files_dir>`, как у `sha256sum`.
pub fn save_checksums(files_dir: &Path, output: &Path) -> io::Result<()> {
    let files = collect_files(files_dir)?;

    let mut checksums = File::create(output)?;
    for path in &files {
        let hash = sha256_file(path)?;
        let name = path
            .strip_prefix(files_dir)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        writeln!(checksums, "{hash}  {name}")?;
    }
    checksums.flush()?;

    Ok(())
}

/// Вычисляет SHA-256 файла и возвращает его в шестнадцатеричном виде.
pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 1024];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

/// Архивация в zip.
pub struct ZipStrategy;

impl ArchiveStrategy for ZipStrategy {
    fn create_archive(&self, files_dir: &Path, output_path: &str) -> Result<(), String> {
        ensure_output_outside_source(files_dir, output_path)?;

        let files = collect_files(files_dir).map_err(|e| e.to_string())?;

        let file = File::create(output_path).map_err(|e| e.to_string())?;
        let mut zip = ZipWriter::new(file);

        for path in &files {
            let name = archive_entry_name(files_dir, path)?;
            zip.start_file(name, SimpleFileOptions::default())
                .map_err(|e| e.to_string())?;

            let data = fs::read(path).map_err(|e| e.to_string())?;
            zip.write_all(&data).map_err(|e| e.to_string())?;
        }

        // Финализация: записывает central directory и закрывает архив.
        zip.finish().map_err(|e| e.to_string())?;
        Ok(())
    }

    fn supports_format(&self, format: &str) -> bool {
        format.eq_ignore_ascii_case("zip")
    }
}

/// Архивация в tar.gz.
pub struct TarGzStrategy;

impl ArchiveStrategy for TarGzStrategy {
    fn create_archive(&self, files_dir: &Path, output_path: &str) -> Result<(), String> {
        ensure_output_outside_source(files_dir, output_path)?;

        let files = collect_files(files_dir).map_err(|e| e.to_string())?;

        let tar_gz = File::create(output_path).map_err(|e| e.to_string())?;
        let enc = GzEncoder::new(tar_gz, Compression::default());
        let mut tar = tar::Builder::new(enc);

        for path in &files {
            let name = archive_entry_name(files_dir, path)?;
            let mut file = File::open(path).map_err(|e| e.to_string())?;
            // Имя внутри архива задаём явно, чтобы сохранить структуру
            // относительно `files_dir`.
            tar.append_file(name, &mut file).map_err(|e| e.to_string())?;
        }

        // Финализация tar: дописывает завершающие блоки, забирает GzEncoder,
        // затем закрываем gzip-поток (это дописывает trailer gzip) и файл.
        tar.finish().map_err(|e| e.to_string())?;
        let encoder = tar.into_inner().map_err(|e| e.to_string())?;
        let mut file = encoder.finish().map_err(|e| e.to_string())?;
        file.flush().map_err(|e| e.to_string())?;

        Ok(())
    }

    fn supports_format(&self, format: &str) -> bool {
        format.eq_ignore_ascii_case("tar.gz") || format.eq_ignore_ascii_case("tgz")
    }
}
