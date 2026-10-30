# Разбор записей USB-трафика

## Требования

- Wireshark с `tshark` (`C:\Program Files\Wireshark\tshark.exe`);
- записи `.pcap`/`.pcapng` в `local/captures/`.

Записи делаются через USBPcap. Инструкция: `docs/capturing.md`.

## `aj199_capture.py`

```powershell
python tools\capture\aj199_capture.py summary local\captures\startup_read_all_dongle.pcap
python tools\capture\aj199_capture.py writes local\captures\dpi_stage1_1600_to_800_dongle.pcap
python tools\capture\aj199_capture.py profile local\captures\startup_read_all_dongle.pcap --json local\re\dongle.json
python tools\capture\aj199_capture.py profile local\captures\startup_read_all_wired.pcap --json local\re\wired.json
python tools\capture\aj199_capture.py diff-profiles local\re\wired.json local\re\dongle.json
```

Команды:

- `summary` — число Feature/Input Report, команды, проверка checksum.
- `writes` — все `WriteEEPROM (0x07)` с адресом, длиной и данными.
- `profile` — восстановление EEPROM по `ReadEEPROM (0x08)`, DPI, цвета, кнопки, параметры.
- `diff-profiles` — сравнение двух JSON-профилей.

Сырые `.pcap` и `.txt` лежат в `local/captures/` и не коммитятся.
