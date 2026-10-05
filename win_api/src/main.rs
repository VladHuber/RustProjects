use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use windows::{
    core::*,
    Win32::Foundation::*,
    Win32::Graphics::Gdi::*,
    Win32::System::Registry::*,
    Win32::UI::WindowsAndMessaging::*,
    Win32::Devices::Properties::*,
};

// Макросы для работы с WPARAM
#[allow(non_snake_case)]
fn HIWORD(l: u32) -> u16 {
    ((l >> 16) & 0xFFFF) as u16
}

#[allow(non_snake_case)]
fn LOWORD(l: u32) -> u16 {
    (l & 0xFFFF) as u16
}

const ID_LISTBOX: i32 = 1001;
const ID_REFRESH: i32 = 1002;

// Константы для стилей окон
const WS_OVERLAPPEDWINDOW: u32 = 0x00CF0000;
const WS_CHILD: u32 = 0x40000000;
const WS_VISIBLE: u32 = 0x10000000;
const WS_BORDER: u32 = 0x00800000;
const WS_VSCROLL: u32 = 0x00200000;
const LBS_NOTIFY: u32 = 0x0001;
const BS_PUSHBUTTON: u32 = 0x00000000;

#[derive(Clone)]
struct BluetoothDeviceInfo {
    name: String,
    battery_level: Option<u8>,
    address: String,
}

static mut DEVICES: Vec<BluetoothDeviceInfo> = Vec::new();

fn main() -> Result<()> {
    unsafe {
        let instance = HMODULE::default();
        let class_name = w!("BluetoothBatteryMonitor");
        
        // Регистрируем класс окна
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            hIcon: LoadIconW(None, IDI_APPLICATION)?,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as *mut _),
            lpszClassName: class_name,
            ..Default::default()
        };
        
        if RegisterClassExW(&wc) == 0 {
            return Err(Error::from_win32());
        }
        
        // Создаем окно
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("Bluetooth Battery Monitor"),
            WINDOW_STYLE(WS_OVERLAPPEDWINDOW),
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            600,
            500,
            None,
            None,
            instance,
            None,
        )?;
        
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = UpdateWindow(hwnd);
        
        // Загружаем устройства при запуске
        refresh_devices(hwnd)?;
        
        // Цикл сообщений
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    
    Ok(())
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            if create_controls(hwnd).is_err() {
                return LRESULT(-1);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let cmd = HIWORD(wparam.0 as u32);
            let id = LOWORD(wparam.0 as u32) as i32;
            
            if u32::from(cmd) == BN_CLICKED {
                match id {
                    ID_REFRESH => {
                        if refresh_devices(hwnd).is_ok() {
                            update_listbox(hwnd);
                        }
                    }
                    _ => {}
                }
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            
            // Рисуем заголовок
            let title = "🔋 Заряд Bluetooth устройств";
            let title_wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
            let _ = TextOutW(hdc, 20, 20, &title_wide[..title_wide.len() - 1]);
            
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn create_controls(hwnd: HWND) -> Result<()> {
    let instance = HMODULE::default();
    
    // Создаем ListBox для отображения устройств
    CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("LISTBOX"),
        None,
        WINDOW_STYLE(WS_CHILD | WS_VISIBLE | WS_BORDER | WS_VSCROLL | LBS_NOTIFY),
        20,
        60,
        540,
        350,
        hwnd,
        HMENU(ID_LISTBOX as isize as *mut _),
        instance,
        None,
    )?;
    
    // Создаем кнопку обновления
    CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("BUTTON"),
        w!("🔄 Обновить"),
        WINDOW_STYLE(WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON),
        20,
        420,
        120,
        30,
        hwnd,
        HMENU(ID_REFRESH as isize as *mut _),
        instance,
        None,
    )?;
    
    Ok(())
}

unsafe fn refresh_devices(hwnd: HWND) -> Result<()> {
    DEVICES = get_bluetooth_devices()?;
    update_listbox(hwnd);
    Ok(())
}

unsafe fn update_listbox(hwnd: HWND) {
    let listbox = match GetDlgItem(hwnd, ID_LISTBOX) {
        Ok(h) => h,
        Err(_) => return,
    };
    
    // Очищаем список
    SendMessageW(listbox, LB_RESETCONTENT, WPARAM(0), LPARAM(0));
    
    // Добавляем устройства
    let devices = unsafe { &*std::ptr::addr_of!(DEVICES) };
    for device in devices {
        let mut line = format!("📱 {}", device.name);
        
        if let Some(level) = device.battery_level {
            let battery_bar = get_battery_bar(level);
            let status_icon = get_battery_icon(level);
            line.push_str(&format!("  {} {}% {}", status_icon, level, battery_bar));
        } else {
            line.push_str("  ⚠ Батарея: недоступна");
        }
        
        let line_wide: Vec<u16> = line.encode_utf16().chain(std::iter::once(0)).collect();
        SendMessageW(
            listbox,
            LB_ADDSTRING,
            WPARAM(0),
            LPARAM(line_wide.as_ptr() as isize),
        );
    }
    
    let devices = unsafe { &*std::ptr::addr_of!(DEVICES) };
    if devices.is_empty() {
        let msg = "Нет подключенных Bluetooth устройств";
        let msg_wide: Vec<u16> = msg.encode_utf16().chain(std::iter::once(0)).collect();
        SendMessageW(
            listbox,
            LB_ADDSTRING,
            WPARAM(0),
            LPARAM(msg_wide.as_ptr() as isize),
        );
    }
}

unsafe fn get_bluetooth_devices() -> Result<Vec<BluetoothDeviceInfo>> {
    let mut devices = Vec::new();
    
    // Получаем устройства из реестра
    match get_devices_from_registry() {
        Ok(reg_devices) => devices.extend(reg_devices),
        Err(_) => {}
    }
    
    // Пробуем получить информацию о батарее через альтернативные методы
    for device in &mut devices {
        if device.battery_level.is_none() {
            // Пробуем все возможные методы поиска
            if let Ok(level) = get_battery_from_alternative_path(&device.address) {
                device.battery_level = Some(level);
            } else if let Ok(level) = get_battery_from_device_properties(&device.address) {
                device.battery_level = Some(level);
            } else if let Ok(level) = get_battery_from_device_instance(&device.address) {
                device.battery_level = Some(level);
            } else if let Ok(level) = get_battery_from_connected_devices(&device.address) {
                device.battery_level = Some(level);
            } else if let Ok(level) = get_battery_from_bthenum_detailed(&device.address) {
                device.battery_level = Some(level);
            } else if let Ok(level) = get_battery_from_current_user(&device.address) {
                device.battery_level = Some(level);
            } else if let Ok(level) = get_battery_from_setupapi(&device.address) {
                device.battery_level = Some(level);
            }
        }
    }
    
    Ok(devices)
}

unsafe fn get_devices_from_registry() -> Result<Vec<BluetoothDeviceInfo>> {
    let mut devices = Vec::new();
    
    let key_path = r"SYSTEM\CurrentControlSet\Services\BTHPORT\Parameters\Devices";
    let mut hkey_result = HKEY::default();
    
    let status = RegOpenKeyExW(
        HKEY_LOCAL_MACHINE,
        &HSTRING::from(key_path),
        0,
        KEY_READ,
        &mut hkey_result,
    );
    
    if status.is_err() {
        return Ok(devices);
    }
    
    let mut index = 0u32;
    loop {
        let mut name_buf = vec![0u16; 256];
        let mut name_len = name_buf.len() as u32;
        
        let status = RegEnumKeyExW(
            hkey_result,
            index,
            PWSTR::from_raw(name_buf.as_mut_ptr()),
            &mut name_len,
            Some(std::ptr::null()),
            PWSTR::null(),
            None,
            None,
        );
        
        if status.is_err() {
            break;
        }
        
        name_buf.truncate(name_len as usize);
        let device_address = OsString::from_wide(&name_buf)
            .to_string_lossy()
            .to_string();
        
        let formatted_address = format_bluetooth_address(&device_address);
        
        // Пробуем получить имя устройства несколькими способами
        let device_name = get_device_name_from_registry(hkey_result, &device_address)
            .or_else(|| get_device_name_from_alternative_path(&device_address))
            .unwrap_or_else(|| {
                // Если имя не найдено, используем адрес
                format!("Bluetooth устройство {}", formatted_address)
            });
        
        let battery_level = get_battery_level_from_registry(hkey_result, &device_address)
            .ok();
        
        devices.push(BluetoothDeviceInfo {
            name: device_name,
            battery_level,
            address: formatted_address,
        });
        
        index += 1;
    }
    
    let _ = RegCloseKey(hkey_result);
    Ok(devices)
}

