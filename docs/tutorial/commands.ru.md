# Краткая справка по командам markoff

Эта страница содержит компактный список команд и параметров готового
консольного приложения `markoff_cli`.

В примерах для Windows используется:

```powershell
.\markoff_cli.exe
```

В Linux используется:

```bash
./markoff_cli
```

Если каталог с приложением добавлен в `PATH`, имя исполняемого файла можно
сократить до `markoff_cli`.

Подробное руководство с пошаговыми сценариями находится в
[README.ru.md](README.ru.md).

## Команды верхнего уровня

| Команда | Назначение |
| --- | --- |
| `--help` | Показать общую справку |
| `--version` | Показать версию и сведения о сборке |
| `convert` | Преобразовать один файл |
| `batch` | Преобразовать группу файлов по шаблону |
| `gui` | Запустить графическое приложение |

### Справка и версия

```powershell
.\markoff_cli.exe --help
.\markoff_cli.exe --version
```

Справка по конкретной команде:

```powershell
.\markoff_cli.exe convert --help
.\markoff_cli.exe batch --help
.\markoff_cli.exe gui --help
```

При запуске без подкоманды CLI выводит подсказку. Для преобразования нужно
явно использовать `convert` или `batch`.

## `convert` — преобразование одного файла

### Синтаксис

```text
markoff_cli convert INPUT [--from FORMAT] [--to FORMAT]
                         [-o OUTPUT] [--overwrite]
                         [--delimiter CHAR]
```

### Аргументы и параметры

| Аргумент или параметр | Обязателен | Описание |
| --- | --- | --- |
| `INPUT` | Да | Путь к входному файлу. `-` означает stdin для текстового формата |
| `-o, --output OUTPUT` | Нет | Путь к результату. `-` означает stdout |
| `--from FORMAT` | Нет* | Явно задать формат источника |
| `--to FORMAT` | Нет* | Задать формат результата |
| `--overwrite` | Нет | Разрешить перезапись существующего результата |
| `--delimiter CHAR` | Нет | Разделитель CSV: один ASCII-символ или `tab` |
| `-h, --help` | Нет | Показать справку по `convert` |

`--from` обязателен, если `INPUT` равен `-`. `--to` можно не указывать,
если формат определяется по расширению `OUTPUT`.

### Правила определения форматов

- формат входного файла определяется по его расширению;
- `--from` переопределяет формат входного файла;
- формат результата берётся из `--to`;
- если `--to` не указан, формат результата определяется по расширению
  `OUTPUT`;
- если `OUTPUT` не указан, результат получает имя входного файла с новым
  расширением;
- существующий результат не перезаписывается без `--overwrite`;
- входной и выходной пути должны различаться.

### Примеры

```powershell
# CSV -> Markdown; результат: .\report.md
.\markoff_cli.exe convert .\report.csv --to md

# Markdown -> DOCX с явным путём
.\markoff_cli.exe convert .\notes.md -o .\output\notes.docx

# DOCX -> Markdown с перезаписью результата
.\markoff_cli.exe convert .\report.docx --to md --overwrite

# Файл с нестандартным расширением
.\markoff_cli.exe convert .\input.data --from json --to yaml `
    -o .\output.yaml

# CSV с разделителем ;
.\markoff_cli.exe convert .\table.csv --to xlsx --delimiter ';'

# TSV
.\markoff_cli.exe convert .\table.tsv --to md --delimiter tab
```

Linux-вариант той же команды:

```bash
./markoff_cli convert ./report.csv --to md
```

## `batch` — пакетное преобразование

### Синтаксис

```text
markoff_cli batch DIRECTORY --pattern GLOB --to FORMAT
                       -o OUTPUT_DIRECTORY
                       [--overwrite] [--delimiter CHAR]
```

### Аргументы и параметры

| Аргумент или параметр | Обязателен | Описание |
| --- | --- | --- |
| `DIRECTORY` | Да | Каталог с исходными файлами |
| `--pattern GLOB` | Да | Glob-шаблон относительно `DIRECTORY` |
| `--to FORMAT` | Да | Формат результата для всех найденных файлов |
| `-o, --output OUTPUT_DIRECTORY` | Да | Каталог для результатов |
| `--overwrite` | Нет | Разрешить перезапись существующих результатов |
| `--delimiter CHAR` | Нет | Разделитель CSV: один ASCII-символ или `tab` |
| `-h, --help` | Нет | Показать справку по `batch` |

### Примеры

```powershell
# Все DOCX из каталога documents -> converted/*.md
.\markoff_cli.exe batch .\documents --pattern '*.docx' --to md `
    -o .\converted

# Все CSV -> XLSX с разделителем ;
.\markoff_cli.exe batch .\exports --pattern '*.csv' --to xlsx `
    -o .\workbooks --delimiter ';'

# Рекурсивный поиск DOCX
.\markoff_cli.exe batch .\documents --pattern '**\*.docx' --to md `
    -o .\converted

# Перезаписать уже существующие результаты
.\markoff_cli.exe batch .\documents --pattern '*.docx' --to md `
    -o .\converted --overwrite
