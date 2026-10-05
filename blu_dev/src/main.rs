use anyhow::Result;
use btleplug::api::{Central, CentralEvent, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Manager, PeripheralId};
use eframe::egui;
use futures_util::StreamExt;
use image::{ImageBuffer, Rgba};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::mpsc;
use tray_icon::{menu::{Menu, MenuItem, MenuEvent}, TrayIconBuilder};
use uuid::Uuid;

// Battery Service UUID (зарезервировано для будущего использования)
#[allow(dead_code)]
const BATTERY_SERVICE_UUID: Uuid = Uuid::from_u128(0x0000180f00001000800000805f9b34fb);
// Battery Level Characteristic UUID
const BATTERY_LEVEL_CHAR_UUID: Uuid = Uuid::from_u128(0x00002a1900001000800000805f9b34fb);

#[derive(Clone, Debug)]
struct DeviceInfo {
    name: String,
    address: String,
    battery_level: Option<u8>,
    connected: bool,
}

enum AppCommand {
    ShowWindow,
    Exit,
}

struct BatteryMonitorApp {
    devices: Arc<Mutex<HashMap<String, DeviceInfo>>>,
    adapter: Option<Adapter>,
    auto_scan: bool,
    auto_start_enabled: bool,
    scan_in_progress: Arc<Mutex<bool>>,
    tray_icon: Option<Arc<Mutex<tray_icon::TrayIcon>>>,
    window_visible: bool,
    command_receiver: mpsc::Receiver<AppCommand>,
    command_sender: mpsc::Sender<AppCommand>,
}

