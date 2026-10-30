# Запись USB-трафика (этап B)

Одно изменение настройки — один файл. Сырые записи лежат в `local/captures/` и не коммитятся.

## 1. Установка

1. **Wireshark** — `WiresharkFoundation.Wireshark`.
2. **USBPcap** — `desowin.USBPcap`.
3. **Npcap** — `https://npcap.com/` (бесплатная версия не поддерживает `/S`, ставится только вручную).
4. **Перезагрузи компьютер.** Без перезагрузки USBPcap-фильтр не подключается к USB-хабам.

`tshark` не добавляется в PATH автоматически. Используй полный путь:

```powershell
& "C:\Program Files\Wireshark\tshark.exe" -D
```

## 2. Ручной способ, который работает

`USBPcapCMD` не работает из скриптов/без настоящей консоли. Запускай вручную.

1. Пуск → `cmd` → правый клик → **«Запуск от имени администратора»**.
2. Перейди в папку записей:

```bat
cd /d local\captures
```

3. Запусти:

```bat
"C:\Program Files\USBPcap\USBPcapCMD.exe"
```

4. Если спросит про перезапуск USB-устройств:

```
Do you want to restart all USB devices (y, n)?
```

ответь `y`. Мышь/клавиатура могут кратко отключиться.

5. Появится список корневых хабов:

```
Following filter control devices are available:
1 \\.\USBPcap4
2 \\.\USBPcap1
3 \\.\USBPcap2
4 \\.\USBPcap3
Select filter to monitor (q to quit):
```

Нужен хаб, под которым есть **HID-мышь + HID-клавиатура + системный контроллер + vendor-defined HID**. Для приёмника AJ199 это был пункт **1** (`\\.\USBPcap4`, root hub `7&32fcecfc`, Port 18). Для кабеля — пункт, где видна такая же раскладка. Если сомневаешься — выбери хаб с HID-мышью и несколькими vendor-defined HID.

6. Введи номер и Enter. Затем имя файла:

```
startup_read_all_dongle.pcap
```

7. Подожди 2 секунды, открой штатный `OemDrv.exe`, дождись полной загрузки.
8. Вернись в консоль и нажми **Ctrl+C**.
9. Файл должен быть не 0 байт.

Для кабеля: отключи приёмник, подключи кабель, проверь PID `0xFA7B`, повтори с именем `startup_read_all_wired.pcap`.

## 3. Wireshark GUI (если USBPcap виден)

Если в списке интерфейсов Wireshark появился `USBPcapN`:

1. Выбери интерфейс.
2. Фильтр `usb.idVendor == 0x25a7`.
3. Start → действие → Stop → Save as `.pcapng`.

Если USBPcap не появился — используй ручной способ из раздела 2.

## 4. Чек-лист для приёмника (радио)

Все файлы — только для `PID 0xFA7C`, без переключения на кабель. Для каждого пункта: запустить `USBPcapCMD` → выбрать **1** → имя файла → 2 секунды → изменить настройку → **Apply** → 2 секунды → **Ctrl+C**.

| № | Файл | Что сделать в OEM-софте |
|---|---|---|
| 1 | `debounce_8_to_4_dongle.pcap` | Debounce 8 → 4 |
| 2 | `polling_1000_to_500_dongle.pcap` | Polling 1000 → 500 |
| 3 | `lod_step_dongle.pcap` | LOD на одну ступень |
| 4 | `sleep_step_dongle.pcap` | Время сна на одну ступень |
| 5 | `motion_sync_toggle_dongle.pcap` | Motion Sync вкл/выкл |
| 6 | `angle_snap_toggle_dongle.pcap` | Angle Snap вкл/выкл |
| 7 | `ripple_toggle_dongle.pcap` | Ripple Control изменить |
| 8 | `led_breathing_red_dongle.pcap` | Breathing + красный цвет |
| 9 | `button_dpiplus_dongle.pcap` | Одну кнопку → DPI + |
| 10 | `macro_simple_abc_dongle.pcap` | Макрос `abc` на кнопку |
| 11 | `xy_independent_dongle.pcap` | XY Independent вкл/выкл |
| 12 | `profile_switch_dongle.pcap` | Переключить/сбросить профиль |
| 13 | `hotswitch_cable_dongle.pcap` | Воткнуть/вынуть кабель при открытом софте |
| 14 | `sleep_wake_dongle.pcap` | Дать мыши уснуть, затем подвигать |

