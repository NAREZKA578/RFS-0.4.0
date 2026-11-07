# Руководство по созданию надзирателя (Supervisor) для RFS

> **Надзиратель** — standalone-программа (вне Rust-воркспейса), которая запускает, останавливает и мониторит серверы RFS через их контрольный порт. Описана в `.DOC/new/server-interface-plan.md`, разделы 3, 5, 8.

Этот документ позволяет любому разработчику создать свой собственный надзиратель на любом языке программирования. Он не зависит от кода RFS — достаточно реализовать клиент контрольного порта.

---

## 1. Архитектура

```
┌─────────────────┐      ┌──────────────────┐      ┌─────────────────┐
│  Окно панели    │─────►│  Надзиратель     │─────►│ RFS сервер      │
│  (Rust UI)      │      │  (любой язык)    │      │  contrôle порт  │
└─────────────────┘      └──────────────────┘      └─────────────────┘
                                       │
                                       ▼
                                ┌──────────────┐
                                │ Профили JSON │
                                │ (на диске)   │
                                └──────────────┘
```

Надзиратель отвечает за:
- **Запуск / остановку / убивание** процессов сервера
- **Мониторинг** состояния, PID, времени работы, памяти и CPU
- **Чтение вывода** сервера (stdout/stderr) и трансляцию на панель
- **Подключение к contrôle порту** сервера и передачу команд/данных панели
- **Хранение профилей** серверов (бинарник, конфиг, аргументы, порт, токен)
- **Цветной терминальный интерфейс** (TUI) с живым дашбордом

---

## 2. Контрольный порт сервера (вариант Б)

Сервер RFS слушает контрольный порт **только на loopback** (`127.0.0.1`). Порт по умолчанию `7778`, настраивается в `server_config.json` в секции `control`.

### Протокол

- **Транспорт:** TCP, один подключение = одна сессия
- **Формат:** newline-delimited JSON, одно сообщение на строку
- **Первое сообщение:** токен hello
- **Ответы:** `{"ok":true,...}` или `{"ok":false,"error":{"code","text"}}`

### Пример сессии

```
Клиент:  {"hello":{"token":"faa37fd0453d0eee8814d767fc50615de0fd00b98671ff6ee17ccbdeb10924ab","proto":1}}
Сервер:  {"hello":"ok","control_proto":1}

Клиент:  {"id":1,"cmd":"status"}
Сервер:  {"id":1,"ok":true,"data":{"state":"running","tick":449,"players":0,"ships":15,...}}

Клиент:  {"id":2,"cmd":"say","args":{"text":"Перезагрузка через минуту"}}
Сервер:  {"id":2,"ok":true,"data":{"queued":true,"bytes":36}}

Клиент:  {"id":3,"cmd":"kick","args":{"player":17,"reason":"спам"}}
Сервер:  {"id":3,"ok":true,"data":{"queued":true}}

Клиент:  {"id":4,"cmd":"stop","args":{"delay_s":0}}
Сервер:  {"id":4,"ok":true,"data":{"queued":true,"delay_s":0}}
```

### Коды ошибок

| Код | Значение |
|-----|----------|
| `unauthorized` | Неверный токен, пропущен токен, пауза аутентификации |
| `rate_limited` | Слишком частые команды (лимит на соединение) |
| `unknown_command` | Команда не из принятого набора |
| `bad_arguments` | Неверные аргументы команды |
| `unknown_player` | Игрок с таким connection id не найден |
| `not_allowed_in_state` | Сервер не в состоянии `running` |
| `shutting_down` | Сервер уже останавливается |
| `internal` | Очередь команд не обслуживается (сервер не тикает) |

### Лимиты (по умолчанию)

| Лимит | Значение | Где настраивается |
|-------|----------|-------------------|
| Адрес прослушивания | `127.0.0.1` только | Жёстко в коде сервера |
| Порт | `7778` | `control.port` в `server_config.json` |
| Макс. соединений | `4` | `control.max_connections` |
| Макс. длина строки | `64 KB` | `control.max_line_bytes` |
| Команд в секунду | `20` | `control.commands_per_second` (на соединение) |
| Неверных токенов до паузы | `5` | `control.max_bad_tokens` (door-wide) |
| Пауза аутентификации | `60 с` | `control.bad_token_cooldown_s` |

### Токен управления

- Генерируется сервером при **первом запуске**
- Сохраняется в файл `control_token.json` рядом с `server_config.json`
- **Никогда не логируется** и **никогда не эхаится** в ответах
- Отдельный от `auth_token` игрока (№700)
- Чтение токена: `ctl --token-file control_token.json` или напрямую из файла

