# RFS engine fork (0.3.2)

Вырезанное из `C:\RFS-0.3` ядро для доработки и отладки отдельно от игры:
**рендер + сеть + сим**. Wire-протокол **сломан намеренно** (`PROTOCOL_VERSION = 2`,
`rfs-core 0.4.0`) — помечен для полной перереализации, совместимости с 0.3.x нет.

## Состав

| Крейт | Что внутри | Донор |
|---|---|---|
| `rfs-core` | типы, `NetMessage`, протокол, spatial/collider (копия как есть + bump v2) | `RFS-0.3/rfs-core` целиком |
| `rfs-engine-render` | RHI-трейты (без DX/Vulkan-заглушек), камера, фрустум, свет | `src/rhi/*.rs` (трейты), `src/graphics/camera.rs`, `frustum.rs`, `lighting.rs` |
| `rfs-engine-net` | снапшот-мерж, интерполятор, очередь событий, лимиты чата | `src/net/handler.rs`, `snapshot.rs`, `game/interpolation.rs`, `server/mod.rs:1762` |
| `rfs-sim` | сессии/ reconnect-cleanup, carry/гидранты, атомарная запись | `rfs-server/.../connection.rs`, `server/mod.rs`, `fire_sim.rs`, `screens/server_browser.rs` |

## Что намеренно НЕ тащили

- `rhi/backends/dx11|dx12` (заглушки с `panic!`), недособранный Vulkan-бэкенд.
- `game_renderer` (тянет `game/`-типы — разрыв будет отдельной работой), `menu_renderer`, шутерное наследие.
- OBJ DSL-парсер (`parse_obj_to_mesh_data`, 0 вызывающих), мёртвый `textures.rs`.
- Ассеты (273 МБ, дубликаты) — подкладываются позже вручную.
- `job_system` (0 использований + дедлок в `Drop`) — переписать с нуля.

## Исправления относительно RFS-0.3 (реестр BUGS.md §9, №219–224)

- **№219 DELTA-WIPE**: `apply_delta` — только merge (`upsert_*` + `retain` по id); пустая дельта ничего не стирает. Гидранты — `Option` (`None` = без изменений), как и было задумано.
- **№220 INTERP-STARVE**: `InterpolationBuffer` кормится и снапшотами, и дельтами (`push_snapshot` + `push_delta`).
- **№221 RECONNECT-LEAK**: `SessionStore::remove_session` — единый путь для disconnect и reconnect: возвращает `BroadcastEvents` (drops гражданских + отключения гидрантов), вызывающий обязан их разослать.
- **№222 NET-EVENT-DROP**: `EventQueue::drain` возвращает события вызывающему (`Vec<T>`); дропа в лог внутри нет — API заставляет обработать.
- **№223 CHAT-LIMIT**: `sanitize_chat_msg(name, msg, &ChatLimits)` — лимиты только из конфига, хардкода нет.
- **№224 FAV-ATOMIC**: `persist::atomic_write` — tmp+rename для любых JSON-файлов.

## Проверка

```powershell
cargo build
cargo test
cargo run --example engine_demo
```