impl BatteryMonitorApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        
        let devices = Arc::new(Mutex::new(HashMap::new()));
        let auto_start_enabled = check_auto_start();
        
        let (tx, rx) = mpsc::channel();
        let tray_icon = Self::create_tray_icon(tx.clone(), devices.clone());
        
        Self {
            devices,
            adapter: None,
            auto_scan: true,
            auto_start_enabled,
            scan_in_progress: Arc::new(Mutex::new(false)),
            tray_icon: Some(tray_icon),
            window_visible: true,
            command_receiver: rx,
            command_sender: tx,
        }
    }
    
    fn create_tray_icon(
        _sender: mpsc::Sender<AppCommand>,
        _devices: Arc<Mutex<HashMap<String, DeviceInfo>>>,
    ) -> Arc<Mutex<tray_icon::TrayIcon>> {
        // Создаем простую иконку батареи (16x16 пикселей)
        let icon_data = Self::create_battery_icon();
        
        let menu = Menu::new();
        let show_item = MenuItem::with_id("show", "Показать окно", true, None);
        let exit_item = MenuItem::with_id("exit", "Выход", true, None);
        
        menu.append(&show_item).unwrap();
        menu.append(&exit_item).unwrap();
        
        let tray_icon = Arc::new(Mutex::new(
            TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_tooltip("Монитор батареи Bluetooth")
                .with_icon(icon_data)
                .build()
                .unwrap()
        ));
        
        // Слушатель меню обрабатывается в основном потоке (update)
        
        tray_icon
    }
    
    fn generate_tooltip(devices: &Arc<Mutex<HashMap<String, DeviceInfo>>>) -> String {
        let devs = devices.lock().unwrap();
        if devs.is_empty() {
            return "Монитор батареи Bluetooth\nУстройства не найдены".to_string();
        }
        
        let mut tooltip = String::from("Монитор батареи Bluetooth\n\n");
        for (_, device) in devs.iter() {
            let name = Self::format_device_name(&device.name);
            if let Some(level) = device.battery_level {
                tooltip.push_str(&format!("{}: {}%\n", name, level));
            } else {
                tooltip.push_str(&format!("{}: N/A\n", name));
            }
        }
        
        tooltip.trim().to_string()
    }
    
    fn format_device_name(raw_name: &str) -> String {
        // Улучшаем отображение названий устройств
        let name = raw_name.trim();
        
        // Если название пустое или только адрес, оставляем как есть
        if name.is_empty() || name.len() < 3 {
            return name.to_string();
        }
        
        // Убираем лишние пробелы и форматируем
        let cleaned: String = name.chars()
            .take(30) // Ограничиваем длину
            .collect();
        
        cleaned
    }
    
    fn create_battery_icon() -> tray_icon::Icon {
        // Создаем простую иконку батареи (зеленую)
        let img = ImageBuffer::from_fn(32, 32, |x, y| {
            // Рисуем простую батарею
            if (x >= 8 && x < 24 && y >= 8 && y < 22) || (x >= 24 && x < 26 && y >= 11 && y < 19) {
                Rgba([100u8, 200u8, 100u8, 255u8]) // Зеленый
            } else {
                Rgba([0u8, 0u8, 0u8, 0u8]) // Прозрачный
            }
        });
        
        let rgba = img.into_raw();
        tray_icon::Icon::from_rgba(rgba, 32, 32).unwrap()
    }
    
    fn create_battery_icon_with_level(devices: &Arc<Mutex<HashMap<String, DeviceInfo>>>) -> tray_icon::Icon {
        let devs = devices.lock().unwrap();
        
        // Находим средний уровень батареи или минимальный
        let mut levels: Vec<u8> = devs.values()
            .filter_map(|d| d.battery_level)
            .collect();
        
        if levels.is_empty() {
            return Self::create_battery_icon();
        }
        
        levels.sort();
        let min_level = levels[0];
        
        // Цвет в зависимости от уровня
        let color = if min_level > 50 {
            [100u8, 200u8, 100u8, 255u8] // Зеленый
        } else if min_level > 20 {
            [255u8, 200u8, 100u8, 255u8] // Желтый
        } else {
            [255u8, 100u8, 100u8, 255u8] // Красный
        };
        
        let img = ImageBuffer::from_fn(32, 32, |x, y| {
            // Рисуем батарею с уровнем
            if (x >= 8 && x < 24 && y >= 8 && y < 22) || (x >= 24 && x < 26 && y >= 11 && y < 19) {
                // Заливка в зависимости от уровня
                let fill_width = (16.0 * (min_level as f32 / 100.0)) as u32;
                if x >= 8 && x < 8 + fill_width && y >= 8 && y < 22 {
                    Rgba(color)
                } else {
                    Rgba([50u8, 50u8, 50u8, 255u8]) // Серый контур
                }
            } else {
                Rgba([0u8, 0u8, 0u8, 0u8]) // Прозрачный
            }
        });
        
        let rgba = img.into_raw();
        tray_icon::Icon::from_rgba(rgba, 32, 32).unwrap()
    }

    fn init_bluetooth(&mut self) -> Result<()> {
        let rt = tokio::runtime::Runtime::new()?;
        
        rt.block_on(async {
            let manager = Manager::new().await?;
            let adapters = manager.adapters().await?;
            
            if adapters.is_empty() {
                anyhow::bail!("Не найдено Bluetooth адаптеров");
            }

            self.adapter = Some(adapters.into_iter().next().unwrap());
            Ok::<(), anyhow::Error>(())
        })?;

        Ok(())
    }

    fn start_scan(&self) -> Result<()> {
        let mut scan_lock = self.scan_in_progress.lock().unwrap();
        if *scan_lock {
            return Ok(());
        }
        *scan_lock = true;
        drop(scan_lock);

        if let Some(adapter) = &self.adapter {
            let devices = Arc::clone(&self.devices);
            let adapter_clone = adapter.clone();
            let scan_in_progress = Arc::clone(&self.scan_in_progress);

            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                
                rt.block_on(async {
                    if let Err(_) = adapter_clone.start_scan(ScanFilter::default()).await {
                        *scan_in_progress.lock().unwrap() = false;
                        return;
                    }
                    
                    let mut events = match adapter_clone.events().await {
                        Ok(stream) => stream,
                        Err(_) => {
                            *scan_in_progress.lock().unwrap() = false;
                            return;
                        }
                    };
                    
                    // Сканируем в течение 10 секунд
                    let timeout = tokio::time::sleep(tokio::time::Duration::from_secs(10));
                    tokio::pin!(timeout);

                    loop {
                        tokio::select! {
                            _ = &mut timeout => {
                                break;
                            }
                            event = events.next() => {
                                if let Some(event) = event {
                                    match event {
                                        CentralEvent::DeviceDiscovered(id) => {
                                            if let Ok(peripheral) = adapter_clone.peripheral(&id).await {
                                                if let Ok(properties) = peripheral.properties().await {
                                                    if let Some(props) = properties {
                                                        let raw_name = props.local_name.unwrap_or_else(|| {
                                                            format!("Устройство {}", &props.address.to_string()[..8])
                                                        });
                                                        
                                                        let device_info = DeviceInfo {
                                                            name: Self::format_device_name(&raw_name),
                                                            address: props.address.to_string(),
                                                            battery_level: None,
                                                            connected: false,
                                                        };
                                                        
                                                        let mut devs = devices.lock().unwrap();
                                                        devs.insert(id.to_string(), device_info);
                                                        drop(devs);
                                                        
                                                        // Попытка подключиться и прочитать батарею
                                                        Self::try_read_battery(&adapter_clone, &id, &devices).await;
                                                    }
                                                }
                                            }
                                        }
                                        CentralEvent::DeviceUpdated(id) => {
                                            Self::try_read_battery(&adapter_clone, &id, &devices).await;
                                        }
                                        _ => {}
                                    }
                                } else {
                                    break;
                                }
                            }
                        }
                    }
                    
                    *scan_in_progress.lock().unwrap() = false;
                })
            });
        }
        
        Ok(())
    }

    async fn try_read_battery(
        adapter: &Adapter,
        device_id: &PeripheralId,
        devices: &Arc<Mutex<HashMap<String, DeviceInfo>>>,
    ) {
        if let Ok(peripheral) = adapter.peripheral(device_id).await {
            if !peripheral.is_connected().await.unwrap_or(false) {
                if let Err(_) = peripheral.connect().await {
                    return;
                }
            }

            // Даем время на подключение
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            if let Ok(_) = peripheral.discover_services().await {
                let characteristics = peripheral.characteristics();
                for char in characteristics {
                    if char.uuid == BATTERY_LEVEL_CHAR_UUID {
                        if let Ok(data) = peripheral.read(&char).await {
                            if let Some(&battery_level) = data.first() {
                                let mut devs = devices.lock().unwrap();
                                if let Some(device) = devs.get_mut(&device_id.to_string()) {
                                    device.battery_level = Some(battery_level);
                                    device.connected = true;
                                }
                            }
                        }
                        break;
                    }
                }
            }
            
            // Отключаемся после чтения
            let _ = peripheral.disconnect().await;
            
            let mut devs = devices.lock().unwrap();
            if let Some(device) = devs.get_mut(&device_id.to_string()) {
                device.connected = false;
            }
        }
    }
}

