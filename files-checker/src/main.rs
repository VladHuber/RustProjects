use clap::{arg, command, value_parser};
use files_checker::{
    TelegramNotifier, event_message, is_ignored, load_telegram_token, send_telegram,
    should_process_event,
};
use glob::Pattern;
use log::{error, info, warn};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Result, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Файл лога; события по нему самому игнорируются.
const LOG_FILE: &str = "app.log";
/// Окно дебаунса повторных событий, в секундах.
const DEBOUNCE_SECS: u64 = 3;
/// Пауза после обработки события, чтобы не обрабатывать шквал изменений.
const SLEEP_AFTER_EVENT: Duration = Duration::from_secs(2);
/// Локальный конфиг с токеном бота (в git не попадает, см. `.gitignore`).
const CONFIG_FILE: &str = "./config_app.json";

fn main() -> Result<()> {
    let matches = command!() // requires `cargo` feature
        // .arg(arg!([name] "Optional name to operate on"))
        .arg(
            arg!(
                --dir <DIR> "scanned dir "
            )
            .required(true)
            .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            arg!(
                --ignore <FILE> "ignore files pattern "
            )
            .required(true)
            .value_parser(value_parser!(String)),
        )
        .arg(
            arg!(
                --telegram <chat_id> "Telegram send status files "
            )
            .required(false)
            .value_parser(value_parser!(String)),
        )
        .get_matches();

    let Some(dir_scan) = matches.get_one::<PathBuf>("dir").cloned() else {
        eprintln!("Не указан каталог для наблюдения (--dir)");
        std::process::exit(2);
    };
    let Some(ignore_pattern) = matches.get_one::<String>("ignore").cloned() else {
        eprintln!("Не указан шаблон игнорирования (--ignore)");
        std::process::exit(2);
    };
    // Telegram строго опционален: без аргумента (или без рабочего конфига)
    // программа просто работает без уведомлений.
    let telegram_chat_id = matches.get_one::<String>("telegram").cloned();

    init_logger();

    let pattern = match Pattern::new(&ignore_pattern) {
        Ok(pattern) => pattern,
        Err(e) => {
            error!("Некорректный glob-шаблон {ignore_pattern:?}: {e}");
            std::process::exit(2);
        }
    };

    let notifier = build_notifier(telegram_chat_id.as_deref());

    let thread_handle = thread::current();
    let mut last_events: HashMap<String, u64> = HashMap::new();

    let mut watcher = RecommendedWatcher::new(
        move |res: Result<Event>| {
            match res {
                Ok(event) => {
                    let Some(path) = event.paths.first() else {
                        return;
                    };

                    if is_ignored(&pattern, path, LOG_FILE) {
                        return;
                    }

                    let Some(path_str) = path.to_str() else {
                        warn!("Пропущено событие с не-UTF-8 путём: {path:?}");
                        return;
                    };

                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|elapsed| elapsed.as_secs())
                        .unwrap_or_default();

                    // Проверяем, когда последний раз обрабатывали это событие
                    if !should_process_event(&mut last_events, path_str, now, DEBOUNCE_SECS) {
                        return;
                    }

                    let Some(log_message) = event_message(&event.kind, path_str) else {
                        return;
                    };

                    info!("{log_message}");
                    send_telegram(notifier.as_ref(), &log_message);
                    thread::sleep(SLEEP_AFTER_EVENT);
                    thread_handle.unpark();
                }
                Err(e) => error!("Ошибка наблюдения за каталогом: {e:?}"),
            }
        },
        Config::default(),
    )?;

    watcher.watch(Path::new(&dir_scan), RecursiveMode::Recursive)?;

    loop {
        thread::park(); // Останавливаем поток до следующего события

        println!("Поток разбужен!");
    }
}

/// Настроить логирование в stdout и в файл.
///
/// Если файл лога недоступен, логирование остаётся только в stdout:
/// это предупреждение, а не причина падать.
fn init_logger() {
    let dispatch = fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{} [{}] {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                message
            ))
        })
        .level(log::LevelFilter::Debug)
        .chain(std::io::stdout());

    let dispatch = match fern::log_file(LOG_FILE) {
        Ok(file) => dispatch.chain(file),
        Err(e) => {
            eprintln!("Не удалось открыть {LOG_FILE} для записи: {e}");
            dispatch
        }
    };

    if let Err(e) = dispatch.apply() {
        eprintln!("Не удалось инициализировать логгер: {e}");
    }
}

/// Собрать отправитель уведомлений, если он запрошен и настроен.
///
/// Любая проблема (нет аргумента `--telegram`, нет конфига, нет токена)
/// приводит к предупреждению в лог и работе без уведомлений.
fn build_notifier(chat_id: Option<&str>) -> Option<TelegramNotifier> {
    let Some(chat_id) = chat_id else {
        warn!("Аргумент --telegram не передан: уведомления в Telegram отключены");
        return None;
    };

    let token = match load_telegram_token(Path::new(CONFIG_FILE)) {
        Ok(token) => token,
        Err(e) => {
            warn!("Telegram-уведомления отключены: {e}");
            return None;
        }
    };

    match TelegramNotifier::new(token, chat_id.to_string()) {
        Ok(notifier) => {
            info!("Уведомления в Telegram включены");
            Some(notifier)
        }
        Err(e) => {
            warn!("Telegram-уведомления отключены: {e}");
            None
        }
    }
}