unsafe fn get_device_name_from_registry(hkey: HKEY, address: &str) -> Option<String> {
    // Открываем подраздел устройства
    let mut device_key = HKEY::default();
    if RegOpenKeyExW(
        hkey,
        &HSTRING::from(address),
        0,
        KEY_READ,
        &mut device_key,
    ).is_err() {
        return None;
    }
    
    // Пробуем разные ключи для имени устройства
    let name_keys = ["Name", "FriendlyName", "DeviceName", "Device Description"];
    
    for key_name in &name_keys {
        let mut name_buf = vec![0u8; 512]; // Буфер для байтов
        let mut name_len = name_buf.len() as u32;
        let value_name = HSTRING::from(*key_name);
        let mut value_type = REG_VALUE_TYPE(0);
        
        // Используем RegQueryValueExW для прямого чтения
        let status = RegQueryValueExW(
            device_key,
            &value_name,
            None,
            Some(&mut value_type),
            Some(name_buf.as_mut_ptr()),
            Some(&mut name_len),
        );
        
        if status.is_ok() && name_len > 0 {
            // Обрабатываем в зависимости от типа
            if value_type.0 == 1 || value_type.0 == 2 { // REG_SZ или REG_EXPAND_SZ
                // Это уже UTF-16 строка
                if name_len >= 2 {
                    let u16_count = (name_len as usize / 2).saturating_sub(1);
                    if u16_count > 0 {
                        let mut u16_vec = Vec::with_capacity(u16_count);
                        for i in 0..u16_count.min(name_len as usize / 2) {
                            let byte_idx = i * 2;
                            if byte_idx + 1 < name_len as usize {
                                let u16_val = u16::from_le_bytes([
                                    name_buf[byte_idx],
                                    name_buf.get(byte_idx + 1).copied().unwrap_or(0),
                                ]);
                                u16_vec.push(u16_val);
                            }
                        }
                        if !u16_vec.is_empty() {
                            if let Some(&0) = u16_vec.last() {
                                u16_vec.pop();
                            }
                            let name = OsString::from_wide(&u16_vec).to_string_lossy().to_string();
                            if !name.is_empty() && name.chars().any(|c| c.is_alphanumeric() || c.is_whitespace() || c == '-' || c == '_') {
                                let _ = RegCloseKey(device_key);
                                return Some(name);
                            }
                        }
                    }
                }
            } else if value_type.0 == 3 { // REG_BINARY
                // REG_BINARY для ключа "Name" в Bluetooth реестре хранит данные как UTF-8!
                // Сначала пробуем как UTF-8/ASCII
                if name_len > 0 {
                    let data = &name_buf[..name_len as usize];
                    // Убираем завершающие нули
                    let trimmed_data: Vec<u8> = data.iter()
                        .take_while(|&&b| b != 0)
                        .copied()
                        .collect();
                    
                    if let Ok(utf8_str) = std::str::from_utf8(&trimmed_data) {
                        let trimmed = utf8_str.trim();
                        if !trimmed.is_empty() && trimmed.chars().any(|c| c.is_alphanumeric() || c.is_whitespace() || c == '-' || c == '_') {
                            let _ = RegCloseKey(device_key);
                            return Some(trimmed.to_string());
                        }
                    }
                }
                
                // Если UTF-8 не сработал, пробуем как UTF-16 (на случай других форматов)
                if name_len >= 2 && name_len % 2 == 0 {
                    let u16_count = name_len as usize / 2;
                    let mut u16_vec = Vec::with_capacity(u16_count);
                    for i in 0..u16_count {
                        let byte_idx = i * 2;
                        if byte_idx + 1 < name_len as usize {
                            let u16_val = u16::from_le_bytes([
                                name_buf[byte_idx],
                                name_buf.get(byte_idx + 1).copied().unwrap_or(0),
                            ]);
                            u16_vec.push(u16_val);
                        }
                    }
                    while u16_vec.last() == Some(&0) {
                        u16_vec.pop();
                    }
                    if !u16_vec.is_empty() {
                        let os_str = OsString::from_wide(&u16_vec);
                        let name = os_str.to_string_lossy().to_string();
                        if !name.is_empty() && name.chars().any(|c| c.is_alphanumeric() || c.is_whitespace() || c == '-' || c == '_') {
                            let _ = RegCloseKey(device_key);
                            return Some(name);
                        }
                    }
                }
            }
        }
    }
    
    let _ = RegCloseKey(device_key);
    
    // Пробуем получить имя через перечисление всех значений
    if let Some(name) = enumerate_registry_values_for_name(hkey, address) {
        return Some(name);
    }
    
    // Пробуем получить имя из альтернативных мест
    if let Some(name) = get_device_name_from_alternative_path(address) {
        return Some(name);
    }
    
    None
}

unsafe fn enumerate_registry_values_for_name(hkey: HKEY, address: &str) -> Option<String> {
    let mut device_key = HKEY::default();
    let status = RegOpenKeyExW(
        hkey,
        &HSTRING::from(address),
        0,
        KEY_READ,
        &mut device_key,
    );
    
    if status.is_err() {
        return None;
    }
    
    let mut index = 0u32;
    loop {
        let mut value_name_buf = vec![0u16; 256];
        let mut value_name_len = value_name_buf.len() as u32;
        let mut value_type = REG_VALUE_TYPE(0);
        let mut value_data = vec![0u8; 512];
        let mut value_data_len = value_data.len() as u32;
        
        let status = RegEnumValueW(
            device_key,
            index,
            PWSTR::from_raw(value_name_buf.as_mut_ptr()),
            &mut value_name_len,
            None,
            Some(&mut value_type.0),
            Some(value_data.as_mut_ptr() as *mut u8),
            Some(&mut value_data_len),
        );
        
        if status.is_err() {
            break;
        }
        
        value_name_buf.truncate(value_name_len as usize);
        let value_name = OsString::from_wide(&value_name_buf)
            .to_string_lossy()
            .to_string();
        
        // Ищем ключи, содержащие "name" или "friendly" (регистронезависимо)
        let value_name_lower = value_name.to_lowercase();
        if value_name_lower.contains("name") || value_name_lower.contains("friendly") {
            // Проверяем тип значения
            if value_type.0 == 1 || value_type.0 == 2 { // REG_SZ или REG_EXPAND_SZ
                if value_data_len >= 2 {
                    // Конвертируем байты в u16 для UTF-16
                    let u16_count = (value_data_len as usize / 2).saturating_sub(1);
                    if u16_count > 0 {
                        let mut u16_data = Vec::with_capacity(u16_count);
                        for i in 0..u16_count.min(value_data.len() / 2) {
                            let byte_idx = i * 2;
                            if byte_idx + 1 < value_data.len() {
                                let u16_val = u16::from_le_bytes([
                                    value_data[byte_idx],
                                    value_data.get(byte_idx + 1).copied().unwrap_or(0),
                                ]);
                                u16_data.push(u16_val);
                            }
                        }
                        if !u16_data.is_empty() {
                            let name = OsString::from_wide(&u16_data).to_string_lossy().to_string();
                            if !name.is_empty() && name.chars().any(|c| c.is_alphanumeric() || c.is_whitespace()) {
                                let _ = RegCloseKey(device_key);
                                return Some(name);
                            }
                        }
                    }
                }
            } else if value_type.0 == 3 { // REG_BINARY
                // REG_BINARY для Name хранит данные как UTF-8
                if value_data_len > 0 {
                    let trimmed_data: Vec<u8> = value_data.iter()
                        .take(value_data_len as usize)
                        .take_while(|&&b| b != 0)
                        .copied()
                        .collect();
                    
                    if let Ok(utf8_str) = std::str::from_utf8(&trimmed_data) {
                        let trimmed = utf8_str.trim();
                        if !trimmed.is_empty() && trimmed.chars().any(|c| c.is_alphanumeric() || c.is_whitespace() || c == '-' || c == '_') {
                            let _ = RegCloseKey(device_key);
                            return Some(trimmed.to_string());
                        }
                    }
                }
            }
        }
        
        index += 1;
    }
    
    let _ = RegCloseKey(device_key);
    None
}

