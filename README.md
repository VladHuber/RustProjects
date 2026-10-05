# RustProjects

Портфолио проектов на Rust (2025): шесть самостоятельных приложений, каждое — отдельный крейт
в своей папке со своим `Cargo.toml`. Общего workspace нет, проекты собираются независимо.

## Проекты

| Проект | Что это | Стек | Объём |
|---|---|---|---|
| [win_api](win_api) | Нативное Win32-приложение: монитор заряда Bluetooth-устройств. Окно, контролы и GDI — на сырых Win32-вызовах, устройства читаются из реестра и через SetupAPI. **Ни одного `unwrap()`** — ошибки через `Error::from_win32()`, 5 тестов | `windows` 0.58 | ~2200 строк, один файл |
| [blu_dev](blu_dev) | GUI-монитор BLE-устройств: асинхронный скан, чтение заряда из GATT-характеристики Battery Level (`0x2A19`), `ProgressBar`, иконка в трее, автозапуск через реестр | `btleplug`, `eframe`/`egui`, `tokio`, `tray-icon`, `winreg` | ~550 строк |
| [backup](backup) | CLI-бэкапер: SHA-256 контрольные суммы, архивы zip/tar.gz **через трейт-стратегию**, планировщик, защита от архивации в себя, 6 интеграционных тестов | `clap`, `clokwerk`, `sha2`, `zip`, `flate2`+`tar`, `walkdir` | ~300 строк, lib + bin |
| [files-checker](files-checker) | Демон-наблюдатель за каталогом: события ФС, дебаунс 3 с, логирование, опциональные уведомления в Telegram. Логика в библиотеке, **ни одного `unwrap`/`panic`**, 8 тестов офлайн | `clap`, `notify`, `fern`, `reqwest` | ~170 строк + lib |
| [log_analyzer](log_analyzer) | Анализатор `nginx access.log`: топы кодов/путей/IP, ASCII-гистограмма по часам, поиск аномалий 5xx, 6 тестов (включая «все 57 строк лога разбираются») | `regex`, `chrono` | ~200 строк |
| [dedup](dedup) | Поиск дубликатов файлов по содержимому: обход дерева, хеши BLAKE3, интерактивное удаление | `blake3`, `walkdir` | ~75 строк |

## Быстрый старт

Каждый проект запускается из своей папки:

```bash
cd log_analyzer && cargo run          # отчёт по test_data/access.log
cd backup && cargo run -- --src <откуда> --dest <куда> --schedule daily
cd files-checker && cargo run -- --dir "D:\Downloads" --ignore "*.tmp"
cd win_api && cargo run --release     # Windows: окно монитора Bluetooth
cd blu_dev && cargo run --release     # Windows: GUI со сканом BLE
cd dedup && cargo run                 # поиск дубликатов в foo/
```

Проверка любого проекта:

```bash
cd <проект>
cargo check
cargo test
```

## Что здесь можно посмотреть по темам

- **Win32 без фреймворков** — `win_api`: `CreateWindowExW`, цикл сообщений, `LISTBOX`, GDI, реестр
  (`RegEnumKeyExW`, `RegGetValueW`), SetupAPI (`SetupDiGetDevicePropertyW`), аккуратная обработка ошибок.
- **BLE и асинхронность в GUI** — `blu_dev`: `btleplug` + `tokio`, чтение GATT-характеристики,
  трей-иконка, автозапуск.
- **Абстракция через трейты и работа с архивами** — `backup`: `ArchiveStrategy` → `ZipStrategy`/`TarGzStrategy`,
  корректная финализация tar.gz, SHA-256, планировщик.
- **Реакция на события файловой системы и тестируемость** — `files-checker`: `notify` + дебаунс,
  вынос логики в `lib.rs`, тесты без сети.
- **Парсинг текстовых логов** — `log_analyzer`: строгий регексп с анкором и именованными группами,
  валидация времени, отчёт через `impl Display`.
- **Хеширование содержимого** — `dedup`: BLAKE3, обход дерева, интерактивный CLI.

## Требования

- Rust **1.85+** (во всех проектах `edition = "2024"`)
- `win_api` и `blu_dev` — только Windows (Win32 / Bluetooth), остальные кроссплатформенные
- сеть нужна только `files-checker` и лишь если включены уведомления в Telegram

## Структура

```
RustProjects/
├── README.md
├── .gitignore
├── backup/           README.md, src/{lib,main}.rs, tests/, examples/
├── blu_dev/          README.md, src/main.rs
├── dedup/            README.md, src/main.rs, foo/ (тестовые файлы)
├── files-checker/    README.md, src/{lib,main}.rs, tests/, config_app.example.json
├── log_analyzer/     README.md, src/main.rs, test_data/access.log
└── win_api/          README.md, src/main.rs
```

## Честные заметки

- Проекты учебные и делались в 2025 году, в том числе с разбором примеров и подсказок ИИ-ассистента —
  при этом все они собираются и запускаются локально, а их слабые места описаны в README каждого проекта.
- Уровень обработки ошибок намеренно не выровнен: в `win_api` и `files-checker` это аккуратный `Result`
  без `unwrap`, в `blu_dev` и `dedup` местами остались `unwrap()` — это отражено в README проектов.
- В репозиторий **не вошли** эксперименты и учебные копии примеров (веб-сервер из главы 20
  «The Rust Book» с `ThreadPool`, сэмпл Direct2D из `windows-rs`, заготовки на 20–50 строк) — они
  остались локально и не выдаются за самостоятельные работы.
- Заряд Bluetooth-устройств в `win_api` определяется эвристикой по реестру, а не GATT-характеристикой;
  подробности и ограничения — в его README.
