# Краткая справка по командам markoff

Эта страница содержит компактный список команд и параметров готового
консольного приложения `markoff`.

В примерах для Windows используется:

```powershell
.\markoff.exe
```

В Linux используется:

```bash
./markoff
```

Если каталог с приложением добавлен в `PATH`, имя исполняемого файла можно
сократить до `markoff`.

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
.\markoff.exe --help
.\markoff.exe --version
```

Справка по конкретной команде:

```powershell
.\markoff.exe convert --help
.\markoff.exe batch --help
.\markoff.exe gui --help
```

При запуске без подкоманды CLI выводит подсказку. Для преобразования нужно
явно использовать `convert` или `batch`.

## `convert` — преобразование одного файла

### Синтаксис

```text
markoff convert INPUT [--from FORMAT] [--to FORMAT]
                         [-o OUTPUT] [--overwrite]
                         [--tables-only] [--delimiter CHAR]
```

### Аргументы и параметры

| Аргумент или параметр | Обязателен | Описание |
| --- | --- | --- |
| `INPUT` | Да | Путь к входному файлу. `-` означает stdin для текстового формата |
| `-o, --output OUTPUT` | Нет | Путь к результату. `-` означает stdout |
| `--from FORMAT` | Нет* | Явно задать формат источника |
| `--to FORMAT` | Нет* | Задать формат результата |
| `--overwrite` | Нет | Разрешить перезапись существующего результата |
| `--tables-only` | Нет | Оставить только таблицы при обмене документа с JSON/YAML/TOML |
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
.\markoff.exe convert .\report.csv --to md

# Markdown -> DOCX с явным путём
.\markoff.exe convert .\notes.md -o .\output\notes.docx

# DOCX -> Markdown с перезаписью результата
.\markoff.exe convert .\report.docx --to md --overwrite

# Markdown или DOCX -> PDF
.\markoff.exe convert .\notes.md -o .\notes.pdf
.\markoff.exe convert .\report.docx --to pdf

# JSON/YAML/TOML -> PDF
.\markoff.exe convert .\report.json --to pdf

# Файл с нестандартным расширением
.\markoff.exe convert .\input.data --from json --to yaml `
    -o .\output.yaml

# CSV с разделителем ;
.\markoff.exe convert .\table.csv --to xlsx --delimiter ';'

# TSV
.\markoff.exe convert .\table.tsv --to md --delimiter tab

# Извлечь из DOCX только таблицы в JSON
.\markoff.exe convert .\report.docx --to json --tables-only

# Восстановить DOCX только из табличных блоков JSON
.\markoff.exe convert .\report.json --to docx --tables-only
```

Linux-вариант той же команды:

```bash
./markoff convert ./report.csv --to md
```

## `batch` — пакетное преобразование

### Синтаксис

```text
markoff batch DIRECTORY --pattern GLOB --to FORMAT
                       -o OUTPUT_DIRECTORY
                       [--overwrite] [--tables-only] [--delimiter CHAR]
```

### Аргументы и параметры

| Аргумент или параметр | Обязателен | Описание |
| --- | --- | --- |
| `DIRECTORY` | Да | Каталог с исходными файлами |
| `--pattern GLOB` | Да | Glob-шаблон относительно `DIRECTORY` |
| `--to FORMAT` | Да | Формат результата для всех найденных файлов |
| `-o, --output OUTPUT_DIRECTORY` | Да | Каталог для результатов |
| `--overwrite` | Нет | Разрешить перезапись существующих результатов |
| `--tables-only` | Нет | Оставить только таблицы при обмене документа с JSON/YAML/TOML |
| `--delimiter CHAR` | Нет | Разделитель CSV: один ASCII-символ или `tab` |
| `-h, --help` | Нет | Показать справку по `batch` |

### Примеры

```powershell
# Все DOCX из каталога documents -> converted/*.md
.\markoff.exe batch .\documents --pattern '*.docx' --to md `
    -o .\converted

# Все CSV -> XLSX с разделителем ;
.\markoff.exe batch .\exports --pattern '*.csv' --to xlsx `
    -o .\workbooks --delimiter ';'

# Рекурсивный поиск DOCX
.\markoff.exe batch .\documents --pattern '**\*.docx' --to md `
    -o .\converted

# Перезаписать уже существующие результаты
.\markoff.exe batch .\documents --pattern '*.docx' --to md `
    -o .\converted --overwrite