Пример файла токена:
```json
{
  "token": "faa37fd0453d0eee8814d767fc50615de0fd00b98671ff6ee17ccbdeb10924ab",
  "created_unix_s": 1794046711
}
```

### Команда `status` — пример ответа

```json
{
  "id": 1,
  "ok": true,
  "data": {
    "state": "running",
    "match_state": "in_progress",
    "match_id": "0fa99553-b93c-4ba8-909a-62e577debd0a",
    "addr": "0.0.0.0:7777",
    "port": 7777,
    "tick": 449,
    "tick_rate": 30,
    "tick_budget_ms": 33.0,
    "uptime_s": 10,
    "players": 0,
    "ships": 15,
    "connections": 0,
    "entities": {"ships":15,"players":0,"stations":225,"compartments":105,"projectiles":0}
  }
}
```

---

## 3. Список команд contrôle порта

| Команда | Аргументы | Описание | Этап |
|---------|-----------|----------|------|
| `status` | — | Состояние сервера, матч, тик, игроки, корабли | 1 |
| `perf` | — | Время тика, фазы, бюджет 33 мс, пропуски | 1 |
| `net` | — | Трафик, пакеты, раунды, потери | 1 |
| `players` | — | Список игроков с connection_id, именем, адресом, пингом | 1 |
| `config` | — | Эффективные настройки, секреты скрыты | 1 |
| `log` | `tail`, `level` | Последние N строк журнала с фильтром по уровню | 1 |
| `watch` | `topics` | Подписка на поток событий (`log`, `conn`, `state`) | 1 |
| `audit` | `tail` | Хвост журнала действий управления | 2 |
| `say` | `text` | Объявление всем игрокам (max 256 байт) | 2 |
| `kick` | `player`, `reason` | Выгнать игрока с причиной (max 256 байт) | 2 |
| `stop` | `delay_s` | Остановить сервер (0 = сразу, иначе 60/30/10 с предупреждения) | 2 |
| `reload` | — | Перечитать конфиг, применить изменяемое на лету | 2 |
| `ban` | `target`, `reason` | Заблокировать цель (Steam ID или адрес) | 3 |
| `unban` | `target` | Убрать цель из бан-листа | 3 |
| `banlist` | `tail` | Показать бан-лист | 3 |
| `end_match` | — | Завершить матч (состояние `ending`) | 4 |
| `start_match` | — | Начать матч (состояние `in_progress`) | 4 |
| `restart_match` | — | Сбросить матч в `waiting` | 4 |

---

## 4. Пошаговая реализация надзирателя

### Шаг 1: Выберите язык и скомпилируйте сервер

Надзиратель не зависит от кода RFS, но ему нужен путь к бинарнику сервера.

```bash
# В server_src:
cargo build --release --bin server
# Готовый бинарник: server_src/target/release/server.exe
```

### Шаг 2: Создайте профиль сервера

Профиль — JSON-файл, который надзиратель использует для запуска:

```json
{
  "name": "main",
  "bin_path": "C:\\RFS-0.4.0\\server_src\\target\\release\\server.exe",
  "config": "server_config.json",
  "args": ["--ships", "4"],
  "work_dir": "C:\\RFS-0.4.0\\server_src",
  "control_port": 7778,
  "token_file": "control_token.json",
  "created_at": "2026-11-07T10:00:00Z"
}
```

Надзиратель хранит профили в папке `profiles/` (настраивается через `--profiles`).

### Шаг 3: Реализуйте клиент contrôle порта

На irm языке:

1. Подключитесь по TCP к `127.0.0.1:<control_port>`
2. Прочитайте токен из файла `control_token.json`
3. Отправьте hello: `{"hello":{"token":"<токен>","proto":1}}\n`
4. Прочитайте ответ: должен быть `{"hello":"ok","control_proto":1}`
5. Отправляйте команды: `{"id":<N>,"cmd":"<команда>","args":{...}}\n`
6. Читайте ответы по одной строке

### Шаг 4: Реализуйте управление процессами

- **Запуск:** выполните бинарник сервера с аргументами профиля,apture stdout/stderr
- **Остановка:** отправьте `{"cmd":"stop","args":{"delay_s":0}}` на contrôle порт, затем дождитесь выхода процесса (таймаут 30 с)
- **Убийство:** если контрольный порт не отвечает, отправьте SIGKILL / TerminateProcess
- **Мониторинг:** проверяйте, жив ли процесс; если нет — отметьте профиль как `stopped`

### Шаг 5: Реализуйте мониторинг