Если какого-то пункта нет в интерфейсе — пропусти и напиши, какого. Не делай несколько изменений в одном файле.

## 5. Имена файлов

| Файл | Режим | Действие |
|---|---|---|
| `startup_read_all_wired.pcap` | кабель | закрыть `OemDrv`, начать запись, открыть `OemDrv`, дождаться загрузки |
| `startup_read_all_dongle.pcap` | приёмник | то же самое |
| `dpi_stage1_1600_to_800_wired.pcap` | кабель | изменить DPI ступени 1, Apply |
| `dpi_stage1_1600_to_800_dongle.pcap` | приёмник | то же самое |
| `debounce_8_to_4_*` | оба | debounce 8 → 4 |
| `led_breathing_red_*` | оба | Breathing + красный |
| `polling_rate_*` | оба | сменить частоту опроса |
| `lod_*` | оба | сменить LOD |
| `motion_sync_*` | оба | включить/выключить Motion Sync |
| `angle_snap_*` | оба | включить/выключить Angle Snap |
| `ripple_*` | оба | сменить Ripple Control |
| `sleep_time_*` | оба | сменить время сна |
| `profile_switch_*` | оба | переключить/сбросить профиль |
| `button_*` | оба | переназначить одну кнопку на один тип функции |
| `macro_simple_abc_*` | оба | макрос `abc` на кнопку |
| `sleep_wake_dongle` | приёмник | дать мыши уснуть, затем подвигать |
| `hotswitch_cable_dongle` | оба | втыкать/вытаскивать кабель при открытом софте |

## 6. Разбор записей

```powershell
python tools\capture\aj199_capture.py summary local\captures\startup_read_all_dongle.pcap
python tools\capture\aj199_capture.py writes local\captures\dpi_stage1_1600_to_800_dongle.pcap
python tools\capture\aj199_capture.py profile local\captures\startup_read_all_dongle.pcap --json local\re\dongle.json
python tools\capture\aj199_capture.py profile local\captures\startup_read_all_wired.pcap --json local\re\wired.json
python tools\capture\aj199_capture.py diff-profiles local\re\wired.json local\re\dongle.json
```

## 7. Что уже подтверждено

- `startup_read_all_wired.pcap` и `startup_read_all_dongle.pcap` — профиль читается, 106 ReadEEPROM, 0 различий.
- `dpi_stage1_1600_to_800_wired.pcap` и `_dongle.pcap` — запись `0x0C = 0F 0F 00 37` в обоих режимах.
- `polling_1000_to_500_dongle.pcap` — `0x00 = 02 53`.
- `debounce_8_to_4_dongle.pcap` — `0xA9 = 04 51`.
- `lod_step_dongle.pcap` — `0x0A = 02 53`.
- `angle_snap_toggle_dongle.pcap` — `0xAF = 01/00`.
- `ripple_toggle_dongle.pcap` — `0xB1 = 01/00`.
- `led_breathing_red_dongle.pcap` — `0x4C = 02 53 80 D5 03 52`, `0x2C = FF 00 00 56`.
- `macro_simple_abc_dongle.pcap` — слот `0xA80`, имя `abc`, кнопка `0x74`.
- `profile_switch_dongle.pcap` — команда `0x09`.
- `button_dpiplus_dongle.pcap` — `0x74 = 02 02 00 51` (DPI+), плюс уведомления `0x0A`.
- Заряд: приёмник 85% / charging=0, кабель 95% / charging=1.

## 8. Что важно не делать

- Не обновлять прошивку во время записи.
- Не писать несколько изменений в один файл.
- Не трогать другие USB-устройства во время записи.
- Не публиковать сырые записи: там могут быть данные других устройств.