```

Linux-вариант:

```bash
./markoff_cli batch ./documents --pattern '*.docx' --to md -o ./converted
```

Шаблон применяется относительно `DIRECTORY`. Результаты записываются прямо в
`OUTPUT_DIRECTORY` и получают базовое имя исходного файла с новым
расширением. Структура вложенных каталогов не переносится в имена
результатов. Если два файла имеют одинаковое базовое имя, заранее разделите
обработку или используйте разные каталоги.

## `gui` — запуск графического приложения

### Синтаксис

```text
markoff_cli gui
```

Команда не имеет дополнительных параметров приложения:

```powershell
.\markoff_cli.exe gui
```

Можно запустить GUI напрямую:

```powershell
.\markoff_gui.exe
```

В Linux:

```bash
./markoff_cli gui
./markoff_gui
```

Инструкция по работе с очередью, выбору формата и предварительному просмотру
приведена в разделе [Использование GUI](README.ru.md#5-использование-gui).

## Поддерживаемые форматы

| Формат | Идентификаторы |
| --- | --- |
| Word | `docx` |
| PDF | `pdf` |
| Markdown | `md`, `markdown` |
| Excel | `xlsx`, `xlsm` |
| JSON | `json` |
| CSV | `csv` |
| YAML | `yaml`, `yml` |
| TOML | `toml` |
| PowerPoint | `pptx` |
| HTML | `html`, `htm` |

Основные реализованные направления:

- DOCX -> Markdown, CSV, XLSX, JSON, YAML, TOML;
- Markdown -> DOCX, CSV, XLSX, JSON, YAML, TOML, PPTX, HTML;
- CSV -> Markdown, XLSX, DOCX;
- XLSX/XLSM -> Markdown, CSV, JSON, YAML, TOML, DOCX;
- JSON/YAML/TOML -> Markdown, DOCX, XLSX;
- PDF -> Markdown;
- PPTX -> Markdown;
- HTML -> Markdown.

Если направление не поддерживается, приложение завершает команду с ошибкой и
не создаёт корректный результат автоматически.

## Потоковый ввод и вывод

Символ `-` обозначает стандартный поток:

| Запись | Значение |
| --- | --- |
| `INPUT` равен `-` | Читать из stdin |
| `OUTPUT` равен `-` | Писать в stdout |

Потоковый режим поддерживает только Markdown, JSON, CSV, YAML, TOML и HTML.
Для stdin обязательно укажите `--from`, а для stdout — `--to`.

PowerShell:

```powershell
Get-Content -Raw .\data.json |
    .\markoff_cli.exe convert - --from json --to yaml -o -
```

Linux:

```bash
cat ./data.json | ./markoff_cli convert - --from json --to yaml -o -
```

DOCX, XLSX/XLSM, PDF и PPTX через stdin/stdout не обрабатываются.

## Общие правила параметров

### `--overwrite`

Без этого параметра существующий файл назначения защищён от перезаписи:

```powershell
.\markoff_cli.exe convert .\source.md --to html `
    -o .\result.html --overwrite
```

### `--delimiter`

Значение по умолчанию — запятая:

```powershell
.\markoff_cli.exe convert .\source.csv --to md
```

Допустимы один ASCII-символ и специальное значение `tab`:

```powershell
.\markoff_cli.exe convert .\source.csv --to md --delimiter ';'
.\markoff_cli.exe convert .\source.tsv --to md --delimiter tab
```

Параметр применяется и к `convert`, и к `batch`.

### Формат вывода

Если результат задаётся через `-o`, его расширение должно соответствовать
поддерживаемому формату, например:

```powershell
.\markoff_cli.exe convert .\source.md -o .\result.docx
```

Если расширение не указано или неизвестно, добавьте `--to`:

```powershell
.\markoff_cli.exe convert .\source.data --from json --to md `
    -o .\result.md
```

## Коды и сообщения об ошибках

Приложение сообщает об ошибках текстом и завершает операцию без
успешного результата. Наиболее частые случаи:

| Сообщение | Причина | Действие |
| --- | --- | --- |
| `output file already exists` | Результат уже существует | Укажите новый путь или добавьте `--overwrite` |
| `stdin requires --from` | Формат потока не задан | Добавьте `--from FORMAT` |
| `specify --to or an output path with a known extension` | Не определён формат результата | Добавьте `--to` или расширение в `-o` |
| `unsupported format` | Неизвестное расширение или идентификатор | Проверьте формат или используйте `--from` |
| `conversion from ... to ... is not implemented yet` | Направление не поддерживается | Выберите другой маршрут |

## Минимальная памятка

```text
markoff_cli --help
markoff_cli --version
markoff_cli convert INPUT --to FORMAT
markoff_cli convert INPUT -o OUTPUT
markoff_cli batch DIRECTORY --pattern GLOB --to FORMAT -o OUTPUT_DIRECTORY
markoff_cli gui
```
