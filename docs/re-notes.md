# Заметки по реверс-инжинирингу

## Инструменты

- Ghidra 12.2 + JDK 25 — в `local/tools/` (не входит в репозиторий).
- `pefile` — в `tools/re/requirements.txt`.
- Скрипты Ghidra: `tools/re/ghidra/scripts/`.

## Ключевые функции `OemDrv.exe`

| Адрес | Имя | Назначение |
|---|---|---|
| `0x0040d1d0` | `Psd_Thread` | поиск/подключение, CID/MID, версия, запуск `ServiceThread` |
| `0x0040d6c0` | `FindDev` | перечисление HID-интерфейсов, выбор `hCmd`/`hNotify` |
| `0x0040f660` | `GetProfile` | чтение профиля, DPI, LED, кнопок, параметров, макросов |
| `0x00410050` | `EEPROM_To_MainRGB` | разбор основного LED-эффекта |
| `0x004102c0` | `EEPROM_To_LogoRGB` | разбор LED логотипа |
| `0x004103e0` | `keyinfo_to_hardware_code` | преобразование кнопки в hardware code |
| `0x00410910` | `HdMacro_To_StMacro` | аппаратный макрос → структура |
| `0x00410bb0` | `StMacro_To_HdMacro` | структура → аппаратный макрос |
| `0x00410ee0` | `Apply` | запись DPI/LED/кнопок/параметров/макросов |
| `0x0044fd50` | `Notification` | приём 17-байтного Input Report |
| `0x0044fee0` | `WaitNotify` | ожидание уведомления по Command ID |
| `0x004503a0` | `GetConnectStatus` | команда `0x03` |
| `0x004504e0` | `CheckPsd` | команда `0x01`, CID/MID |
| `0x00450700` | `GetData` | чтение ответа с таймаутом |
| `0x00450820` | `SendData` | сборка/отправка Feature Report `0x08` |
| `0x00450910` | `SendCMD` | команда `0x02` и общая отправка |
| `0x00450a80` | `TellDrvStatus` | статус приложения |
| `0x00450bb0` | `GetCurProfile` | команда `0x0E` |
| `0x00450ce0` | `SetCurProfile` | команда `0x10` |
| `0x00450dc0` | `WriteEEPROM` | команда `0x07` |
| `0x00451080` | `ReadROM` | чтение блоками по 10 байт |
| `0x00451120` | `ReadEEPROM` | команда `0x08` |
| `0x00451290` | `SetDpiColor` | запись цветов DPI, адрес `0x2C` |
| `0x004513b0` | `SetDpi` | запись DPI, адрес `0x0C` |
| `0x00451640` | `SetKeyMatrix` | запись кнопок, адрес `0x60` |
| `0x00451780` | `SetPolling` | запись частоты опроса, адрес `0x00` |
| `0x004518e0` | `GetPower` | команда `0x04` |
| `0x004519f0` | `BuildAndSendFeature` | checksum и `HidD_SetFeature` |

## Макросы

- `StMacro_To_HdMacro` (VA `0x410bb0`) конвертирует `.jmm`-события в 5-байтные hardware-события.
- Таблица software key → HID: VA `0x5B2EB8`, 0x68 пар `(hid, key)`, raw `0x1B1EB8`.
- Спецклавиши: `0x10/0xA0 → 2`, `0x11/0xA2 → 1`, `0x12/0xA4 → 4`, `0x5B/0x5C → 8`, `0xA1 → 0x20`, `0xA3 → 0x10`, `0xA5 → 0x40`; медиа `0xF0..0xF4 → 1/2/4/0x10/8` с `kind=4`.
- `flags = ((state & 1) + 1) * 0x40 | kind`; `kind=0` special, `1` HID, `4` media, `5` wheel. `state=1` — down, `0` — up.
- `HdMacro_To_StMacro` (VA `0x410910`) делает обратное; `flags == 5` — wheel, иначе `kind = flags & 7`, `state = flags & 0x80`.
- Слот `0x300 + i * 0x180`: `+0x00` длина имени, `+0x01` имя, `+0x1F` count, `+0x20` события.
- `.jmm` writer в OEM: VA `0x410940`, magic, timestamp в `+0x04`, count в `+0x10`, события с `+0x6A`; файл поворачивается вправо на 2 бита.

## Как воспроизвести

```powershell
$env:JAVA_HOME = "$PWD\local\tools\jdk-25.0.5+8"
local\tools\ghidra_12.2_PUBLIC\support\analyzeHeadless `
  "$PWD\local\re\ghidra_project" AJ199 `
  -import "$PWD\local\original\OemDrv.exe" -overwrite
```

Затем:

```powershell
local\tools\ghidra_12.2_PUBLIC\support\analyzeHeadless `
  "$PWD\local\re\ghidra_project" AJ199 -process OemDrv.exe `
  -scriptPath "$PWD\tools\re\ghidra\scripts" `
  -postScript DumpAj199Anchors.java "$PWD\local\re\ghidra_out" -noanalysis
```

Скрипт `DecompileAt.java` декомпилирует функции по адресам, `ListCallers.java` показывает вызывающих.

## Важно

- Адреса выше — RVA/VA в образе `0x400000`, не файловые смещения.
- Сырые выгрузки лежат в `local/re/` и не коммитятся.
- Если Ghidra не установлена, анализ через `pefile` даст только строки/импорты, но не логику.
