use chrono::{NaiveTime, Timelike};
use regex::Regex;
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Путь к логу по умолчанию: используется, если аргумент командной строки не передан.
const DEFAULT_LOG_PATH: &str = "./test_data/access.log";

/// Во сколько раз число 5xx-ошибок в час должно превышать среднее,
/// чтобы час считался аномальным всплеском.
const ANOMALY_THRESHOLD: f64 = 1.5;

/// Шаблон формата nginx `combined`:
/// `192.168.1.1 - - [21/Jul/2025:19:30:01 +0300] "GET /index.html HTTP/1.1" 500 1234 "-" "curl/8.0"`
///
/// Извлекает: ip, дату/время (день, месяц, год, час, минута, секунда, таймзона),
/// метод, путь, протокол, код ответа, размер ответа, referer и user-agent.
/// Блок `"referer" "user-agent"` необязателен, поэтому шаблон принимает и common log format.
const NGINX_COMBINED_PATTERN: &str = concat!(
    r"^(?P<ip>\S+)\s+(?P<ident>\S+)\s+(?P<user>\S+)\s+",
    r"\[(?P<day>\d{2})/(?P<month>[A-Za-z]{3})/(?P<year>\d{4}):",
    r"(?P<hour>\d{2}):(?P<minute>\d{2}):(?P<second>\d{2})\s+(?P<tz>[+-]\d{4})\]\s+",
    r#""(?P<method>[A-Z]+)\s+(?P<path>\S+)\s+(?P<protocol>[^"]*)"\s+"#,
    r#"(?P<status>\d{3})\s+(?P<size>\d+|-)"#,
    r#"(?:\s+"(?P<referer>[^"]*)"\s+"(?P<agent>[^"]*)")?\s*$"#,
);

/// Компилирует шаблон nginx combined. Шаблон статический, поэтому паника здесь
/// означала бы ошибку в коде, а не во входных данных.
fn nginx_regex() -> Regex {
    Regex::new(NGINX_COMBINED_PATTERN).expect("шаблон nginx combined должен компилироваться")
}

/// Разбирает одну строку лога. Возвращает `None`, если строка не является
/// валидной записью nginx combined.
fn parse_line(re: &Regex, line: &str) -> Option<LogEntry> {
    let caps = re.captures(line)?;
    let time = NaiveTime::parse_from_str(
        &format!("{}:{}:{}", &caps["hour"], &caps["minute"], &caps["second"]),
        "%H:%M:%S",
    )
    .ok()?;

    Some(LogEntry {
        ip: caps["ip"].to_string(),
        time,
        method: caps["method"].to_string(),
        path: caps["path"].to_string(),
        status: caps["status"].parse().unwrap_or(0),
        size: caps
            .name("size")
            .map(|m| m.as_str())
            .filter(|size| *size != "-")
            .and_then(|size| size.parse().ok())
            .unwrap_or(0),
    })
}

struct LogEntry {
    ip: String,
    time: NaiveTime,
    method: String,
    path: String,
    status: u16,
    size: u64,
}

impl LogEntry {
    /// 4xx и 5xx.
    pub fn is_error(&self) -> bool {
        (400..600).contains(&self.status)
    }

    /// 5xx.
    pub fn is_server_error(&self) -> bool {
        (500..600).contains(&self.status)
    }

    pub fn hour(&self) -> u8 {
        self.time.hour() as u8
    }
}

struct LogAnalyzer {
    log_path: String,
    entries: Vec<LogEntry>,
    status_counts: HashMap<u16, usize>,
    error_counts: HashMap<u16, usize>,
    hourly_requests: HashMap<u8, usize>,
    hourly_server_errors: HashMap<u8, usize>,
    method_counts: HashMap<String, usize>,
    ip_counts: HashMap<String, usize>,
    path_counts: HashMap<String, usize>,
    total_bytes: u64,
}

