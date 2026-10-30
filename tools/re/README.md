# Инструменты статического анализа

Это вспомогательные скрипты, они не входят в приложение.

## Требования

- Python 3.10+
- `pefile`
- Ghidra 12.2 и JDK 25 — портативно в `local/tools/` (в репозиторий не входят)

Установка Python-зависимостей:

```powershell
python -m pip install -r tools\re\requirements.txt
```

## `pe_inventory.py`

Собирает секции, импорты, RTTI-классы, ASCII/UTF-16 строки и ссылки на строки из `.text`.

```powershell
python tools\re\pe_inventory.py
python tools\re\pe_inventory.py --filter "SendCMD|ReadROM|WriteEEPROM|DPI"
```

Результат по умолчанию: `local/re/pe_inventory.json`.

## `parse_jmm.py`

```powershell
python tools
python tools/re/parse_jmm.py fixtures/abc.jmm
```

Разбирает экспорт `.jmm`: magic, имя, события key/state/delay.

## Ghidra

```powershell
$env:JAVA_HOME = "$PWD\local\tools\jdk-25.0.5+8"
local\tools\ghidra_12.2_PUBLIC\support\analyzeHeadless `
  "$PWD\local\re\ghidra_project" AJ199 `
  -process OemDrv.exe `
  -scriptPath "$PWD\tools\re\ghidra\scripts" `
  -postScript DumpAj199Anchors.java "$PWD\local\re\ghidra_out"
```

Результат: `local/re/ghidra_out/anchors.txt`, `functions.txt`, `decompiled/*.c`.

## Важно

- Файлы штатного драйвера не изменяются.
- `local/re/` — временные выгрузки, в git не коммитятся.
- В `docs/` попадают только проверенные выводы, а не сырые логи.
