# win_api — Bluetooth Battery Monitor (чистый Win32)

Нативное GUI-приложение на Win32 API без сторонних GUI-фреймворков: показывает Bluetooth-устройства
компьютера и уровень заряда их батарей.

## Что делает

- создаёт окно через `CreateWindowExW`: список `LISTBOX` (контрол 1001) и кнопку «🔄 Обновить» (1002);
- перечисляет Bluetooth-устройства из реестра: `HKLM\SYSTEM\CurrentControlSet\Services\BTHPORT\Parameters\Devices`,
  `…\Enum\BTHENUM`, `…\Enum\BTHLE`;
- для каждого устройства пробует несколько источников заряда подряд (7 эвристик), чтобы покрыть разные
  драйверы и версии Windows;
- рисует заряд текстом и эмодзи-шкалой (`get_battery_bar` / `get_battery_icon`) через GDI `TextOutW`.

## Как запустить

```bash
cd win_api
cargo run --release
```

Только Windows. Нужен Bluetooth-адаптер и хотя бы одно сопряжённое устройство; чтение ветки `HKLM`
доступно обычному пользователю.

## Чем интересен

- **~2200 строк** в одном файле, 32 функции, весь Win32-обвес написан вручную (окно, цикл сообщений,
  контролы, GDI-вывод);
- работа с реестром: `RegOpenKeyExW`, `RegEnumKeyExW`, `RegEnumValueW`, `RegGetValueW`;
- SetupAPI: `SetupDiGetClassDevsW`, `CM_Get_Device_IDW`, `SetupDiGetDevicePropertyW`, `SetupDiOpenDevRegKey`;
- ошибки обрабатываются через `windows::core::Result` и `Error::from_win32()` — **ни одного `unwrap()`**;
- 5 тестов в `#[cfg(test)] mod tests`;
- ~100 строк комментариев, поясняющих работу с API.

Зависимость одна: `windows = "0.58"` с наборами features `Win32_Foundation`,
`Win32_UI_WindowsAndMessaging`, `Win32_UI_Controls`, `Win32_Graphics_Gdi`, `Win32_System_Registry`,
`Win32_Devices_Properties`, `Win32_Devices_DeviceAndDriverInstallation`, `Win32_System_SystemInformation`.

## Честные ограничения

- **Заряд определяется эвристикой.** В ключах реестра ищется любое числовое значение в диапазоне 1..=100,
  поэтому возможны ложные проценты (например, значение из другого поля). Для «настоящего» заряда нужно
  читать GATT-характеристику Battery Level (0x2A19) по BLE.
- WinRT/Bluetooth API не вызывается, хотя feature `Win32_Devices_Bluetooth` объявлен — список устройств
  берётся из реестра.
- ~30 блоков `unsafe` и `static mut DEVICES` (через `std::ptr::addr_of!`) — типично для сырого Win32,
  но с точки зрения безопасности кода это не образец.
- Тесты в основном печатают результат, есть `assert!(true, …)` — реальной проверки логики нет.
- Часть объявленных features не используется.