impl eframe::App for BatteryMonitorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Обработка кликов меню из системного трея
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            let id_str: &str = &ev.id.0;
            if id_str == "show" {
                let _ = self.command_sender.send(AppCommand::ShowWindow);
            } else if id_str == "exit" {
                let _ = self.command_sender.send(AppCommand::Exit);
            }
        }
        
        // Обработка закрытия окна - сворачиваем в трей
        if ctx.input(|i| i.viewport().close_requested()) {
            self.window_visible = false;
            ctx.send_viewport_cmd_to(ctx.viewport_id(), egui::ViewportCommand::Visible(false));
        }
        
        // Обновляем tooltip периодически
        if let Some(tray_icon) = &self.tray_icon {
            let tooltip = Self::generate_tooltip(&self.devices);
            if let Ok(icon) = tray_icon.lock() {
                let _ = icon.set_tooltip(Some(&tooltip));
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Монитор батареи Bluetooth устройств");
            
            ui.separator();
            
            // Кнопка инициализации Bluetooth
            if self.adapter.is_none() {
                if ui.button("Инициализировать Bluetooth").clicked() {
                    if let Err(e) = self.init_bluetooth() {
                        ui.label(format!("Ошибка: {}", e));
                    } else {
                        if let Err(e) = self.start_scan() {
                            ui.label(format!("Ошибка сканирования: {}", e));
                        }
                    }
                }
            } else {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.auto_scan, "Автоматическое сканирование");
                    
                    if ui.button("Обновить").clicked() {
                        if let Err(e) = self.start_scan() {
                            ui.label(format!("Ошибка: {}", e));
                        }
                    }
                });
                
                ui.separator();
                
                // Автозапуск
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.auto_start_enabled, "Автозапуск при старте Windows");
                    if ui.button("Применить").clicked() {
                        if self.auto_start_enabled {
                            set_auto_start().unwrap_or_default();
                        } else {
                            remove_auto_start().unwrap_or_default();
                        }
                    }
                });
                
                ui.separator();
                
                ui.label("💡 При закрытии окна приложение сворачивается в системный трей");
                ui.separator();
                
                // Список устройств
                let devices = self.devices.lock().unwrap();
                if devices.is_empty() {
                    ui.label("Устройства не найдены. Сканирование...");
                } else {
                    ui.heading("Найденные устройства:");
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for (_, device) in devices.iter() {
                            ui.group(|ui| {
                                let display_name = Self::format_device_name(&device.name);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(&display_name)
                                            .size(16.0)
                                    );
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if let Some(level) = device.battery_level {
                                            // Индикатор батареи
                                            let color = if level > 50 {
                                                egui::Color32::GREEN
                                            } else if level > 20 {
                                                egui::Color32::YELLOW
                                            } else {
                                                egui::Color32::RED
                                            };
                                            
                                            ui.label(
                                                egui::RichText::new(format!("{}%", level))
                                                    .color(color)
                                                    .size(18.0)
                                                    .strong()
                                            );
                                            
                                            // Индикатор прогресса
                                            let progress = level as f32 / 100.0;
                                            ui.add(
                                                egui::ProgressBar::new(progress)
                                                    .show_percentage()
                                                    .animate(true)
                                            );
                                        } else {
                                            ui.label(
                                                egui::RichText::new("N/A")
                                                    .color(egui::Color32::GRAY)
                                            );
                                        }
                                    });
                                });
                                ui.label(
                                    egui::RichText::new(format!("Адрес: {}", device.address))
                                        .color(egui::Color32::GRAY)
                                        .small()
                                );
                            });
                            ui.add_space(5.0);
                        }
                    });
                }
            }
        });

        // Периодическое обновление при включенном автосканировании
        if self.auto_scan && self.adapter.is_some() {
            let scan_lock = self.scan_in_progress.lock().unwrap();
            if !*scan_lock {
                drop(scan_lock);
                ctx.request_repaint_after(std::time::Duration::from_secs(15));
                let _ = self.start_scan();
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(500));
            }
        }
    }
}