- **PID:** из запущенного процесса
- **Состояние:** `starting` / `running` / `stopping` / `stopped` / `error`
- **Время работы:** `now - start_time`
- **Память и CPU:** через системные API (на Windows — `GetProcessMemoryInfo`, на Linux — `/proc/<pid>/stat`)
- **Адрес и порт:** из ответа `status` на contrôle порту

### Шаг 6: Реализуйте CLI

Поддержите как минимум:

```
supervisor --server <путь> start [имя]
supervisor --server <путь> stop [имя]
supervisor --server <путь> kill [имя]
supervisor --server <путь> list
supervisor --server <путь> status [имя]
supervisor --server <путь> watch [имя]
supervisor --server <путь> exec [имя] <команда> [аргументы...]
```

---

## 5. Примеры реализации клиента

### Go

```go
package main

import (
    "bufio"
    "encoding/json"
    "fmt"
    "net"
    "os"
    "strings"
    "time"
)

type ControlClient struct {
    conn    net.Conn
    reader  *bufio.Reader
    token   string
}

func Dial(addr, token string) (*ControlClient, error) {
    conn, err := net.DialTimeout("tcp", addr, 2*time.Second)
    if err != nil {
        return nil, err
    }
    c := &ControlClient{conn: conn, reader: bufio.NewReader(conn), token: token}
    if err := c.hello(); err != nil {
        conn.Close()
        return nil, err
    }
    return c, nil
}

func (c *ControlClient) hello() error {
    req := fmt.Sprintf(`{"hello":{"token":"%s","proto":1}}` + "\n", c.token)
    if _, err := c.conn.Write([]byte(req)); err != nil {
        return err
    }
    line, err := c.reader.ReadString('\n')
    if err != nil {
        return err
    }
    if !strings.Contains(line, `"hello":"ok"`) {
        return fmt.Errorf("hello refused: %s", strings.TrimSpace(line))
    }
    return nil
}

func (c *ControlClient) Command(id uint64, cmd string, args map[string]interface{}) (map[string]interface{}, error) {
    envelope := map[string]interface{}{"id": id, "cmd": cmd}
    if args != nil {
        envelope["args"] = args
    }
    data, _ := json.Marshal(envelope)
    data = append(data, '\n')
    if _, err := c.conn.Write(data); err != nil {
        return nil, err
    }
    line, err := c.reader.ReadString('\n')
    if err != nil {
        return nil, err
    }
    return parseResponse(line)
}

func (c *ControlClient) Close() error {
    return c.conn.Close()
}

func parseResponse(line string) (map[string]interface{}, error) {
    line = strings.TrimSpace(line)
    var resp struct {
        OK  bool                   `json:"ok"`
        Data map[string]interface{} `json:"data"`
        Error struct {
            Code string `json:"code"`
            Text string `json:"text"`
        } `json:"error"`
    }
    if err := json.Unmarshal([]byte(line), &resp); err != nil {
        return nil, err
    }
    if !resp.OK {
        return nil, fmt.Errorf("%s: %s", resp.Error.Code, resp.Error.Text)
    }
    return resp.Data, nil
}

// Пример использования:
// client, _ := Dial("127.0.0.1:7778", token)
// data, _ := client.Command(1, "status", nil)
// fmt.Println(data["state"]) // "running"
// client.Close()
```

### Python

```python
import json
import socket
import time

class ControlClient:
    def __init__(self, addr, token):
        self.sock = socket.create_connection(addr, timeout=2)
        self.reader = self.sock.makefile('r')
        self.token = token
        self._hello()

    def _hello(self):
        req = json.dumps({"hello": {"token": self.token, "proto": 1}}) + "\n"
        self.sock.sendall(req.encode())
        line = self.reader.readline()
        resp = json.loads(line)
        if resp.get("hello") != "ok":
            raise RuntimeError(f"hello refused: {resp}")

    def command(self, cmd_id, cmd, args=None):
        envelope = {"id": cmd_id, "cmd": cmd}
        if args:
            envelope["args"] = args
        data = json.dumps(envelope) + "\n"
        self.sock.sendall(data.encode())
        line = self.reader.readline()
        resp = json.loads(line)
        if not resp.get("ok"):
            err = resp.get("error", {})
            raise RuntimeError(f"{err.get('code')}: {err.get('text')}")
        return resp.get("data", {})

    def close(self):
        self.sock.close()

# Пример:
# client = ControlClient(("127.0.0.1", 7778), token)
# data = client.command(1, "status")
# print(data["state"])
# client.close()
```

### cURL (настоящий TCP, не HTTP)