/// Топ-N значений счётчика: сначала по убыванию количества, затем по возрастанию ключа
/// (детерминированный порядок при равных количествах).
fn top_counts<K: Clone + Ord>(map: &HashMap<K, usize>, limit: usize) -> Vec<(K, usize)> {
    let mut pairs: Vec<(K, usize)> = map.iter().map(|(key, &count)| (key.clone(), count)).collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    pairs.truncate(limit);
    pairs
}

impl LogAnalyzer {
    pub fn new() -> Self {
        LogAnalyzer {
            log_path: String::new(),
            entries: Vec::new(),
            status_counts: HashMap::new(),
            error_counts: HashMap::new(),
            hourly_requests: HashMap::new(),
            hourly_server_errors: HashMap::new(),
            method_counts: HashMap::new(),
            ip_counts: HashMap::new(),
            path_counts: HashMap::new(),
            total_bytes: 0,
        }
    }

    pub fn parse_file<P: AsRef<Path>>(&mut self, path: P) -> Result<(), Box<dyn Error>> {
        let path = path.as_ref();
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let nginx_re = nginx_regex();

        self.log_path = path.display().to_string();

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Some(entry) = parse_line(&nginx_re, &line) {
                *self.status_counts.entry(entry.status).or_insert(0) += 1;
                if entry.is_error() {
                    *self.error_counts.entry(entry.status).or_insert(0) += 1;
                }
                *self.hourly_requests.entry(entry.hour()).or_insert(0) += 1;
                if entry.is_server_error() {
                    *self.hourly_server_errors.entry(entry.hour()).or_insert(0) += 1;
                }
                *self.method_counts.entry(entry.method.clone()).or_insert(0) += 1;
                *self.ip_counts.entry(entry.ip.clone()).or_insert(0) += 1;
                *self.path_counts.entry(entry.path.clone()).or_insert(0) += 1;
                self.total_bytes += entry.size;
                self.entries.push(entry);
            }
        }
        Ok(())
    }

    pub fn total_requests(&self) -> usize {
        self.entries.len()
    }

    /// Период наблюдений (самое раннее и самое позднее время в логе).
    pub fn period(&self) -> Option<(NaiveTime, NaiveTime)> {
        let first = self.entries.iter().map(|entry| entry.time).min()?;
        let last = self.entries.iter().map(|entry| entry.time).max()?;
        Some((first, last))
    }

    /// Топ кодов ответа (все статусы).
    pub fn top_statuses(&self) -> Vec<(u16, usize)> {
        top_counts(&self.status_counts, 10)
    }

    /// Топ ошибок (4xx/5xx).
    pub fn top_errors(&self) -> Vec<(u16, usize)> {
        top_counts(&self.error_counts, 10)
    }

    pub fn hourly_requests_graph(&self) -> String {
        // Пустой лог: максимум отсутствует, деления на ноль/NaN не возникает.
        let max = self.hourly_requests.values().max().copied().unwrap_or(0);
        let mut res = String::new();
        for hour in 0..24u8 {
            let count = self.hourly_requests.get(&hour).copied().unwrap_or(0);
            let bar_len = if max == 0 {
                0
            } else {
                ((count as f64 / max as f64) * 10.0).round() as usize
            };
            let bar = "█".repeat(bar_len);
            res.push_str(&format!("{:02}:00 | {:>5} | {}\n", hour, count, bar));
        }
        res
    }

    /// Поиск аномалий: часы, в которых 5xx-ошибок больше среднего в `threshold` раз.
    ///
    /// Аномалий нет в том числе при пустом логе (среднее не вычисляется,
    /// деление на ноль/NaN исключено).
    pub fn find_anomalies(&self, threshold: f64) -> Vec<String> {
        if self.hourly_server_errors.is_empty() {
            return Vec::new();
        }

        let avg: f64 = self.hourly_server_errors.values().sum::<usize>() as f64
            / self.hourly_server_errors.len() as f64;

        let mut hours: Vec<(u8, usize)> = self
            .hourly_server_errors
            .iter()
            .map(|(&hour, &count)| (hour, count))
            .collect();
        hours.sort_by_key(|&(hour, _)| hour);

        let mut anomalies = Vec::new();
        for (hour, count) in hours {
            if count as f64 > avg * threshold {
                anomalies.push(format!(
                    "Аномалия: в {:02}:00 было {} 5xx-ошибок (среднее по часам с ошибками: {:.1})",
                    hour, count, avg
                ));
            }
        }
        anomalies
    }

    fn write_top<K: fmt::Display>(
        f: &mut fmt::Formatter<'_>,
        title: &str,
        items: &[(K, usize)],
        empty_note: &str,
    ) -> fmt::Result {
        writeln!(f, "{}", title)?;
        if items.is_empty() {
            writeln!(f, "  {}", empty_note)?;
        } else {
            for (key, count) in items {
                writeln!(f, "  {}: {}", key, count)?;
            }
        }
        Ok(())
    }
}