```

Linux-вариант:

```bash
./markoff batch ./documents --pattern '*.docx' --to md -o ./converted
```

Шаблон применяется относительно `DIRECTORY`. Результаты записываются прямо в
`OUTPUT_DIRECTORY` и получают базовое имя исходного файла с новым
расширением. Структура вложенных каталогов не переносится в имена
результатов. Если два файла имеют одинаковое базовое имя, заранее разделите
обработку или используйте разные каталоги.

## `gui` — запуск графического приложения

### Синтаксис

```text
markoff gui
```

Команда не имеет дополнительных параметров приложения:

```powershell
.\markoff.exe gui
```

Можно запустить GUI напрямую:

```powershell
.\markoff_gui.exe
```

В Linux:

```bash
./markoff gui
./markoff_gui
```

Инструкция по работе с очередью, выбору формата и предварительному просмотру
приведена в разделе [Использование GUI](README.ru.md#5-использование-gui).

## Поддерживаемые форматы

| Формат | Идентификаторы |
| --- | --- |
| Word | `docx` |
| OpenDocument Text | `odt` |
| PDF | `pdf` |
| Markdown | `md`, `markdown` |
| Excel | `xlsx`, `xlsm` |
| OpenDocument Spreadsheet | `ods` |
| JSON | `json` |
| CSV | `csv` |
| YAML | `yaml`, `yml` |
| TOML | `toml` |
| PowerPoint | `pptx` |
| OpenDocument Presentation | `odp` |
| HTML | `html`, `htm` |

### Необязательная тема оформления

Для выходных PDF, HTML, DOCX и ODT можно передать TOML-файл:

```powershell
markoff convert report.md --to pdf --style docs\examples\style-theme.toml
markoff batch documents --pattern "*.md" --to docx -o converted --style corporate.toml
```

Если `--style` не указан, используется стандартное оформление Markoff.
Неизвестные свойства, неверные цвета и недопустимые размеры возвращают явную
ошибку.

Шаблон со всеми параметрами и значениями по умолчанию создаётся командой
`style-template`:

```powershell
markoff style-template -o corporate.toml
markoff style-template -o corporate.toml --overwrite
```

| Параметр | Назначение |
| --- | --- |
| `-o, --output <FILE>` | Путь к создаваемому TOML-файлу; без него или с `-` шаблон выводится в stdout |
| `--overwrite` | Разрешить замену существующего файла; без флага возвращается ошибка |

Каждый параметр в шаблоне снабжён комментарием. Ненужные строки можно удалить:
пропущенные свойства получают те же значения по умолчанию.

Основные реализованные направления:

- DOCX -> Markdown, CSV, XLSX, JSON, YAML, TOML, PDF;
- ODT -> Markdown, HTML, CSV, XLSX, ODS, JSON, YAML, TOML, PDF;
- Markdown -> DOCX, ODT, PDF, CSV, XLSX, ODS, JSON, YAML, TOML, PPTX, ODP, HTML;
- CSV -> Markdown, XLSX, ODS, DOCX, ODT;
- XLSX/XLSM и ODS -> Markdown и поддерживаемые табличные форматы;
- JSON/YAML/TOML -> Markdown, DOCX, ODT, PDF, PPTX, ODP, HTML, XLSX, ODS и другие JSON/YAML/TOML;
- PDF -> Markdown, JSON, YAML, TOML;
- PPTX -> Markdown, JSON, YAML, TOML;
- ODP -> Markdown, JSON, YAML, TOML;
- HTML -> Markdown, JSON, YAML, TOML.

При обмене документами и JSON/YAML/TOML в схеме `blocks` по умолчанию
сохраняются все поддерживаемые типы блоков; `--tables-only` оставляет только
отдельные блоки таблиц. Табличный обмен JSON/YAML/TOML с XLSX не меняется.

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
    .\markoff.exe convert - --from json --to yaml -o -
```

Linux:

```bash
cat ./data.json | ./markoff convert - --from json --to yaml -o -
```

DOCX, ODT, XLSX/XLSM, ODS, PDF, PPTX и ODP через stdin/stdout не обрабатываются.

## Общие правила параметров

### `--overwrite`

Без этого параметра существующий файл назначения защищён от перезаписи:

```powershell
.\markoff.exe convert .\source.md --to html `
    -o .\result.html --overwrite
```

### `--delimiter`

Значение по умолчанию — запятая:

```powershell
.\markoff.exe convert .\source.csv --to md
```

Допустимы один ASCII-символ и специальное значение `tab`:

```powershell
.\markoff.exe convert .\source.csv --to md --delimiter ';'
.\markoff.exe convert .\source.tsv --to md --delimiter tab
```

Параметр применяется и к `convert`, и к `batch`.

### Формат вывода

Если результат задаётся через `-o`, его расширение должно соответствовать
поддерживаемому формату, например:

```powershell
.\markoff.exe convert .\source.md -o .\result.docx
```

Если расширение не указано или неизвестно, добавьте `--to`:

```powershell
.\markoff.exe convert .\source.data --from json --to md `
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
markoff --help
markoff --version
markoff convert INPUT --to FORMAT
markoff convert INPUT -o OUTPUT
markoff batch DIRECTORY --pattern GLOB --to FORMAT -o OUTPUT_DIRECTORY
markoff gui
```