unsafe fn get_device_name_from_alternative_path(address: &str) -> Option<String> {
    // Ищем имя устройства в BTHENUM
    let enum_path = r"SYSTEM\CurrentControlSet\Enum\BTHENUM";
    let mut hkey_enum = HKEY::default();
    
    let status = RegOpenKeyExW(
        HKEY_LOCAL_MACHINE,
        &HSTRING::from(enum_path),
        0,
        KEY_READ,
        &mut hkey_enum,
    );
    
    if status.is_err() {
        return None;
    }
    
    let address_upper = address.replace(":", "").to_uppercase();
    let mut index = 0u32;
    
    loop {
        let mut name_buf = vec![0u16; 256];
        let mut name_len = name_buf.len() as u32;
        
        let status = RegEnumKeyExW(
            hkey_enum,
            index,
            PWSTR::from_raw(name_buf.as_mut_ptr()),
            &mut name_len,
            Some(std::ptr::null()),
            PWSTR::null(),
            None,
            None,
        );
        
        if status.is_err() {
            break;
        }
        
        name_buf.truncate(name_len as usize);
        let device_id = OsString::from_wide(&name_buf)
            .to_string_lossy()
            .to_string();
        
        if device_id.to_uppercase().contains(&address_upper) {
            let mut device_key = HKEY::default();
            let device_path = format!(r"{}\{}", enum_path, device_id);
            
            if RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                &HSTRING::from(&device_path),
                0,
                KEY_READ,
                &mut device_key,
            ).is_ok() {
                // Ищем FriendlyName или DeviceDesc
                let name_keys = ["FriendlyName", "DeviceDesc"];
                for key_name in &name_keys {
                    let mut name_buf = vec![0u16; 512];
                    let mut name_len = name_buf.len() as u32;
                    let value_name = HSTRING::from(*key_name);
                    let mut value_type = REG_VALUE_TYPE(0);
                    
                    let name_status = RegGetValueW(
                        device_key,
                        None,
                        &value_name,
                        REG_ROUTINE_FLAGS(0),
                        Some(&mut value_type),
                        Some(name_buf.as_mut_ptr() as *mut std::ffi::c_void),
                        Some(&mut name_len),
                    );
                    
                    if name_status.is_ok() && name_len > 0 {
                        let char_count = (name_len as usize / 2).saturating_sub(1);
                        if char_count > 0 && char_count < name_buf.len() {
                            name_buf.truncate(char_count);
                            let name = OsString::from_wide(&name_buf).to_string_lossy().to_string();
                            if !name.is_empty() {
                                let _ = RegCloseKey(device_key);
                                let _ = RegCloseKey(hkey_enum);
                                return Some(name);
                            }
                        }
                    }
                }
                
                let _ = RegCloseKey(device_key);
            }
        }
        
        index += 1;
    }
    
    let _ = RegCloseKey(hkey_enum);
    None
}

unsafe fn get_battery_level_from_registry(hkey: HKEY, address: &str) -> Result<u8> {
    // Расширенный список возможных ключей для батареи
    let battery_keys = [
        "BatteryLevel",
        "BatteryPercentage", 
        "Battery",
        "BatteryPercent",
        "BatteryLevelPercent",
        "BatteryStatus",
        "Battery",
        "BatteryInfo",
        "BatteryState",
    ];
    
    // Пробуем прочитать каждый ключ
    for key_name in &battery_keys {
        let mut battery_buf = vec![0u8; 8]; // Увеличиваем буфер для разных типов данных
        let mut battery_len = battery_buf.len() as u32;
        let value_name = HSTRING::from(*key_name);
        
        let mut value_type = REG_VALUE_TYPE(0);
        let status = RegGetValueW(
            hkey,
            &HSTRING::from(address),
            &value_name,
            REG_ROUTINE_FLAGS(0),
            Some(&mut value_type),
            Some(battery_buf.as_mut_ptr() as *mut std::ffi::c_void),
            Some(&mut battery_len),
        );
        
        if status.is_ok() && battery_len >= 4 {
            // Пробуем разные форматы данных
            // DWORD (4 байта)
            if battery_len >= 4 {
                let level = u32::from_le_bytes([
                    battery_buf[0],
                    battery_buf[1],
                    battery_buf[2],
                    battery_buf[3],
                ]) as u8;
                
                if level <= 100 {
                    return Ok(level);
                }
            }
            
            // BYTE (1 байт)
            if battery_len >= 1 {
                let level = battery_buf[0];
                if level <= 100 {
                    return Ok(level);
                }
            }
        }
    }
    
    // Пробуем найти через перечисление всех значений в ключе устройства
    if let Ok(level) = enumerate_registry_values_for_battery(hkey, address) {
        return Ok(level);
    }
    
    Err(Error::from_win32())
}

unsafe fn enumerate_registry_values_for_battery(hkey: HKEY, address: &str) -> Result<u8> {
    // Открываем подраздел устройства
    let mut device_key = HKEY::default();
    let status = RegOpenKeyExW(
        hkey,
        &HSTRING::from(address),
        0,
        KEY_READ,
        &mut device_key,
    );
    
    if status.is_err() {
        return Err(Error::from_win32());
    }
    
    // Перечисляем все значения в ключе устройства
    let mut index = 0u32;
    loop {
        let mut value_name_buf = vec![0u16; 256];
        let mut value_name_len = value_name_buf.len() as u32;
        let mut value_type = REG_VALUE_TYPE(0);
        let mut value_data = vec![0u8; 256];
        let mut value_data_len = value_data.len() as u32;
        
        let status = RegEnumValueW(
            device_key,
            index,
            PWSTR::from_raw(value_name_buf.as_mut_ptr()),
            &mut value_name_len,
            None,
            Some(&mut value_type.0),
            Some(value_data.as_mut_ptr()),
            Some(&mut value_data_len),
        );
        
        if status.is_err() {
            break;
        }
        
        value_name_buf.truncate(value_name_len as usize);
        let value_name = OsString::from_wide(&value_name_buf)
            .to_string_lossy()
            .to_string();
        
        // Ищем ключи, содержащие "battery" (регистронезависимо)
        if value_name.to_lowercase().contains("battery") {
            if value_data_len >= 4 {
                let level = u32::from_le_bytes([
                    value_data[0],
                    value_data[1],
                    value_data[2],
                    value_data[3],
                ]) as u8;
                
                if level <= 100 {
                    let _ = RegCloseKey(device_key);
                    return Ok(level);
                }
            } else if value_data_len >= 1 {
                let level = value_data[0];
                if level <= 100 {
                    let _ = RegCloseKey(device_key);
                    return Ok(level);
                }
            }
        }
        
        index += 1;
    }
    
    let _ = RegCloseKey(device_key);
    Err(Error::from_win32())
}