```bash
# Отправить hello и статус через netcat (если доступен)
printf '{"hello":{"token":"%s","proto":1}}\n' "$TOKEN" | \
    nc 127.0.0.1 7778 | head -1

# Или одна команда:
printf '{"hello":{"token":"%s","proto":1}}\n{"id":1,"cmd":"status"}\n' "$TOKEN" | \
    nc 127.0.0.1 7778
```

---

## 6. Формат профиля (JSON)

```json
{
  "name": "main-server",
  "bin_path": "/opt/rfs/server",
  "config": "server_config.json",
  "args": ["--ships", "15"],
  "work_dir": "/opt/rfs",
  "control_port": 7778,
  "token_file": "control_token.json",
  "created_at": "2026-11-07T10:00:00Z"
}
```

| Поле | Обязательно | Описание |
|------|-------------|----------|
| `name` | Да | Уникальное имя профиля |
| `bin_path` | Да | Путь к бинарнику сервера |
| `config` | Нет | Путь к `server_config.json` |
| `args` | Нет | Дополнительные аргументы командной строки |
| `work_dir` | Нет | Рабочая директория сервера |
| `control_port` | Нет | Порт contrôle (7778 по умолчанию) |
| `token_file` | Нет | Файл токена управления |
| `created_at` | Нет | Время создания профиля |

---

## 7. Безопасность

| Правило | Реализация |
|---------|------------|
| Только loopback | Сервер биндится на `127.0.0.1`, надзиратель подключается только туда |
| Отдельный токен | Токен управления ≠ `auth_token` игрока |
| Токен в файле | `control_token.json` рядом с конфигом, генерируется при первом запуске |
| Токен не в логах | Сервер логирует только путь к файлу, никогда не сам токен |
| Токен не в ответах | Ответы contrôle порта не содержат токен |
| Пауза после ошибок | 5 неверных токена → пауза 60 с, door-wide (не за соединение) |
| Лимит соединений | Максимум 4 одновременных соединения к contrôle порту |
| Лимит команды | 20 команд/с на соединение (token bucket) |
| Макс. строка | 64 КБ, больше — отказ и закрытие соединения |

---

## 8. Примеры использования

### Запуск сервера через надзирателя

```bash
./rfs-orchestrator --server ./target/release/server.exe --profiles ./profiles start main
```

### Посмотреть статус

```bash
./rfs-orchestrator status main
# profile: main
# state:   running
# pid:     1234
# addr:    0.0.0.0:7777
# uptime:  1h2m3s
# mem:     256.5 MB
# cpu:     12.3%
```

### Список профилей

```bash
./rfs-orchestrator list
# NAME          STATE   PID   ADDR             UPTIME   MEM     CPU
# main          running 1234  0.0.0.0:7777     1h2m     256.5   12.3%
# beta          stopped 0     -                -        0       0%
```

### Отправить команду серверу

```bash
./rfs-orchestrator exec main status
./rfs-orchestrator exec main say "Сервер перезагружается через 5 минут"
./rfs-orchestrator exec main kick 17 "спам в чате"
./rfs-orchestrator exec main stop 30
```

### Посмотреть журнал действий

```bash
./rfs-orchestrator exec main audit
```

### Посмотреть лог сервера в реальном времени

```bash
./rfs-orchestrator watch main
```

### Остановить сервер

```bash
./rfs-orchestrator stop main
```

---

## 9. Стандартные порты и пути

| Путь | Значение по умолчанию |
|------|----------------------|
| Контрольный порт | `7778` |
| Игровой порт | `7777` |
| Файл конфига | `server_config.json` |
| Файл токена | `control_token.json` |
| Файл журнала действий | `control_actions.jsonl` |
| Директория профилей | `profiles/` |

---

## 10. Тестирование надзирателя

Перед выпуском своего надзирателя убедитесь, что он проходит следующие проверки:

1. **Подключение к contrôle порту** — hello с правильным токеном возвращает `{"hello":"ok"}`
2. **Неверный токен** — возвращает `{"ok":false,"error":{"code":"unauthorized"}}`
3. **Команда status** — возвращает `state`, `tick`, `players`, `ships`
4. **Команда say** — возвращает `{"ok":true,"data":{"queued":true}}`
5. **Команда stop** — сервер завершает работу с кодом выхода 0
6. **Бан** — `ctl ban <addr> "причина"` добавляет запись, реконнект с этого адреса отказывается
7. **Неизвестная команда** — возвращает `unknown_command` со списком принятых команд
8. **Лимит строки** — строка > 64 КБ возвращает `bad_arguments` и не роняет сервер

---

## 11. Связь с панелью (этап 6)