// Функции для автозапуска Windows
fn check_auto_start() -> bool {
    use winreg::enums::*;
    use winreg::RegKey;
    
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path = r"Software\Microsoft\Windows\CurrentVersion\Run";
    
    if let Ok(key) = hkcu.open_subkey(path) {
        if let Ok(_) = key.get_value::<String, _>("BluDevBatteryMonitor") {
            return true;
        }
    }
    
    false
}

fn set_auto_start() -> Result<()> {
    use winreg::enums::*;
    use winreg::RegKey;
    use std::env;
    
    let exe_path = env::current_exe()?.to_string_lossy().to_string();
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path = r"Software\Microsoft\Windows\CurrentVersion\Run";
    
    let (key, _) = hkcu.create_subkey(path)?;
    key.set_value("BluDevBatteryMonitor", &exe_path)?;
    
    Ok(())
}

fn remove_auto_start() -> Result<()> {
    use winreg::enums::*;
    use winreg::RegKey;
    
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path = r"Software\Microsoft\Windows\CurrentVersion\Run";
    
    if let Ok(key) = hkcu.open_subkey(path) {
        let _ = key.delete_value("BluDevBatteryMonitor");
    }
    
    Ok(())
}

fn main() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([650.0, 550.0])
            .with_title("Bluetooth Battery Monitor")
            .with_close_button(true),
        ..Default::default()
    };

    if let Err(e) = eframe::run_native(
        "Bluetooth Battery Monitor",
        options,
        Box::new(|cc| Box::new(BatteryMonitorApp::new(cc))),
    ) {
        eprintln!("Ошибка запуска приложения: {}", e);
    }
}