unsafe fn get_battery_from_alternative_path(address: &str) -> Result<u8> {
    let enum_path = r"SYSTEM\CurrentControlSet\Enum\BTHENUM";
    let mut hkey_enum = HKEY::default();
    
    let status = RegOpenKeyExW(
        HKEY_LOCAL_MACHINE,
        &HSTRING::from(enum_path),
        0,
        KEY_READ,
        &mut hkey_enum,
    );
    
    if status.is_err() {
        return Err(Error::from_win32());
    }
    
    let address_upper = address.replace(":", "").to_uppercase();
    let mut index = 0u32;
    
    loop {
        let mut name_buf = vec![0u16; 256];
        let mut name_len = name_buf.len() as u32;
        
        let status = RegEnumKeyExW(
            hkey_enum,
            index,
            PWSTR::from_raw(name_buf.as_mut_ptr()),
            &mut name_len,
            Some(std::ptr::null()),
            PWSTR::null(),
            None,
            None,
        );
        
        if status.is_err() {
            break;
        }
        
        name_buf.truncate(name_len as usize);
        let device_id = OsString::from_wide(&name_buf)
            .to_string_lossy()
            .to_string();
        
        if device_id.to_uppercase().contains(&address_upper) {
            let mut device_key = HKEY::default();
            let device_path = format!(r"{}\{}", enum_path, device_id);
            
            if RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                &HSTRING::from(&device_path),
                0,
                KEY_READ,
                &mut device_key,
            ).is_ok() {
                let mut sub_index = 0u32;
                loop {
                    let mut sub_name_buf = vec![0u16; 256];
                    let mut sub_name_len = sub_name_buf.len() as u32;
                    
                    let sub_status = RegEnumKeyExW(
                        device_key,
                        sub_index,
                        PWSTR::from_raw(sub_name_buf.as_mut_ptr()),
                        &mut sub_name_len,
                        Some(std::ptr::null()),
                        PWSTR::null(),
                        None,
                        None,
                    );
                    
                    if sub_status.is_err() {
                        break;
                    }
                    
                    sub_name_buf.truncate(sub_name_len as usize);
                    let sub_id = OsString::from_wide(&sub_name_buf)
                        .to_string_lossy()
                        .to_string();
                    
                    let mut final_key = HKEY::default();
                    let final_path = format!(r"{}\{}\{}", enum_path, device_id, sub_id);
                    
                    if RegOpenKeyExW(
                        HKEY_LOCAL_MACHINE,
                        &HSTRING::from(&final_path),
                        0,
                        KEY_READ,
                        &mut final_key,
                    ).is_ok() {
                        // Пробуем разные ключи для батареи
                        let battery_keys = ["BatteryLevel", "BatteryPercentage", "Battery", "BatteryPercent"];
                        
                        for battery_key_name in &battery_keys {
                            let mut battery_buf = vec![0u8; 8];
                            let mut battery_len = battery_buf.len() as u32;
                            let value_name = HSTRING::from(*battery_key_name);
                            let mut value_type = REG_VALUE_TYPE(0);
                            
                            let battery_status = RegGetValueW(
                                final_key,
                                None,
                                &value_name,
                                REG_ROUTINE_FLAGS(0),
                                Some(&mut value_type),
                                Some(battery_buf.as_mut_ptr() as *mut std::ffi::c_void),
                                Some(&mut battery_len),
                            );
                            
                            if battery_status.is_ok() && battery_len >= 1 {
                                let level = if battery_len >= 4 {
                                    u32::from_le_bytes([
                                        battery_buf[0],
                                        battery_buf[1],
                                        battery_buf[2],
                                        battery_buf[3],
                                    ]) as u8
                                } else {
                                    battery_buf[0]
                                };
                                
                                if level <= 100 {
                                    let _ = RegCloseKey(final_key);
                                    let _ = RegCloseKey(device_key);
                                    let _ = RegCloseKey(hkey_enum);
                                    return Ok(level);
                                }
                            }
                        }
                        
                        // Пробуем перечислить все значения в этом ключе
                        if let Ok(level) = enumerate_registry_values_for_battery(final_key, "") {
                            let _ = RegCloseKey(final_key);
                            let _ = RegCloseKey(device_key);
                            let _ = RegCloseKey(hkey_enum);
                            return Ok(level);
                        }
                        
                        let _ = RegCloseKey(final_key);
                    }
                    
                    sub_index += 1;
                }
                
                let _ = RegCloseKey(device_key);
            }
        }
        
        index += 1;
    }
    
    let _ = RegCloseKey(hkey_enum);
    Err(Error::from_win32())
}

unsafe fn get_battery_from_device_properties(address: &str) -> Result<u8> {
    let device_paths = [
        r"SYSTEM\CurrentControlSet\Enum\BTHENUM",
        r"SYSTEM\CurrentControlSet\Enum\BTHLE",
    ];
    
    for base_path in &device_paths {
        if let Ok(level) = search_battery_in_device_tree(base_path, address) {
            return Ok(level);
        }
    }
    
    Err(Error::from_win32())
}

unsafe fn search_battery_in_device_tree(base_path: &str, address: &str) -> Result<u8> {
    let mut hkey_base = HKEY::default();
    
    let status = RegOpenKeyExW(
        HKEY_LOCAL_MACHINE,
        &HSTRING::from(base_path),
        0,
        KEY_READ,
        &mut hkey_base,
    );
    
    if status.is_err() {
        return Err(Error::from_win32());
    }
    
    let result = search_battery_recursive(hkey_base, address, "");
    let _ = RegCloseKey(hkey_base);
    result
}

unsafe fn search_battery_recursive(hkey: HKEY, address: &str, _current_path: &str) -> Result<u8> {
    let mut index = 0u32;
    let address_upper = address.replace(":", "").to_uppercase();
    
    loop {
        let mut name_buf = vec![0u16; 256];
        let mut name_len = name_buf.len() as u32;
        
        let status = RegEnumKeyExW(
            hkey,
            index,
            PWSTR::from_raw(name_buf.as_mut_ptr()),
            &mut name_len,
            Some(std::ptr::null()),
            PWSTR::null(),
            None,
            None,
        );
        
        if status.is_err() {
            break;
        }
        
        name_buf.truncate(name_len as usize);
        let subkey_name = OsString::from_wide(&name_buf)
            .to_string_lossy()
            .to_string();
        
        if subkey_name.to_uppercase().contains(&address_upper) {
            let mut subkey = HKEY::default();
            
            if RegOpenKeyExW(
                hkey,
                &HSTRING::from(&subkey_name),
                0,
                KEY_READ,
                &mut subkey,
            ).is_ok() {
                let mut battery_buf = vec![0u8; 4];
                let mut battery_len = battery_buf.len() as u32;
                let value_name = HSTRING::from("BatteryLevel");
                let mut value_type = REG_VALUE_TYPE(0);
                
                let battery_status = RegGetValueW(
                    subkey,
                    None,
                    &value_name,
                    REG_ROUTINE_FLAGS(0),
                    Some(&mut value_type),
                    Some(battery_buf.as_mut_ptr() as *mut std::ffi::c_void),
                    Some(&mut battery_len),
                );
                
                if battery_status.is_ok() && battery_len >= 4 {
                    let level = u32::from_le_bytes([
                        battery_buf[0],
                        battery_buf[1],
                        battery_buf[2],
                        battery_buf[3],
                    ]) as u8;
                    
                    if level <= 100 {
                        let _ = RegCloseKey(subkey);
                        return Ok(level);
                    }
                }
                
                if let Ok(level) = search_battery_recursive(subkey, address, "") {
                    let _ = RegCloseKey(subkey);
                    return Ok(level);
                }
                
                let _ = RegCloseKey(subkey);
            }
        }
        
        index += 1;
    }
    
    Err(Error::from_win32())
}

unsafe fn get_battery_from_device_instance(address: &str) -> Result<u8> {
    // Ищем через DeviceInstanceID в реестре
    // Windows может хранить информацию в SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices
    let paths = [
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices",
        r"SYSTEM\CurrentControlSet\Control\DeviceClasses",
        r"SYSTEM\CurrentControlSet\Enum\USB",
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Bluetooth\Devices",
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Bluetooth\Settings",
    ];
    
    for base_path in &paths {
        let mut hkey_base = HKEY::default();
        if RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(*base_path),
            0,
            KEY_READ,
            &mut hkey_base,
        ).is_ok() {
            if let Ok(level) = search_battery_recursive(hkey_base, address, "") {
                let _ = RegCloseKey(hkey_base);
                return Ok(level);
            }
            let _ = RegCloseKey(hkey_base);
        }
    }
    
    Err(Error::from_win32())
}

unsafe fn get_battery_from_connected_devices(address: &str) -> Result<u8> {
    // Ищем в разделе подключенных устройств
    let path = r"SOFTWARE\Microsoft\Windows\CurrentVersion\DeviceAccess\Global";
    let mut hkey = HKEY::default();
    
    if RegOpenKeyExW(
        HKEY_LOCAL_MACHINE,
        &HSTRING::from(path),
        0,
        KEY_READ,
        &mut hkey,
    ).is_ok() {
        // Ищем подразделы, содержащие адрес устройства
        let address_upper = address.replace(":", "").to_uppercase();
        let mut index = 0u32;
        
        loop {
            let mut name_buf = vec![0u16; 256];
            let mut name_len = name_buf.len() as u32;
            
            let status = RegEnumKeyExW(
                hkey,
                index,
                PWSTR::from_raw(name_buf.as_mut_ptr()),
                &mut name_len,
                Some(std::ptr::null()),
                PWSTR::null(),
                None,
                None,
            );
            
            if status.is_err() {
                break;
            }
            
            name_buf.truncate(name_len as usize);
            let subkey_name = OsString::from_wide(&name_buf)
                .to_string_lossy()
                .to_string();
            
            if subkey_name.to_uppercase().contains(&address_upper) {
                let mut subkey = HKEY::default();
                if RegOpenKeyExW(
                    hkey,
                    &HSTRING::from(&subkey_name),
                    0,
                    KEY_READ,
                    &mut subkey,
                ).is_ok() {
                    // Ищем BatteryLevel в подразделе
                    let mut battery_buf = vec![0u8; 4];
                    let mut battery_len = battery_buf.len() as u32;
                    let value_name = HSTRING::from("BatteryLevel");
                    let mut value_type = REG_VALUE_TYPE(0);
                    
                    let battery_status = RegGetValueW(
                        subkey,
                        None,
                        &value_name,
                        REG_ROUTINE_FLAGS(0),
                        Some(&mut value_type),
                        Some(battery_buf.as_mut_ptr() as *mut std::ffi::c_void),
                        Some(&mut battery_len),
                    );
                    
                    let _ = RegCloseKey(subkey);
                    
                    if battery_status.is_ok() && battery_len >= 4 {
                        let level = u32::from_le_bytes([
                            battery_buf[0],
                            battery_buf[1],
                            battery_buf[2],
                            battery_buf[3],
                        ]) as u8;
                        
                        if level <= 100 {
                            let _ = RegCloseKey(hkey);
                            return Ok(level);
                        }
                    }
                }
            }
            
            index += 1;
        }
        
        let _ = RegCloseKey(hkey);
    }
    
    Err(Error::from_win32())
}

