use clap::{arg, command, value_parser};

use std::io;
use std::path::{Path, PathBuf};

use clokwerk::{Scheduler, TimeUnits};
use std::thread;
use std::time::Duration;

use backup::{ArchiveStrategy, CHECKSUMS_FILE_NAME, TarGzStrategy, ZipStrategy, save_checksums};

fn main() -> io::Result<()> {
    let matches = command!()
        .arg(
            arg!(
                --src <FILE> "FROM dir "
            )
            .required(true)
            .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            arg!(
                --dest <FILE> "TO dir "
            )
            .required(true)
            .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            arg!(
                --schedule <daily> "TO schedule "
            )
            .required(false)
            .default_value("daily")
            .value_parser(value_parser!(String)),
        )
        .get_matches();

    let src_path = matches
        .get_one::<PathBuf>("src")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Source path not provided"))?
        .clone();
    let dest_path = matches
        .get_one::<PathBuf>("dest")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Destination path not provided"))?
        .clone();

    if !src_path.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("каталог-источник не найден: {}", src_path.display()),
        ));
    }

    let archive_format = default_format();
    // `Path::join` сам подставляет разделитель; `format!("{}{}", ...)` его терял.
    let backup_path = dest_path.join(archive_file_name(archive_format, unix_stamp()));
    let backup_path = backup_path
        .to_str()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "путь назначения не является корректным UTF-8: {}",
                    backup_path.display()
                ),
            )
        })?
        .to_string();
    let checksums_path = src_path.join(CHECKSUMS_FILE_NAME);

    let schedule_command = matches
        .get_one::<String>("schedule")
        .cloned()
        .unwrap_or_else(|| "daily".to_string());
    let count_days = match schedule_command.as_str() {
        "daily" => 1,
        "weekly" => 7,
        _ => 1,
    };
    println!("Таймер выставлен на {} дней", count_days);

    // Первый бэкап делаем сразу: запланированная задача сработает только через
    // весь интервал (для daily — через сутки), а результат нужен уже сейчас.
    match run_backup(&src_path, &backup_path, &checksums_path, archive_format) {
        Ok(()) => println!("Бэкап создан: {backup_path}"),
        Err(e) => eprintln!("Ошибка бэкапа: {e}"),
    }

    let mut scheduler = Scheduler::new();
    scheduler.every(count_days.days()).run(move || {
        println!("Запущен процесс");
        match run_backup(&src_path, &backup_path, &checksums_path, archive_format) {
            Ok(()) => println!("Бэкап создан: {backup_path}"),
            Err(e) => eprintln!("Ошибка бэкапа: {e}"),
        }
    });

    // Бесконечный цикл выполнения
    loop {
        scheduler.run_pending();
        thread::sleep(Duration::from_millis(100));
    }
}

/// Одноразовый бэкап: контрольные суммы + архив.
fn run_backup(
    src_path: &Path,
    backup_path: &str,
    checksums_path: &Path,
    format: ArchiveFormat,
) -> io::Result<()> {
    save_checksums(src_path, checksums_path)?;

    let strategy = get_archive_strategy(format);
    strategy
        .create_archive(src_path, backup_path)
        .map_err(|e| io::Error::other(e.to_string()))
}

/// Формат архива по умолчанию для текущей платформы.
fn default_format() -> ArchiveFormat {
    if cfg!(target_os = "windows") {
        ArchiveFormat::Zip
    } else {
        ArchiveFormat::TarGz
    }
}

fn archive_file_name(format: ArchiveFormat, stamp: u64) -> String {
    match format {
        ArchiveFormat::Zip => format!("backup-{stamp}.zip"),
        ArchiveFormat::TarGz => format!("backup-{stamp}.tar.gz"),
    }
}

/// Метка времени (секунды UNIX) для имени архива; при сбое часов — 0.
fn unix_stamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Clone, Copy)]
pub enum ArchiveFormat {
    Zip,
    TarGz,
}

pub fn get_archive_strategy(format: ArchiveFormat) -> Box<dyn ArchiveStrategy> {
    match format {
        ArchiveFormat::Zip => Box::new(ZipStrategy),
        ArchiveFormat::TarGz => Box::new(TarGzStrategy),
    }
}