Окно панели (Rust, `crates/panel`) разговаривает **только с надзирателем**, а надзиратель — с серверами. Это позволяет:
- Окну не знать о бинарниках серверов
- Надзирателю мерять память и CPU процесса (что окно не может)
- Управлять несколькими серверами из одного окна

Панель подключается к contrôle порту надзирателя (а не напрямую к серверам) и пересылает команды на подходящий сервер по имени профиля.

---

## 11. Быстрый старт (копипаста)

```bash
# 1. Собрать сервер
cd server_src
cargo build --release --bin server

# 2. Собрать надзиратель
cd ../ops/orchestrator
go build -o rfs-orchestrator .

# 3. Запустить сервер через надзиратель
./rfs-orchestrator --server ../server_src/target/release/server.exe start main

# 4. Проверить статус
./rfs-orchestrator status main

# 5. Отправить объявление
./rfs-orchestrator exec main say "Привет от надзирателя!"

# 6. Остановить
./rfs-orchestrator stop main
```

---

## 12. HTTP API для панели (Rust UI)

Окно панели (Rust, `crates/panel`) подключается к HTTP API надзирателя вместо прямого подключения к contrôle портам серверов. Это централизует управление и позволяет панели не знать о бинарниках серверов.

### Запуск HTTP API

```bash
./rfs-orchestrator --server ./server.exe --http --http-addr 127.0.0.1:8080 start main
```

### Эндпоинты

| Метод | Путь | Описание |
|-------|------|----------|
| `GET` | `/` | Информация о сервисе |
| `GET` | `/api/health` | Проверка здоровья надзирателя |
| `GET` | `/api/profiles` | Список всех профилей с состоянием |
| `GET` | `/api/profiles/{name}` | Конфигурация профиля |
| `GET` | `/api/profiles/{name}/status` | Детальный статус профиля |
| `GET` | `/api/profiles/{name}/metrics` | Метрики (память, CPU, рестарты) |
| `POST` | `/api/profiles/{name}/start` | Запуск профиля |
| `POST` | `/api/profiles/{name}/stop` | Остановка профиля |
| `POST` | `/api/profiles/{name}/kill` | Немедленное убийство |
| `POST` | `/api/profiles/{name}/restart` | Перезапуск |
| `POST` | `/api/profiles/{name}/exec` | Выполнить contrôle команду |
| `POST` | `/api/profiles/start-all` | Запуск всех профилей |
| `POST` | `/api/profiles/stop-all` | Остановка всех профилей |
| `POST` | `/api/profiles/kill-all` | Убийство всех профилей |
| `GET` | `/api/ws` | WebSocketライブ обновления (заглушка, используйте polling) |

### Пример: панель получает статус всех профилей

```bash
curl http://127.0.0.1:8080/api/profiles
# [{"name":"main","state":"running","port":7778,"tags":["prod"],"running":true}]
```

### Пример: панель отправляет команду say

```bash
curl -X POST http://127.0.0.1:8080/api/profiles/main/exec \
  -H "Content-Type: application/json" \
  -d '{"command":"say","args":["сервер перезагружается"]}'
```

### Пример: панель получает метрики

```bash
curl http://127.0.0.1:8080/api/profiles/main/metrics
# {"name":"main","state":"running","pid":1234,"uptime":"1h2m","mem_mb":256.5,"cpu_percent":12.3}
```

### WebSocket (в разработке)

Эндпоинт `/api/ws` пока возвращает заглушку. Панель может использовать polling через `/api/profiles` и `/api/profiles/{name}/status` для получения обновлений в реальном времени.

---

## 13. Терминальный UI надзирателя

Надзиратель имеет встроенный цветной терминальный интерфейс для оператора:

```
rfs-orchestrator list
NAME    STATE   PID   ADDR             UPTIME   MEM     CPU     TAGS
main    running 1234  127.0.0.1:7777   1h2m     256.5   12.3%   prod
beta    stopped 0     -                -        0       0%      dev
```

Цвета состояния:
- 🟢 `running` — зелёный
- 🟡 `starting`/`stopping` — жёлтый
- 🔴 `error`/`unhealthy` — красный
- ⚪ `stopped` — серый

Команды UI:
- `list` — таблица всех профилей
- `status [name]` — детальный статус
- `status-all` — статус всех запущенных
- `metrics [name]` — память, CPU, рестарты, health
- `health [name]` — проверка через contrôle порт
- `watch [name]` — поток логов сервера в реальном времени

---

*Документация соответствует `.DOC/new/server-interface-plan.md` (Приложение А, этапы 1–4, раздел 8). Актуальна на 07 ноября 2026.*
