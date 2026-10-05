//! Интеграционные тесты для чистой логики наблюдателя.
//!
//! Тесты не требуют сети, токенов и файла `test.json`: проверяются
//! фильтрация путей по glob, дебаунс и формирование текста уведомления.

use files_checker::{event_message, is_ignored, should_process_event};
use glob::Pattern;
use notify::event::{AccessKind, CreateKind, EventKind, ModifyKind, RemoveKind};
use std::collections::HashMap;
use std::path::Path;

#[test]
fn ignores_paths_matching_glob_pattern() {
    let pattern = Pattern::new("*.tmp").expect("корректный glob-шаблон");

    assert!(is_ignored(
        &pattern,
        Path::new("C:/tmp/note.tmp"),
        "app.log"
    ));
    assert!(is_ignored(&pattern, Path::new("note.tmp"), "app.log"));
    assert!(!is_ignored(
        &pattern,
        Path::new("C:/tmp/note.txt"),
        "app.log"
    ));
}

#[test]
fn ignores_the_log_file_itself() {
    let pattern = Pattern::new("*.tmp").expect("корректный glob-шаблон");

    // Сам лог не подходит под glob, но должен игнорироваться по имени файла,
    // иначе запись в лог порождает новые события.
    assert!(is_ignored(&pattern, Path::new("app.log"), "app.log"));
    assert!(is_ignored(
        &pattern,
        Path::new("C:/work/app.log"),
        "app.log"
    ));
    assert!(!is_ignored(
        &pattern,
        Path::new("C:/work/other.log"),
        "app.log"
    ));
}

#[test]
fn debounce_processes_first_event_and_skips_repeats() {
    let mut last_events = HashMap::new();
    let path = "C:/work/file.txt";

    assert!(should_process_event(&mut last_events, path, 1_000, 3));
    // Внутри окна дебаунса событие отбрасывается.
    assert!(!should_process_event(&mut last_events, path, 1_001, 3));
    assert!(!should_process_event(&mut last_events, path, 1_003, 3));
    // Спустя окно событие снова обрабатывается.
    assert!(should_process_event(&mut last_events, path, 1_004, 3));
}

#[test]
fn debounce_keeps_paths_independent() {
    let mut last_events = HashMap::new();

    assert!(should_process_event(&mut last_events, "a.txt", 10, 3));
    // Другой путь не блокируется дебаунсом первого.
    assert!(should_process_event(&mut last_events, "b.txt", 10, 3));
}

#[test]
fn debounce_does_not_overflow_on_clock_skew() {
    let mut last_events = HashMap::from([("a.txt".to_string(), 1_000_u64)]);

    // Время "назад" (например, перевод часов) не должно паниковать.
    assert!(!should_process_event(&mut last_events, "a.txt", 500, 3));
}

#[test]
fn event_message_formats_notifications() {
    assert_eq!(
        event_message(&EventKind::Create(CreateKind::File), "C:/work/a.txt").as_deref(),
        Some("Добавлен файл: C:/work/a.txt")
    );
    assert_eq!(
        event_message(&EventKind::Modify(ModifyKind::Any), "C:/work/a.txt").as_deref(),
        Some("Изменен файл: C:/work/a.txt")
    );
    assert_eq!(
        event_message(&EventKind::Remove(RemoveKind::File), "C:/work/a.txt").as_deref(),
        Some("Удален файл: C:/work/a.txt")
    );
}

#[test]
fn event_message_is_empty_for_uninteresting_events() {
    assert_eq!(
        event_message(&EventKind::Access(AccessKind::Any), "a.txt"),
        None
    );
}

#[test]
fn missing_config_reports_error_instead_of_panicking() {
    let err = files_checker::load_telegram_token(Path::new("./does-not-exist.json"))
        .expect_err("отсутствующий конфиг должен вернуть ошибку");

    assert!(
        err.contains("does-not-exist.json"),
        "неожиданная ошибка: {err}"
    );
}
