# Архитектура

## Слои

1. **`aj199-protocol`** — только байты: сборка Feature Report, разбор Input Report, контрольная сумма, кодирование/декодирование DPI, кнопок, светодиодов и макросов. Не знает, подключена мышь кабелем или через приёмник.
2. **`aj199-device`** — реализация `Transport`, перечисление HID-интерфейсов, выбор `hCmd`/`hNotify` по свойствам, таймауты, повторы, горячее переподключение, очередь команд при сне.
3. **`model`** — профили и состояние в JSON, backup/export/import.
4. **`apps/gui`** — Tauri 2 + TypeScript: четыре страницы, статус, tray, single instance.

## Транспорт

```rust
pub trait Transport {
    fn find_devices(&self) -> Result<Vec<DeviceSet>, TransportError>;
    fn open(&mut self, device: &DeviceSet) -> Result<(), TransportError>;
    fn send_feature(&mut self, frame: &FeatureFrame) -> Result<(), TransportError>;
    fn read_notify(&mut self, timeout: Duration) -> Result<Option<InputFrame>, TransportError>;
    fn close(&mut self);
}
```

Поиск интерфейсов идёт по VID/PID и фактическим возможностям отчётов, а не по номеру интерфейса. Для каждого кандидата логируются путь, номер интерфейса, usage page, usage, длины input/output/feature и report ID.

## Tauri-команды

`get_state`, `get_status`, `set_write_enabled`, `list_interfaces`, `backup`,
`export_profile`, `restore_profile`, `get_macros`, `set_dpi`, `set_current_stage`,
`test_write`, `set_dpi_color`, `set_polling`, `set_debounce`, `set_lod`,
`set_angle_snap`, `set_ripple`, `set_led`, `set_key`, `reset_profile`,
`set_macro_name`, `assign_macro`, `read_macro`, `write_macro`, `export_macro`,
`encode_jmm`, `parse_jmm`.

Доступ к HID сериализуется мьютексом. Устройство открывается на время команды;
открытие повторяется до 3 раз с паузой. Разрешение записи и профильные действия
находятся в меню ⋮.
Перед записью создаётся JSON-backup профиля в `%APPDATA%\com.aj199.control\backups`;
перед записью макроса дополнительно сохраняется JSON старого макроса в `backups\macros`.

## Жизненный цикл окна и трей

- Главное окно создаётся программно (`WebviewWindowBuilder`), а не из `tauri.conf.json`.
- Закрытие (крестик) и сворачивание вызывают `destroy()`: WebView2 и его процессы завершаются.
- Rust-процесс продолжает жить в трее; `RunEvent::ExitRequested` блокируется, пока не выбран «Выход».
- Повторный запуск перехватывает single-instance и пересоздаёт окно в первом процессе.
- Пока окна нет, фоновый поток опрашивает устройство раз в 60 с; при ошибке EEPROM (мышь спит)
  опрос останавливается до открытия окна или действия в трее.
- Трей-меню: состояние/заряд/DPI, смена ступени (только при включённой записи), автозапуск, выход.
- `--minimized` (автозапуск) не создаёт WebView2.

## Правила записи

- Все адреса записи берутся из `docs/protocol.md`.
- Запись: backup → запись → чтение обратно. При ошибке возвращается ошибка; полный автоматический откат пока не реализован.
- Режим `read-only` включён по умолчанию; в CLI нужен `--allow-write`, в GUI — тумблер «Разрешить запись».
- Очередь команд при сне пока не реализована: UI показывает офлайн и позволяет повторить.

## Производительность

- Старт GUI: около 1 секунды (проверено на debug-сборке).
- Простой CPU: около 0 %.
- Опрос подключения: раз в 2.5 секунды.
- HID-обмен не блокирует webview, но команды выполняются последовательно.

## Безопасность

- Нет кода прошивки, bootloader и обновления.
- Нет записи по неопределённым адресам.
- Нет копирования бинарников и графики OEM-софта.