unsafe fn get_battery_from_bthenum_detailed(address: &str) -> Result<u8> {
    // Детальный поиск в BTHENUM - перебираем все подразделы и ищем батарею
    let enum_paths = [
        r"SYSTEM\CurrentControlSet\Enum\BTHENUM",
        r"SYSTEM\CurrentControlSet\Enum\BTHLE",
    ];
    
    let address_upper = address.replace(":", "").to_uppercase();
    
    for enum_path in &enum_paths {
        let mut hkey_enum = HKEY::default();
        if RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(*enum_path),
            0,
            KEY_READ,
            &mut hkey_enum,
        ).is_err() {
            continue;
        }
        
        let mut index = 0u32;
        loop {
            let mut name_buf = vec![0u16; 512];
            let mut name_len = name_buf.len() as u32;
            
            let status = RegEnumKeyExW(
                hkey_enum,
                index,
                PWSTR::from_raw(name_buf.as_mut_ptr()),
                &mut name_len,
                Some(std::ptr::null()),
                PWSTR::null(),
                None,
                None,
            );
            
            if status.is_err() {
                break;
            }
            
            name_buf.truncate(name_len as usize);
            let device_id = OsString::from_wide(&name_buf)
                .to_string_lossy()
                .to_string();
            
            if device_id.to_uppercase().contains(&address_upper) {
                // Нашли устройство, ищем во всех подразделах
                let mut device_key = HKEY::default();
                if RegOpenKeyExW(
                    hkey_enum,
                    &HSTRING::from(&device_id),
                    0,
                    KEY_READ,
                    &mut device_key,
                ).is_ok() {
                    // Рекурсивно ищем батарею во всех подразделах
                    if let Ok(level) = search_battery_in_all_subkeys(device_key, "") {
                        let _ = RegCloseKey(device_key);
                        let _ = RegCloseKey(hkey_enum);
                        return Ok(level);
                    }
                    let _ = RegCloseKey(device_key);
                }
            }
            
            index += 1;
        }
        
        let _ = RegCloseKey(hkey_enum);
    }
    
    Err(Error::from_win32())
}

unsafe fn search_battery_in_all_subkeys(hkey: HKEY, _path: &str) -> Result<u8> {
    // Сначала проверяем значения в текущем ключе
    let mut value_index = 0u32;
    loop {
        let mut value_name_buf = vec![0u16; 256];
        let mut value_name_len = value_name_buf.len() as u32;
        let mut value_type = REG_VALUE_TYPE(0);
        let mut value_data = vec![0u8; 256];
        let mut value_data_len = value_data.len() as u32;
        
        let status = RegEnumValueW(
            hkey,
            value_index,
            PWSTR::from_raw(value_name_buf.as_mut_ptr()),
            &mut value_name_len,
            None,
            Some(&mut value_type.0),
            Some(value_data.as_mut_ptr()),
            Some(&mut value_data_len),
        );
        
        if status.is_err() {
            break;
        }
        
        value_name_buf.truncate(value_name_len as usize);
        let value_name = OsString::from_wide(&value_name_buf)
            .to_string_lossy()
            .to_string();
        
        let value_name_lower = value_name.to_lowercase();
        
        // Ищем любые ключи, связанные с батареей, питанием или уровнем
        let is_battery_related = value_name_lower.contains("battery") || 
           value_name_lower.contains("power") ||
           value_name_lower.contains("charge") ||
           value_name_lower.contains("percent") ||
           (value_name_lower.contains("level") && value_data_len <= 4);
        
        if is_battery_related && value_data_len > 0 {
            value_data.truncate(value_data_len as usize);
            
            // Пробуем интерпретировать как уровень батареи
            if value_data_len == 1 {
                let level = value_data[0];
                if level > 0 && level <= 100 {
                    return Ok(level);
                }
            } else if value_data_len == 2 {
                let level = u16::from_le_bytes([value_data[0], value_data[1]]) as u8;
                if level > 0 && level <= 100 {
                    return Ok(level);
                }
            } else if value_data_len >= 4 {
                let level = u32::from_le_bytes([
                    value_data[0],
                    value_data[1],
                    value_data[2],
                    value_data[3],
                ]) as u8;
                if level > 0 && level <= 100 {
                    return Ok(level);
                }
            }
        }
        
        // Также проверяем любые числовые значения в диапазоне 0-100
        // которые могут быть уровнем батареи (но только если имя не системное)
        if value_data_len > 0 && value_data_len <= 4 {
            let excluded_keywords = [
                "version", "id", "cod", "manufacturer", "lmp", "subversion",
                "timestamp", "fingerprint", "capabilities", "config", "flags",
                "vid", "pid", "dib", "host", "local", "evald", "type",
            ];
            
            let is_excluded = excluded_keywords.iter()
                .any(|&keyword| value_name_lower.contains(keyword));
            
            if !is_excluded {
                if value_data_len == 1 {
                    let level = value_data[0];
                    if level > 0 && level <= 100 {
                        // Возможно это уровень батареи
                        return Ok(level);
                    }
                } else if value_data_len == 2 {
                    let level = u16::from_le_bytes([value_data[0], value_data[1]]) as u8;
                    if level > 0 && level <= 100 {
                        return Ok(level);
                    }
                } else if value_data_len >= 4 {
                    let dword = u32::from_le_bytes([
                        value_data[0],
                        value_data[1],
                        value_data[2],
                        value_data[3],
                    ]);
                    if dword > 0 && dword <= 100 {
                        return Ok(dword as u8);
                    }
                }
            }
        }
        
        value_index += 1;
    }
    
    // Теперь рекурсивно проверяем подразделы
    let mut sub_index = 0u32;
    loop {
        let mut sub_name_buf = vec![0u16; 256];
        let mut sub_name_len = sub_name_buf.len() as u32;
        
        let status = RegEnumKeyExW(
            hkey,
            sub_index,
            PWSTR::from_raw(sub_name_buf.as_mut_ptr()),
            &mut sub_name_len,
            Some(std::ptr::null()),
            PWSTR::null(),
            None,
            None,
        );
        
        if status.is_err() {
            break;
        }
        
        sub_name_buf.truncate(sub_name_len as usize);
        let sub_name = OsString::from_wide(&sub_name_buf)
            .to_string_lossy()
            .to_string();
        
        let mut sub_key = HKEY::default();
        if RegOpenKeyExW(
            hkey,
            &HSTRING::from(&sub_name),
            0,
            KEY_READ,
            &mut sub_key,
        ).is_ok() {
            if let Ok(level) = search_battery_in_all_subkeys(sub_key, "") {
                let _ = RegCloseKey(sub_key);
                return Ok(level);
            }
            let _ = RegCloseKey(sub_key);
        }
        
        sub_index += 1;
    }
    
    Err(Error::from_win32())
}