impl fmt::Display for LogAnalyzer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== Анализ лога: {} ===", self.log_path)?;
        writeln!(f, "Всего запросов: {}", self.total_requests())?;
        match self.period() {
            Some((first, last)) => writeln!(
                f,
                "Период: {} .. {}",
                first.format("%H:%M:%S"),
                last.format("%H:%M:%S")
            )?,
            None => writeln!(f, "Период: (нет данных)")?,
        }
        writeln!(f, "Суммарный размер ответов: {} байт", self.total_bytes)?;

        Self::write_top(f, "\nTOP кодов ответа:", &self.top_statuses(), "нет данных")?;
        Self::write_top(f, "\nTOP ошибок (4xx/5xx):", &self.top_errors(), "ошибок нет")?;
        Self::write_top(
            f,
            "\nТоп-5 методов:",
            &top_counts(&self.method_counts, 5),
            "нет данных",
        )?;
        Self::write_top(
            f,
            "\nТоп-5 путей:",
            &top_counts(&self.path_counts, 5),
            "нет данных",
        )?;
        Self::write_top(
            f,
            "\nТоп-5 IP-адресов:",
            &top_counts(&self.ip_counts, 5),
            "нет данных",
        )?;

        writeln!(f, "\nГрафик запросов в час:")?;
        write!(f, "{}", self.hourly_requests_graph())?;

        writeln!(
            f,
            "\nПоиск аномалий (5xx в час > среднего в {:.1} раза):",
            ANOMALY_THRESHOLD
        )?;
        let anomalies = self.find_anomalies(ANOMALY_THRESHOLD);
        if anomalies.is_empty() {
            writeln!(f, "  Аномалий не обнаружено")?;
        } else {
            for anomaly in anomalies {
                writeln!(f, "  {}", anomaly)?;
            }
        }
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let log_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_LOG_PATH.to_string());

    let mut analyzer = LogAnalyzer::new();
    if let Err(err) = analyzer.parse_file(&log_path) {
        eprintln!("Не удалось прочитать лог '{}': {}", log_path, err);
        std::process::exit(1);
    }
    print!("{}", analyzer);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Пример строки из условия задачи (формат nginx combined).
    const ROW_EXAMPLE: &str =
        r#"192.168.1.1 - - [21/Jul/2025:19:30:01 +0300] "GET /index.html HTTP/1.1" 500 1234 "-" "curl/8.0""#;

    fn test_log_path() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("test_data/access.log")
    }

    #[test]
    fn test_request() {
        let entry = parse_line(&nginx_regex(), ROW_EXAMPLE).expect("строка должна разбираться");

        assert_eq!(entry.ip, "192.168.1.1");
        assert_eq!(entry.method, "GET");
        assert_eq!(entry.path, "/index.html");
        assert_eq!(entry.status, 500);
        assert_eq!(entry.size, 1234);
        assert_eq!(entry.time, NaiveTime::from_hms_opt(19, 30, 1).unwrap());
        assert_eq!(entry.hour(), 19);
        assert!(entry.is_error());
        assert!(entry.is_server_error());
    }

    /// Главная проверка шаблона: он должен совпасть с каждой строкой тестового лога.
    #[test]
    fn test_all_lines_of_test_data_are_parsed() {
        let re = nginx_regex();
        let file = File::open(test_log_path()).expect("тестовый лог должен открываться");
        let mut total = 0usize;
        let mut parsed = 0usize;
        let mut server_errors = 0usize;

        for line in BufReader::new(file).lines() {
            let line = line.expect("строка должна читаться");
            if line.trim().is_empty() {
                continue;
            }
            total += 1;
            match parse_line(&re, &line) {
                Some(entry) => {
                    parsed += 1;
                    if entry.status == 500 {
                        server_errors += 1;
                    }
                }
                None => panic!("шаблон не совпал со строкой: {line}"),
            }
        }

        assert_eq!(total, 57, "в тестовом логе ровно 57 непустых строк");
        assert_eq!(parsed, total, "шаблон обязан разобрать все строки");
        assert_eq!(server_errors, 54, "в тестовом логе 54 ответа со статусом 500");
    }

    #[test]
    fn test_analyzer_report_on_test_data() {
        let mut analyzer = LogAnalyzer::new();
        analyzer
            .parse_file(test_log_path())
            .expect("тестовый лог должен разбираться без ошибок");

        assert_eq!(analyzer.total_requests(), 57);
        assert_eq!(analyzer.top_statuses().first(), Some(&(500, 54)));
        assert_eq!(analyzer.top_errors(), vec![(500, 54), (404, 1)]);

        let report = analyzer.to_string();
        assert!(report.contains("500: 54"));
        assert!(report.contains("19:00 |    53 |"));
        assert!(report.contains("Аномалия: в 19:00"));
        assert!(!report.to_lowercase().contains("nan"));
    }

    #[test]
    fn test_anomaly_threshold_is_consistent() {
        let mut analyzer = LogAnalyzer::new();
        analyzer.parse_file(test_log_path()).unwrap();

        // 54 ошибки 500: одна в 16:00 и 53 в 19:00 -> среднее 27.0.
        let anomalies = analyzer.find_anomalies(ANOMALY_THRESHOLD);
        assert_eq!(anomalies.len(), 1);
        assert!(anomalies[0].contains("19:00 было 53"));
        // Вдвое выше среднего (54.0) порог уже не превышен.
        assert!(analyzer.find_anomalies(2.0).is_empty());
    }

    #[test]
    fn test_empty_log_has_no_nan() {
        let analyzer = LogAnalyzer::new();

        assert_eq!(analyzer.total_requests(), 0);
        assert!(analyzer.find_anomalies(ANOMALY_THRESHOLD).is_empty());
        assert!(!analyzer.hourly_requests_graph().to_lowercase().contains("nan"));

        let report = analyzer.to_string();
        assert!(!report.to_lowercase().contains("nan"));
        assert!(report.contains("Аномалий не обнаружено"));
    }

    #[test]
    fn test_garbage_lines_are_skipped() {
        let re = nginx_regex();
        assert!(parse_line(&re, "это не строка лога").is_none());
        assert!(parse_line(&re, "").is_none());
        // Несуществующее время должно отбрасываться.
        let broken_time = r#"1.2.3.4 - - [10/Jul/2025:25:99:99 +0300] "GET / HTTP/1.1" 200 10"#;
        assert!(parse_line(&re, broken_time).is_none());
        // Строка без referer/agent (common log format).
        let common = r#"1.2.3.4 - - [10/Jul/2025:01:02:03 +0300] "POST /api HTTP/1.0" 201 42"#;
        let entry = parse_line(&re, common).expect("common log format должен разбираться");
        assert_eq!(entry.method, "POST");
        assert_eq!(entry.status, 201);
        assert_eq!(entry.size, 42);
    }
}