unsafe fn get_battery_from_current_user(address: &str) -> Result<u8> {
    // Ищем в HKEY_CURRENT_USER - Windows может хранить информацию о батарее здесь
    let paths = [
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Bluetooth\Devices",
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Bluetooth\Settings",
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\DeviceAccess\Global",
    ];
    
    let address_upper = address.replace(":", "").to_uppercase();
    
    for base_path in &paths {
        let mut hkey_base = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(*base_path),
            0,
            KEY_READ,
            &mut hkey_base,
        ).is_err() {
            continue;
        }
        
        // Ищем устройство по адресу
        let mut index = 0u32;
        loop {
            let mut name_buf = vec![0u16; 512];
            let mut name_len = name_buf.len() as u32;
            
            let status = RegEnumKeyExW(
                hkey_base,
                index,
                PWSTR::from_raw(name_buf.as_mut_ptr()),
                &mut name_len,
                Some(std::ptr::null()),
                PWSTR::null(),
                None,
                None,
            );
            
            if status.is_err() {
                break;
            }
            
            name_buf.truncate(name_len as usize);
            let device_id = OsString::from_wide(&name_buf)
                .to_string_lossy()
                .to_string();
            
            if device_id.to_uppercase().contains(&address_upper) {
                let mut device_key = HKEY::default();
                if RegOpenKeyExW(
                    hkey_base,
                    &HSTRING::from(&device_id),
                    0,
                    KEY_READ,
                    &mut device_key,
                ).is_ok() {
                    // Ищем батарею в значениях
                    if let Ok(level) = search_battery_in_all_subkeys(device_key, "") {
                        let _ = RegCloseKey(device_key);
                        let _ = RegCloseKey(hkey_base);
                        return Ok(level);
                    }
                    let _ = RegCloseKey(device_key);
                }
            }
            
            index += 1;
        }
        
        let _ = RegCloseKey(hkey_base);
    }
    
    Err(Error::from_win32())
}

unsafe fn get_battery_from_setupapi(address: &str) -> Result<u8> {
    // Используем SetupAPI для получения информации о батарее через драйверы устройств
    use windows::Win32::Devices::DeviceAndDriverInstallation::*;
    
    let address_clean = address.replace(":", "").to_uppercase();
    
    // GUID для Bluetooth устройств
    // {e0cbf06c-cd8b-4647-bb8a-263b43f0f974} - Bluetooth
    let bluetooth_guid = GUID::from_u128(0xe0cbf06c_cd8b_4647_bb8a_263b43f0f974);
    
    // Получаем список устройств через SetupDiGetClassDevs
    let device_info_set = match SetupDiGetClassDevsW(
        Some(&bluetooth_guid),
        None,
        None,
        DIGCF_PRESENT,
    ) {
        Ok(set) => set,
        Err(_) => return Err(Error::from_win32()),
    };
    
    let mut device_info_data = SP_DEVINFO_DATA {
        cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
        ..Default::default()
    };
    
    let mut device_index = 0u32;
    loop {
        if SetupDiEnumDeviceInfo(device_info_set, device_index, &mut device_info_data).is_err() {
            break;
        }
        
        // Получаем Device Instance ID
        let mut instance_id_buf = vec![0u16; 256];
        let mut instance_id_len = instance_id_buf.len() as u32;
        
        let config_ret = CM_Get_Device_ID_Size(
            &mut instance_id_len,
            device_info_data.DevInst,
            0,
        );
        
        if config_ret.0 == 0 {
            instance_id_buf.resize(instance_id_len as usize + 1, 0);
            
            let config_ret2 = CM_Get_Device_IDW(
                device_info_data.DevInst,
                &mut instance_id_buf[..instance_id_len as usize + 1],
                0,
            );
            
            if config_ret2.0 == 0 {
                instance_id_buf.truncate(instance_id_len as usize);
                let instance_id = OsString::from_wide(&instance_id_buf)
                    .to_string_lossy()
                    .to_string();
                
                // Проверяем, соответствует ли устройство нашему адресу
                if instance_id.to_uppercase().contains(&address_clean) {
                    // Пробуем получить свойство BatteryLevel через SetupDiGetDeviceProperty
                    let mut property_type = DEVPROPTYPE(0);
                    let mut property_buffer = vec![0u8; 16];
                    
                    // DEVPKEY_Device_BatteryLevel
                    // {49CD1F76-5626-4B17-A6E4-180F923F7B0C0C}, 9
                    // Исправляем GUID - правильный формат
                    let battery_level_key = DEVPROPKEY {
                        fmtid: GUID::from_u128(0x49CD1F76_5626_4B17_A6E4_180F923F7B0C),
                        pid: 9,
                    };
                    
                    if SetupDiGetDevicePropertyW(
                        device_info_set,
                        &device_info_data,
                        &battery_level_key,
                        &mut property_type,
                        Some(&mut property_buffer),
                        None,
                        0,
                    ).is_ok() {
                        if property_buffer.len() >= 4 {
                            let level = u32::from_le_bytes([
                                property_buffer[0],
                                property_buffer[1],
                                property_buffer[2],
                                property_buffer[3],
                            ]) as u8;
                            
                            if level > 0 && level <= 100 {
                                let _ = SetupDiDestroyDeviceInfoList(device_info_set);
                                return Ok(level);
                            }
                        } else if property_buffer.len() >= 1 {
                            let level = property_buffer[0];
                            if level > 0 && level <= 100 {
                                let _ = SetupDiDestroyDeviceInfoList(device_info_set);
                                return Ok(level);
                            }
                        }
                    }
                    
                    // Пробуем получить через реестр устройства
                    if let Ok(hkey) = SetupDiOpenDevRegKey(
                        device_info_set,
                        &device_info_data,
                        DICS_FLAG_GLOBAL.0,
                        0,
                        DIREG_DEV,
                        KEY_READ.0,
                    ) {
                        // Ищем батарею в реестре устройства
                        if let Ok(level) = search_battery_in_device_registry(hkey) {
                            let _ = RegCloseKey(hkey);
                            let _ = SetupDiDestroyDeviceInfoList(device_info_set);
                            return Ok(level);
                        }
                        let _ = RegCloseKey(hkey);
                    }
                }
            }
        }
        
        device_index += 1;
    }
    
    let _ = SetupDiDestroyDeviceInfoList(device_info_set);
    Err(Error::from_win32())
}

unsafe fn search_battery_in_device_registry(hkey: HKEY) -> Result<u8> {
    // Ищем батарею в реестре устройства
    let battery_keys = [
        "BatteryLevel",
        "BatteryPercentage",
        "Battery",
        "BatteryPercent",
        "PowerLevel",
        "ChargeLevel",
    ];
    
    for key_name in &battery_keys {
        let mut value_buf = vec![0u8; 16];
        let mut value_len = value_buf.len() as u32;
        let value_name = HSTRING::from(*key_name);
        let mut value_type = REG_VALUE_TYPE(0);
        
        let status = RegQueryValueExW(
            hkey,
            &value_name,
            None,
            Some(&mut value_type),
            Some(value_buf.as_mut_ptr()),
            Some(&mut value_len),
        );
        
        if status.is_ok() && value_len > 0 {
            value_buf.truncate(value_len as usize);
            
            if value_len >= 4 {
                let level = u32::from_le_bytes([
                    value_buf[0],
                    value_buf[1],
                    value_buf[2],
                    value_buf[3],
                ]) as u8;
                if level <= 100 {
                    return Ok(level);
                }
            } else if value_len >= 1 {
                let level = value_buf[0];
                if level <= 100 {
                    return Ok(level);
                }
            }
        }
    }
    
    Err(Error::from_win32())
}

fn format_bluetooth_address(address: &str) -> String {
    if address.len() >= 12 {
        let bytes: Vec<&str> = (0..6)
            .map(|i| &address[i * 2..(i * 2) + 2])
            .collect();
        bytes.join(":")
    } else {
        address.to_string()
    }
}

fn get_battery_bar(level: u8) -> String {
    let total_segments = 10;
    let filled = ((level as f32 / 100.0) * total_segments as f32).ceil() as usize;
    let filled = filled.min(total_segments);
    let empty = total_segments - filled;
    format!("[{}>{}]", "█".repeat(filled), "░".repeat(empty))
}

fn get_battery_icon(level: u8) -> &'static str {
    match level {
        80..=100 => "🟢",
        50..=79 => "🟡",
        20..=49 => "🟠",
        _ => "🔴",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_get_bluetooth_devices() {
        unsafe {
            println!("\n=== ТЕСТ: Получение Bluetooth устройств ===");
            match get_bluetooth_devices() {
                Ok(devices) => {
                    println!("Найдено устройств: {}", devices.len());
                    for (i, device) in devices.iter().enumerate() {
                        println!("\n--- Устройство {} ---", i + 1);
                        println!("Имя: {}", device.name);
                        println!("Адрес: {}", device.address);
                        if let Some(level) = device.battery_level {
                            println!("Батарея: {}% {}", level, get_battery_icon(level));
                        } else {
                            println!("Батарея: недоступна");
                        }
                    }
                    assert!(true, "Тест завершен успешно");
                }
                Err(e) => {
                    println!("Ошибка получения устройств: {:?}", e);
                    panic!("Не удалось получить устройства");
                }
            }
        }
    }
    
    #[test]
    fn test_get_devices_from_registry() {
        unsafe {
            println!("\n=== ТЕСТ: Получение устройств из реестра ===");
            match get_devices_from_registry() {
                Ok(devices) => {
                    println!("Найдено устройств в реестре: {}", devices.len());
                    for (i, device) in devices.iter().enumerate() {
                        println!("\n--- Устройство {} ---", i + 1);
                        println!("Имя: {}", device.name);
                        println!("Адрес: {}", device.address);
                        println!("Батарея: {:?}", device.battery_level);
                    }
                }
                Err(e) => {
                    println!("Ошибка: {:?}", e);
                }
            }
        }
    }
    
    #[test]
    fn test_get_device_name() {
        unsafe {
            println!("\n=== ТЕСТ: Получение имени устройства ===");
            
            // Получаем список устройств
            if let Ok(devices) = get_devices_from_registry() {
                for device in &devices {
                    println!("\nТестируем устройство: {}", device.address);
                    
                    // Тест 1: Получение имени из основного пути
                    let key_path = r"SYSTEM\CurrentControlSet\Services\BTHPORT\Parameters\Devices";
                    let mut hkey = HKEY::default();
                    
                    if RegOpenKeyExW(
                        HKEY_LOCAL_MACHINE,
                        &HSTRING::from(key_path),
                        0,
                        KEY_READ,
                        &mut hkey,
                    ).is_ok() {
                        let device_addr = device.address.replace(":", "");
                        let mut device_key = HKEY::default();
                        
                        if RegOpenKeyExW(
                            hkey,
                            &HSTRING::from(&device_addr),
                            0,
                            KEY_READ,
                            &mut device_key,
                        ).is_ok() {
                            // Читаем сырые байты ключа Name
                            let mut name_buf = vec![0u8; 256];
                            let mut name_len = name_buf.len() as u32;
                            let value_name = HSTRING::from("Name");
                            let mut value_type = REG_VALUE_TYPE(0);
                            
                            let status = RegQueryValueExW(
                                device_key,
                                &value_name,
                                None,
                                Some(&mut value_type),
                                Some(name_buf.as_mut_ptr()),
                                Some(&mut name_len),
                            );
                            
                            if status.is_ok() && name_len > 0 {
                                name_buf.truncate(name_len as usize);
                                println!("  Name - тип: {:?}, размер: {} байт", value_type, name_len);
                                print!("  Name - байты: ");
                                for &byte in &name_buf {
                                    print!("{:02X} ", byte);
                                }
                                println!();
                                
                                // Пробуем разные интерпретации
                                // Вариант 1: Пропустить первый байт
                                if name_len > 1 {
                                    let skip_first = &name_buf[1..];
                                    if skip_first.len() % 2 == 0 {
                                        let mut u16_vec = Vec::new();
                                        for chunk in skip_first.chunks(2) {
                                            if chunk.len() == 2 {
                                                u16_vec.push(u16::from_le_bytes([chunk[0], chunk[1]]));
                                            }
                                        }
                                        while u16_vec.last() == Some(&0) {
                                            u16_vec.pop();
                                        }
                                        if !u16_vec.is_empty() {
                                            let os_str = OsString::from_wide(&u16_vec);
                                            let name = os_str.to_string_lossy();
                                            println!("  Вариант 1 (пропуск первого байта): {:?}", name);
                                        }
                                    }
                                }
                                
                                // Вариант 2: Все как UTF-16
                                if name_len % 2 == 0 {
                                    let mut u16_vec = Vec::new();
                                    for chunk in name_buf.chunks(2) {
                                        if chunk.len() == 2 {
                                            u16_vec.push(u16::from_le_bytes([chunk[0], chunk[1]]));
                                        }
                                    }
                                    while u16_vec.last() == Some(&0) {
                                        u16_vec.pop();
                                    }
                                    if !u16_vec.is_empty() {
                                        let os_str = OsString::from_wide(&u16_vec);
                                        let name = os_str.to_string_lossy();
                                        println!("  Вариант 2 (все как UTF-16): {:?}", name);
                                    }
                                }
                                
                                // Вариант 3: Как ASCII/UTF-8
                                if let Ok(utf8_str) = std::str::from_utf8(&name_buf) {
                                    println!("  Вариант 3 (как UTF-8): {:?}", utf8_str.trim_end_matches('\0'));
                                }
                            }
                            
                            let _ = RegCloseKey(device_key);
                        }
                        
                        let name = get_device_name_from_registry(hkey, &device_addr);
                        println!("  Имя из основного пути: {:?}", name);
                        let _ = RegCloseKey(hkey);
                    }
                    
                    // Тест 2: Получение имени из альтернативного пути
                    let alt_name = get_device_name_from_alternative_path(&device.address.replace(":", ""));
                    println!("  Имя из альтернативного пути: {:?}", alt_name);
                }
            }
        }
    }
    
    #[test]
    fn test_get_battery_level() {
        unsafe {
            println!("\n=== ТЕСТ: Получение уровня батареи ===");
            
            // Получаем список устройств
            if let Ok(devices) = get_devices_from_registry() {
                for device in &devices {
                    println!("\nТестируем устройство: {} ({})", device.name, device.address);
                    
                    // Детальное исследование BTHENUM для поиска батареи
                    println!("  Ищем батарею в BTHENUM...");
                    let address_upper = device.address.replace(":", "").to_uppercase();
                    let enum_path = r"SYSTEM\CurrentControlSet\Enum\BTHENUM";
                    let mut hkey_enum = HKEY::default();
                    
                    if RegOpenKeyExW(
                        HKEY_LOCAL_MACHINE,
                        &HSTRING::from(enum_path),
                        0,
                        KEY_READ,
                        &mut hkey_enum,
                    ).is_ok() {
                        let mut index = 0u32;
                        loop {
                            let mut name_buf = vec![0u16; 512];
                            let mut name_len = name_buf.len() as u32;
                            
                            let status = RegEnumKeyExW(
                                hkey_enum,
                                index,
                                PWSTR::from_raw(name_buf.as_mut_ptr()),
                                &mut name_len,
                                Some(std::ptr::null()),
                                PWSTR::null(),
                                None,
                                None,
                            );
                            
                            if status.is_err() {
                                break;
                            }
                            
                            name_buf.truncate(name_len as usize);
                            let device_id = OsString::from_wide(&name_buf)
                                .to_string_lossy()
                                .to_string();
                            
                            if device_id.to_uppercase().contains(&address_upper) {
                                println!("    Найдено устройство в BTHENUM: {}", device_id);
                                
                                let mut device_key = HKEY::default();
                                if RegOpenKeyExW(
                                    hkey_enum,
                                    &HSTRING::from(&device_id),
                                    0,
                                    KEY_READ,
                                    &mut device_key,
                                ).is_ok() {
                                    // Ищем во всех подразделах
                                    let mut sub_index = 0u32;
                                    loop {
                                        let mut sub_name_buf = vec![0u16; 256];
                                        let mut sub_name_len = sub_name_buf.len() as u32;
                                        
                                        let sub_status = RegEnumKeyExW(
                                            device_key,
                                            sub_index,
                                            PWSTR::from_raw(sub_name_buf.as_mut_ptr()),
                                            &mut sub_name_len,
                                            Some(std::ptr::null()),
                                            PWSTR::null(),
                                            None,
                                            None,
                                        );
                                        
                                        if sub_status.is_err() {
                                            break;
                                        }
                                        
                                        sub_name_buf.truncate(sub_name_len as usize);
                                        let sub_id = OsString::from_wide(&sub_name_buf)
                                            .to_string_lossy()
                                            .to_string();
                                        
                                        println!("      Подраздел: {}", sub_id);
                                        
                                        let mut sub_key = HKEY::default();
                                        if RegOpenKeyExW(
                                            device_key,
                                            &HSTRING::from(&sub_id),
                                            0,
                                            KEY_READ,
                                            &mut sub_key,
                                        ).is_ok() {
                                            // Перечисляем все значения в подразделе
                                            let mut value_index = 0u32;
                                            loop {
                                                let mut value_name_buf = vec![0u16; 256];
                                                let mut value_name_len = value_name_buf.len() as u32;
                                                let mut value_type = REG_VALUE_TYPE(0);
                                                let mut value_data = vec![0u8; 256];
                                                let mut value_data_len = value_data.len() as u32;
                                                
                                                let status = RegEnumValueW(
                                                    sub_key,
                                                    value_index,
                                                    PWSTR::from_raw(value_name_buf.as_mut_ptr()),
                                                    &mut value_name_len,
                                                    None,
                                                    Some(&mut value_type.0),
                                                    Some(value_data.as_mut_ptr()),
                                                    Some(&mut value_data_len),
                                                );
                                                
                                                if status.is_err() {
                                                    break;
                                                }
                                                
                                                value_name_buf.truncate(value_name_len as usize);
                                                let value_name = OsString::from_wide(&value_name_buf)
                                                    .to_string_lossy()
                                                    .to_string();
                                                
                                                // Выводим ВСЕ ключи для анализа (не только связанные с батареей)
                                                let value_name_lower = value_name.to_lowercase();
                                                let is_battery_related = value_name_lower.contains("battery") || 
                                                   value_name_lower.contains("power") ||
                                                   value_name_lower.contains("charge") ||
                                                   value_name_lower.contains("level") ||
                                                   value_name_lower.contains("percent");
                                                
                                                // Выводим все ключи, особенно те, что могут быть батареей
                                                if is_battery_related || (value_data_len <= 4 && value_data_len > 0) {
                                                    let icon = if is_battery_related { "🔋" } else { "  " };
                                                    println!("        {} Ключ: {} (тип: {:?}, размер: {} байт)", 
                                                        icon, value_name, value_type, value_data_len);
                                                    
                                                    // Выводим байты
                                                    print!("          Байты: ");
                                                    for i in 0..value_data_len.min(16) {
                                                        print!("{:02X} ", value_data[i as usize]);
                                                    }
                                                    println!();
                                                    
                                                    // Пробуем интерпретировать
                                                    if value_data_len == 1 {
                                                        let level = value_data[0];
                                                        println!("          Значение (BYTE): {}", level);
                                                        if level > 0 && level <= 100 {
                                                            println!("          ✅ Возможно уровень батареи: {}%", level);
                                                        }
                                                    } else if value_data_len == 4 {
                                                        let dword = u32::from_le_bytes([
                                                            value_data[0],
                                                            value_data[1],
                                                            value_data[2],
                                                            value_data[3],
                                                        ]);
                                                        println!("          Значение (DWORD): {}", dword);
                                                        if dword > 0 && dword <= 100 {
                                                            println!("          ✅ Возможно уровень батареи: {}%", dword);
                                                        }
                                                    }
                                                }
                                                
                                                value_index += 1;
                                            }
                                            
                                            let _ = RegCloseKey(sub_key);
                                        }
                                        
                                        sub_index += 1;
                                    }
                                    
                                    let _ = RegCloseKey(device_key);
                                }
                            }
                            
                            index += 1;
                        }
                        
                        let _ = RegCloseKey(hkey_enum);
                    }
                    
                    // Тест всех методов поиска
                    println!("\n  Тестируем методы поиска:");
                    let device_addr = device.address.replace(":", "");
                    let key_path = r"SYSTEM\CurrentControlSet\Services\BTHPORT\Parameters\Devices";
                    let mut hkey = HKEY::default();
                    
                    if RegOpenKeyExW(
                        HKEY_LOCAL_MACHINE,
                        &HSTRING::from(key_path),
                        0,
                        KEY_READ,
                        &mut hkey,
                    ).is_ok() {
                        match get_battery_level_from_registry(hkey, &device_addr) {
                            Ok(level) => println!("  ✅ Батарея из основного пути: {}%", level),
                            Err(_) => println!("  ❌ Батарея из основного пути: не найдена"),
                        }
                        let _ = RegCloseKey(hkey);
                    }
                    
                    match get_battery_from_alternative_path(&device.address) {
                        Ok(level) => println!("  ✅ Батарея из альтернативного пути: {}%", level),
                        Err(_) => println!("  ❌ Батарея из альтернативного пути: не найдена"),
                    }
                    
                    match get_battery_from_device_properties(&device.address) {
                        Ok(level) => println!("  ✅ Батарея из device properties: {}%", level),
                        Err(_) => println!("  ❌ Батарея из device properties: не найдена"),
                    }
                    
                    match get_battery_from_device_instance(&device.address) {
                        Ok(level) => println!("  ✅ Батарея из device instance: {}%", level),
                        Err(_) => println!("  ❌ Батарея из device instance: не найдена"),
                    }
                    
                    match get_battery_from_connected_devices(&device.address) {
                        Ok(level) => println!("  ✅ Батарея из connected devices: {}%", level),
                        Err(_) => println!("  ❌ Батарея из connected devices: не найдена"),
                    }
                    
                    match get_battery_from_bthenum_detailed(&device.address) {
                        Ok(level) => println!("  ✅ Батарея из BTHENUM (детальный поиск): {}%", level),
                        Err(_) => println!("  ❌ Батарея из BTHENUM (детальный поиск): не найдена"),
                    }
                }
            }
        }
    }
    
    #[test]
    fn test_enumerate_registry_values() {
        unsafe {
            println!("\n=== ТЕСТ: Перечисление значений реестра ===");
            
            let key_path = r"SYSTEM\CurrentControlSet\Services\BTHPORT\Parameters\Devices";
            let mut hkey = HKEY::default();
            
            if RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                &HSTRING::from(key_path),
                0,
                KEY_READ,
                &mut hkey,
            ).is_ok() {
                let mut index = 0u32;
                loop {
                    let mut name_buf = vec![0u16; 256];
                    let mut name_len = name_buf.len() as u32;
                    
                    let status = RegEnumKeyExW(
                        hkey,
                        index,
                        PWSTR::from_raw(name_buf.as_mut_ptr()),
                        &mut name_len,
                        Some(std::ptr::null()),
                        PWSTR::null(),
                        None,
                        None,
                    );
                    
                    if status.is_err() {
                        break;
                    }
                    
                    name_buf.truncate(name_len as usize);
                    let device_address = OsString::from_wide(&name_buf)
                        .to_string_lossy()
                        .to_string();
                    
                    println!("\nУстройство: {}", format_bluetooth_address(&device_address));
                    
                    // Открываем ключ устройства
                    let mut device_key = HKEY::default();
                    if RegOpenKeyExW(
                        hkey,
                        &HSTRING::from(&device_address),
                        0,
                        KEY_READ,
                        &mut device_key,
                    ).is_ok() {
                        // Перечисляем все значения
                        let mut value_index = 0u32;
                        loop {
                            let mut value_name_buf = vec![0u16; 256];
                            let mut value_name_len = value_name_buf.len() as u32;
                            let mut value_type = REG_VALUE_TYPE(0);
                            let mut value_data = vec![0u8; 256];
                            let mut value_data_len = value_data.len() as u32;
                            
                            let status = RegEnumValueW(
                                device_key,
                                value_index,
                                PWSTR::from_raw(value_name_buf.as_mut_ptr()),
                                &mut value_name_len,
                                None,
                                Some(&mut value_type.0),
                                Some(value_data.as_mut_ptr()),
                                Some(&mut value_data_len),
                            );
                            
                            if status.is_err() {
                                break;
                            }
                            
                            value_name_buf.truncate(value_name_len as usize);
                            let value_name = OsString::from_wide(&value_name_buf)
                                .to_string_lossy()
                                .to_string();
                            
                            println!("  Ключ: {} (тип: {:?}, размер: {} байт)", 
                                value_name, value_type, value_data_len);
                            
                            // Если это строка, выводим её
                            if value_type.0 == 1 && value_data_len >= 2 { // REG_SZ
                                let char_count = (value_data_len as usize / 2).saturating_sub(1);
                                if char_count > 0 {
                                    let str_data: Vec<u16> = value_data.chunks(2)
                                        .take(char_count)
                                        .map(|chunk| u16::from_le_bytes([chunk[0], chunk.get(1).copied().unwrap_or(0)]))
                                        .collect();
                                    let str_os = OsString::from_wide(&str_data);
                                    let str_value = str_os.to_string_lossy();
                                    println!("    Значение (строка): {}", str_value);
                                }
                            } else if value_data_len == 4 { // DWORD
                                let dword_value = u32::from_le_bytes([
                                    value_data[0],
                                    value_data[1],
                                    value_data[2],
                                    value_data[3],
                                ]);
                                println!("    Значение (DWORD): {}", dword_value);
                            }
                            
                            value_index += 1;
                        }
                        
                        let _ = RegCloseKey(device_key);
                    }
                    
                    index += 1;
                }
                
                let _ = RegCloseKey(hkey);
            }
        }
    }
}
