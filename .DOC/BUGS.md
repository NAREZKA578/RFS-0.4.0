Пример записи бага в реестор:

Баг№(Номер бага, по порядку начиная с 1)
Баг в файле: (Понлый путь до файла и самим файлом, пример: C:\RFS-0.4.0\.DOC\BUGS.md)
В чём заключается баг: (подробное описани что за баг и что он портит, и т.д.)
Тип бага: (Синтаксический, стилистический, архетектурный и так далее...)
Статус: (Исправлен\не исправлен)
Если исправлен в должно быть написано: "Исправлен как: (и тут полная роспись как исправлялся данный конкретный баг)".

---

Баг№1
Баг в файле: C:\RFS-0.4.0\server_src\crates\core\src\packet.rs (строка 9, константа HEADER_SIZE)
В чём заключается баг: HEADER_SIZE был равен 16, а реальный bincode-размер PacketHeader — 24 байта (4+2+1+1+4+4+4+2+2). Из-за этого deserialize_packet резал заголовок коротко, клиент не мог подключиться вообще: сервер отвечал "io error: unexpected end of file" на первую же датаграмму.
Тип бага: Логический (сериализация)
Статус: Исправлен
Исправлен как: установлено HEADER_SIZE = 24; добавлены регрессионные тесты header_size_matches_wire_format и packet_roundtrip в том же файле (оба зелёные).

Баг№2
Баг в файле: C:\RFS-0.4.0\server_src\crates\core\src\packet.rs (функция serialize_packet)
В чём заключается баг: serialize_packet не проставлял header.payload_size сам — поле оставалось нулевым из конструкта вызывающей стороны. Латентно ломал ветку ConnectReject и любой пакет, где вызыватель забывал выставить размер.
Тип бага: Логический (сериализация)
Статус: Исправлен
Исправлен как: serialize_packet теперь сам выставляет header.payload_size = payload.len() перед сериализацией заголовка.

Баг№3
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\snapshot.rs (SnapshotBuffer::get_latest)
В чём заключается баг: get_latest читал слот current_index, который указывает на base_tick, а не на newest-запись (write_snapshot его не двигал на обычном пути). В итоге send loop сервера всегда получал None и слал клиентам пустые снапшоты через fallback — клиент видел entities=0 каждый тик при полностью заполненном интересе.
Тип бага: Логический
Статус: Исправлен
Исправлен как: get_latest переписан на скан всех слотов с выбором максимального tick; добавлен регрессионный тест buffer_latest_tracks_writes.

Баг№4
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\snapshot.rs (SnapshotBuffer::write_snapshot, ветка advance)
В чём заключается баг: при переполнении окна ветка делала двойной сдвиг индекса (сначала index += advance, потом ещё инкремент в цикле очистки) и затирала живые слоты, а get_snapshot после этого считал idx от неверной базы и возвращал чужие/пустые снапшоты.
Тип бага: Логический
Статус: Исправлен
Исправлен как: ветка переписана — сначала очищаются выпадающие слоты от текущего index, потом сдвигаются base и index, затем diff пересчитывается от новой базы; добавлен тест buffer_evicts_out_of_window_ticks.

Баг№5
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\connection.rs (fragment_large_packet), crates\net\src\client.rs, crates\net\src\server.rs, crates\core\src\packet.rs
В чём заключается баг: дельта с 24 сущностями не влезала в MTU и резалась на фрагменты старым 8-байтным форматом без msg_id, а сборки фрагментов на клиенте не было вообще — клиент пытался парсить каждый кусок как целый StateDeltaPacket и падал с Serialization error. Репликация умирала сразу после первого непустого снапшота.
Тип бага: Архитектурный (сетевой протокол)
Статус: Исправлен
Исправлен как: добавлен PacketType::Fragment = 0x23 с 13-байтным заголовком (orig_type, total, index, len, msg_id); написан FragmentAssembler (crates\net\src\fragment.rs) с вытеснением устаревших групп и TTL; подключён на клиенте (State/Delta/Full) и на сервере (Input/Command); добавлены 4 unit-теста (порядок/беспорядок/вытеснение/мусор).

Баг№6
Баг в файле: C:\RFS-0.4.0\server_src\crates\server\src\main.rs + crates\net\src\interest.rs
В чём заключается баг: на старте корабль регистрировали через update_entity_interest, который обновляет только spatial grid, но не регистрирует entity_layers. Без слоя should_replicate_entity всегда возвращал false — корабль не реплицировался.
Тип бага: Логический
Статус: Исправлен
Исправлен как: регистрация слоя делается через add_entity_to_interest (add_* методы InterestManager — единственное место регистрации слоёв).

Баг№7
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\server.rs (send_snapshots), crates\net\src\client.rs (process_state_packet), crates\core\src\packet.rs (StateDeltaPacket), crates\net\src\snapshot.rs (DeltaSnapshot, DeltaCompressor)
В чём заключается баг: отсутствовали слои репликации из плана §3.2/§3.4 — всё слалось одним потоком 30 Гц вместо L0 15 Гц / L2+L3 30 Гц / L1 по изменению + ресинк.
Тип бага: Архитектурный (отсутствующая фича из плана)
Статус: Исправлен
Исправлен как: добавлено поле layer в StateDeltaPacket/DeltaSnapshot; DeltaCompressor ведёт базы поключево (client, layer); сервер хранит per-client [LayerBase; 4] с расписанием LAYER_INTERVAL_TICKS/LAYER_RESYNC_TICKS; клиент применяет дельты к latest-снапшоту с upsert и отбрасывает stale по last_applied_layer; добавлены 3 теста слоёв; сквозной смоук показал послойную доставку (22→23→24 сущности).

Баг№8
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\connection.rs (строки 285, 306, 425)
В чём заключается баг: bincode::serialize(..).unwrap_or_default() маскирует ошибку сериализации пустым Vec, дальше отправляется пустой State/StateDelta, пир роняет его в deserialize. Ошибка превращается в молчаливую потерю.
Тип бага: Логический
Статус: не исправлен

Баг№9
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\connection.rs (строки 295, 316, 387, 426, 434) и crates\core\src\packet.rs (serialize_packet, payload.len() as u16)
В чём заключается баг: payload_size усекается через as u16 — при пейлоаде больше 65535 заголовок врёт и пир гарантированно дропает пакет по сверке размера. Сегодня ветки State/Delta/Fragment ограничены ~1400 байт, но OutgoingPacket::new/raw как публичное API без проверки пропустит большой Event/Command.
Тип бага: Сетевой
Статус: не исправлен

Баг№10
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\connection.rs (строки 287, 308, 369)
В чём заключается баг: max_packet_size - HEADER_SIZE [- FRAGMENT_HEADER_SIZE] — вычитание usize паникует в debug (и враппится в release в огромный max_payload) при конфиге max_packet_size меньше 24/37.
Тип бага: Паника
Статус: не исправлен

Баг№11
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\connection.rs (строка 164) и везде, где sequence сравнивается напрямую
В чём заключается баг: нет обработки врапа u32 sequence. После перехода MAX->0 remote_sequence застревает на MAX, ack'и замораживаются. Проявится только на очень долгих сессиях, но это вечный сервер.
Тип бага: Сетевой
Статус: не исправлен

Баг№12
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\connection.rs (строка 178) и crates\net\src\client.rs (строки 333, 349)
В чём заключается баг: RTT считается как Instant::now().elapsed() (время с текущего момента — всегда ~0), EWMA сползает к нулю. Метрика rtt врёт во всех логах.
Тип бага: Метрика
Статус: не исправлен

Баг№13
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\connection.rs (строки 212-224, process_acks) и crates\net\src\channel.rs (строки 129-148, 212-234)
В чём заключается баг: чистка подтверждений останавливается на первом неподтверждённом (break) — потеря первого пакета блокирует чистку всех следующих (head-of-line blocking), pending_acks растёт без границы. Там же нет учёта врапа sequence (см. баг №11).
Тип бага: Сетевой + Утечка памяти
Статус: не исправлен

Баг№14
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\connection.rs (строки 241, 279, 335, 350)
В чём заключается баг: pending_acks.push_back без границы — объявленная константа MAX_RELIABLE_WINDOW=32 нигде не проверяется. При потере ack'ов очередь растёт бесконечно.
Тип бага: Утечка памяти
Статус: не исправлен

Баг№15
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\channel.rs (строки 181-200, ReliableOrdered)
В чём заключается баг: remote_seq выставляется в sequence ДО match (строки 150-174), затем ветка ищет expected = remote+1 — in-order пакет никогда не отдаётся получателю (всегда Ok(None)). Надёжный упорядоченный канал молча глотает всё. Сейчас мёртвый код (Channel никто не вызывает из Connection), сработает при подключении каналов.
Тип бага: Логический (мёртвый код)
Статус: не исправлен

Баг№16
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\channel.rs (строки 177, 182)
В чём заключается баг: receive_buffer.insert без чистки — для ReliableUnordered вообще нет remove, лимиты max_fragments/ReceiveBufferFull не проверяются. Unbounded HashMap — утечка. Сейчас мёртвый код.
Тип бага: Утечка памяти (мёртвый код)
Статус: не исправлен

Баг№17
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\fragment.rs (строки 68-76)
В чём заключается баг: любой фрагмент того же типа с другим msg_id сносит ВСЕ незавершённые группы этого типа. При чередовании/запоздалом ретрае старого сообщения свежее тоже никогда не соберётся.
Тип бага: Сетевой
Статус: не исправлен

Баг№18
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\fragment.rs (строки 134-154, 142, 148, 149)
В чём заключается баг: make_fragments не отказывает при data больше MAX_REASSEMBLED_SIZE или чанков больше MAX_FRAGMENTS_PER_MESSAGE — отправитель нарезает то, что push заведомо отбросит. Плюс касты len as u16 / i as u16 усекаются при экстремальных размерах. Пустой data даёт total_chunks=0 и отправляет вообще ничего (молчаливая потеря, триггерится багом №8).
Тип бага: Сетевой
Статус: не исправлен

Баг№19
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\interest.rs (строки 188-257, compute_interest)
В чём заключается баг: для неизвестного player_id клонируется Default-интерес (позиция ZERO, nil-корабль) и ВСТАВЛЯЕТСЯ в карту. Любой spoofed id засоряет player_interests и гоняет запросы вокруг нулевой точки.
Тип бага: Логический
Статус: не исправлен

Баг№20
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\snapshot.rs (SnapshotBuffer::new + write/get)
В чём заключается баг: SnapshotBuffer::new(0) — деление %0 паникует в write/get/get_latest_tick. Никто сейчас с нулём не создаёт (дефолт 128), но конструктор это позволяет.
Тип бага: Паника
Статус: не исправлен

Баг№21
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\snapshot.rs (write_snapshot держит snapshots.write→base.write→index.write, get_latest_tick берёт base.read→index.read→snapshots.read)
В чём заключается баг: классическая инверсия порядка локов. Латентно: get_latest_tick сейчас никем не вызывается, но при подключении — дедлок двух потоков.
Тип бага: Состояние гонки/Дедлок (латентный)
Статус: не исправлен

Баг№22
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\snapshot.rs (DeltaCompressor::create_delta early-return) + crates\net\src\server.rs (обновление LayerBase)
В чём заключается баг: last_snapshots двигается даже на пустой early-return, а LayerBase на сервере — только при реальной отправке. base_tick в пакете и содержимое базы расходятся.
Тип бага: Логический
Статус: не исправлен

Баг№23
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\snapshot.rs (apply_delta, ветка update)
В чём заключается баг: update для отсутствующей в базе сущности молча скипается. При reorder/потере на ненадёжном канале состояние залипает до ресинка (150 тиков).
Тип бага: Сетевой
Статус: не исправлен

Баг№24
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\snapshot.rs (ProjectileUpdate::to_state_update, строки ~1035-1044 + apply ~967-971)
В чём заключается баг: поля None (без изменений) через unwrap_or_default превращаются в нули и безусловно затирают позицию/скорость/lifetime снаряда. Асимметрия с EntityUpdate, где Option сохраняется. Портит снаряды при частичных апдейтах.
Тип бага: Логический (порча данных)
Статус: не исправлен

Баг№25
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\snapshot.rs (interpolate, alpha)
В чём заключается баг: alpha=(t-b)/(a-b) без проверки a.time==b.time и без clamp — деление на ноль даёт inf/NaN, которые расползаются в позиции сущностей.
Тип бага: Математический
Статус: не исправлен

Баг№26
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\snapshot.rs (interpolate, построение результата)
В чём заключается баг: результат строится только из сущностей before — сущности, существующие только в after (заспавнились в интервале), выпадают из интерполированного кадра.
Тип бага: Логический
Статус: не исправлен

Баг№27
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\server.rs (строки 237-281)
В чём заключается баг: Connection создаётся по ЛЮБОМУ пакету от неизвестного адреса без проверки, что это Connect. Spray мусором исчерпывает max_connections=256 + даёт amplification через ConnectAccept. Плюс TOCTOU: проверка лимита под read, вставка под write — кап превышается.
Тип бага: Сетевой (DoS)
Статус: не исправлен

Баг№28
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\server.rs (send_packet, строка 327+)
В чём заключается баг: проверка размера идёт против константы MAX_PACKET_SIZE=1400 вместо config.max_packet_size; большие Event/CommandAck/InputAck не фрагментируются (фрагментируются только State/Delta) и молча дропаются через let _.
Тип бага: Сетевой
Статус: не исправлен

Баг№29
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\server.rs (send_snapshots)
В чём заключается баг: connections.read + client_layers.write держатся на весь цикл диффов по всем клиентам — блокирует disconnect/check_timeouts; при 256 клиентах × 4 слоя диффы каждый тик — деградация send loop.
Тип бага: Архитектурный (блокировки)
Статус: не исправлен

Баг№30
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\server.rs (обновление LayerBase после queue)
В чём заключается баг: база слоя обновляется сразу после постановки пакетов в очередь на ненадёжную отправку. Потеря пакета = дыра в состоянии клиента до ресинка (3-5 секунд залипания).
Тип бага: Сетевой
Статус: не исправлен

Баг№31
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\server.rs (строки ~358, tick_rate)
В чём заключается баг: интервал считается как 1000/tick_rate: при tick_rate=0 — паника деления на ноль, при tick_rate>1000 — интервал 0 мс и busy-loop.
Тип бага: Паника/Архитектурный
Статус: не исправлен

Баг№32
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\server.rs (broadcast_event)
В чём заключается баг: tokio::spawn на каждый пакет × каждое соединение без лимита — спам задачами при массовых событиях.
Тип бага: Архитектурный
Статус: не исправлен

Баг№33
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\client.rs (process_state_packet, ветка full State)
В чём заключается баг: полный State только insert'ит сущности, но не удаляет отсутствующие — удалённые на сервере сущности остаются ghost'ами у клиента навсегда.
Тип бага: Сетевой
Статус: не исправлен

Баг№34
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\client.rs (stale-защита слоёв)
В чём заключается баг: stale проверяется только по server_tick <= last_applied, base_tick игнорируется; server_tick перезаписывается тиком слоя без max — reorder откатывает часы, дыра вида 9→11 без 10 теряет переход 9→10 навсегда (до ресинка).
Тип бага: Сетевой
Статус: не исправлен

Баг№35
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\client.rs (ветка StateDelta, отсутствие latest)
В чём заключается баг: если первый full потерян (а full шлётся один раз по unreliable), а latest в буфере нет — все дельты скипаются, включая ресинки. Вечный stall до переподключения.
Тип бага: Сетевой
Статус: не исправлен

Баг№36
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\client.rs (эмиты событий в дельта-пути)
В чём заключается баг: на каждую дельту эмитится EntityUpdate по ВСЕМ сущностям target (а не только изменённым) — спам O(n) на 30 Гц подписчикам; а Projectile-события в дельта-пути вообще не эмитятся (только в full) — подписчики снарядов слепнут между полными стейтами.
Тип бага: Архитектурный
Статус: не исправлен

Баг№37
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\client.rs (send_packet_static)
В чём заключается баг: нет MTU-проверки перед отправкой (в отличие от строгого deserialize_packet у пира) — oversize Command уходит в сеть и дропается молча на другой стороне.
Тип бага: Сетевой
Статус: не исправлен

Баг№38
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\bandwidth.rs (строка 40)
В чём заключается баг: record_sent нигде не вызывается, только record_received — sent_bps всегда 0. Подтверждено смоуком (bw 0.0 во всех отчётах).
Тип бага: Метрика
Статус: не исправлен

Баг№39
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\bandwidth.rs (BandwidthLimiter)
В чём заключается баг: лимитер создан, но try_consume/wait_for нигде не вызываются — ограничения пропускной способности не работают вообще. Плюс set_max_bps меняет только max_tokens, не темп.
Тип бага: Архитектурный (мёртвый код)
Статус: не исправлен

Баг№40
Баг в файле: C:\RFS-0.4.0\server_src\crates\sim\src\tick.rs (строки 161, 182-206)
В чём заключается баг: collisions только растут (extend без чистки), попавшие снаряды не деспавнятся — каждый тик итерируется вся история (O(история), утечка памяти) и каждый тик наносится повторный урон, пока снаряд внутри bounds. Урон за тик вместо урона за попадание.
Тип бага: Логический (критический, горячий путь)
Статус: исправлен
Исправлен как: segment_intersects_bounds — swept-тест по отрезку (prev,curr) за тик (никакого туннелирования сквозь тонкую цель); перебор ship'ов компактный (Arc-клоны без вложенных блокировок); смена формулировки одна на тик; урон отложен на рубеж следующего тика (apply_pending_damage, план §5.1); двойная проверка: попавший снаряд деспавнится в collect_damage; history collisions очищается каждый тик (ровно один тик). Проверка: Tool\harness\tests\tick.rs (5 тестов, swept + деспавн + отсутствие повторного урона).

Баг№41
Баг в файле: C:\RFS-0.4.0\server_src\crates\sim\src\tick.rs (update_projectiles, строки 108-131)
В чём заключается баг: движение снарядов — хардкод (гравитация -9.81, Эйлер), вся ballistics.rs игнорируется: нет drag/воды/наведения/max_range, нет потолка. lifetime += dt при max_lifetime=NaN/отрицательном (поле pub, fire_projectile принимает любое) даёт бессмертный снаряд; spawn_tick пишется, но для expiry не читается.
Тип бага: Логический
Статус: не исправлен

Баг№42
Баг в файле: C:\RFS-0.4.0\server_src\crates\sim\src\tick.rs (calculate_impact_normal, строки 164-180)
В чём заключается баг: возвращает локально-осевой вектор как мировой без обратного поворота кватернионом — для повёрнутого корабля нормаль неверна; при попадании в центр signum(0)=0 — нулевая нормаль в ивент.
Тип бага: Математический
Статус: не исправлен

Баг№43
Баг в файле: C:\RFS-0.4.0\server_src\crates\sim\src\tick.rs + план §5.1
В чём заключается баг: урон применяется в том же тике (apply_damage сразу за check_collisions), а план требует применять результат этапа 2 на следующей границе тика. Клиент видит апдейт и ивент одного тика с разным health.
Тип бага: Архитектурный (нарушение плана)
Статус: не исправлен

Баг№44
Баг в файле: C:\RFS-0.4.0\server_src\crates\sim\src\ballistics.rs (строки 174, 379)
В чём заключается баг: get(&projectile_type).unwrap() — в таблице только 5 конфигов, а ProjectileType имеет 7 вариантов. Вызов с GrapeShot/DepthCharge — паника. Сейчас мёртвый код (тик не вызывает), сработает при подключении баллистики.
Тип бага: Паника (мёртвый код)
Статус: исправлен
Исправлен как: оба unwrap заменены на let-else (пустой вектор / None). Проверка: Tool\harness\tests\stability.rs (unknown_projectile_type_does_not_panic, GrapeShot без конфига).

Баг№45
Баг в файле: C:\RFS-0.4.0\server_src\crates\sim\src\ballistics.rs (строка 243) и solve_ballistic_arc (строки 397-409)
В чём заключается баг: acos(dot) без clamp(-1,1) — fp-округление даёт 1.0000001 → NaN в impact_angle; деления на horizontal_dist/v0*cos/g без guards — вертикальный выстрел и нулевая гравитация дают inf вместо None. Сейчас мёртвый код.
Тип бага: Математический (мёртвый код)
Статус: не исправлен

Баг№46
Баг в файле: C:\RFS-0.4.0\server_src\crates\sim\src\collision.rs (строка 582, raycast_box)
В чём заключается баг: 1.0/dir без обработки нулевой компоненты — 0*inf=NaN для луча в плоскости грани, хит пропускается. Классика slab-метода. Сейчас мёртвый код (тик использует Bounds::contains).
Тип бага: Математический (мёртвый код)
Статус: не исправлен

Баг№47
Баг в файле: C:\RFS-0.4.0\server_src\crates\ship\src\ship.rs (строка 246, update_stations)
В чём заключается баг: station_states.get_mut(station_id).unwrap() — достаточно подсунуть ShipState с неполным station_states (через публичный apply_state), и следующий update() запаникует внутри rayon-фазы тика, роняя весь тик.
Тип бага: Паника
Статус: исправлен
Исправлен как: if let вместо unwrap (в рамках ликвидации split-brain №53).

Баг№48
Баг в файле: C:\RFS-0.4.0\server_src\crates\ship\src\loader.rs (строки 107, 112)
В чём заключается баг: (comp_id.0 - 1000) / (station_id.0 - 2000) — вычитание u64 без проверки. EntityId::nil()==0 активно используется как плейсхолдер — в debug паника underflow, в release wrap до огромного usize и молча пустые связи.
Тип бага: Паника
Статус: исправлен
Исправлен как: функции с индексной арифметикой (resolve/get_compartment_name/get_station_name) удалены из loader.rs — связи строит Ship::new по именам (см. №52).

Баг№49
Баг в файле: C:\RFS-0.4.0\server_src\crates\ship\src\ship.rs (строки 212-213, 272)
В чём заключается баг: angle.clamp(-turn_rate, turn_rate) — f32::clamp паникует при min>max. turn_rate грузится из JSON без валидации — отрицательный turn_rate роняет каждый тик.
Тип бага: Паника
Статус: исправлен
Исправлен как: лимит берётся как turn_rate.abs() в update_physics и set_rudder. Проверка: Tool\harness\tests\stability.rs (negative_turn_rate_does_not_panic).

Баг№50
Баг в файле: C:\RFS-0.4.0\server_src\crates\core\src\math.rs (Quatf::normalize + Transform::default)
В чём заключается баг: Quatf::default() это (0,0,0,0), а normalize нулевого кватерниона даёт NaN, который расползается по всем трансформам. Transform::default() дегенеративен ({ZERO, (0,0,0,0), ZERO}) вместо IDENTITY — схлопывает точки и даёт inf в inverse. Любой пропущенный/нулевой кватернион из сети — тихий NaN.
Тип бага: Математический
Статус: исправлен
Исправлен как: Quatf::default() теперь IDENTITY (ручной impl Default вместо derive); normalize нулевого/нефинитного кватерниона возвращает IDENTITY вместо NaN. Проверка: Tool\harness\tests\stability.rs (quat_default_is_identity_and_zero_normalizes_cleanly).

Баг№51
Баг в файле: C:\RFS-0.4.0\server_src\crates\ship\src\ship.rs (строка 232)
В чём заключается баг: перепутаны yaw/pitch — state.transform.rotation = from_euler(0.0, heading, 0.0), а heading крутится через angular_velocity.y и должен идти первым параметром (yaw). Корабль при повороте кивает носом вместо разворота, forward уходит не туда.
Тип бага: Логический
Статус: исправлен
Исправлен как: from_euler(state.heading, 0.0, 0.0) — heading идёт yaw-параметром. Проверка: Tool\harness\tests\stability.rs (heading_rotates_around_y_not_pitch).

Баг№52
Баг в файле: C:\RFS-0.4.0\server_src\crates\ship\src\ship.rs + loader.rs + server\src\main.rs (схема EntityId)
В чём заключается баг: коллизии id: compartments = ship_id+1000+i, stations = ship_id+2000+i, bulkheads = comp+100, а STRIDE между кораблями всего 100. Корабль 110 имеет compartments 1110+, а у корабля 10 там же bulkheads 1110+ — прямое пересечение при --ships>1. Внутри корабля bulkheads разных отсеков тоже пересекаются. Плюс loader::resolve пушит 1000+i без базы ship_id, а get_compartment_name вычитает 1000 из уже-смещённого id — для любого ship_id!=0 топология пустая.
Тип бага: Архитектурный (схема id)
Статус: исправлен
Исправлен как: иерархические регионы внутри одного корабля — compartments ship_id+1000+rank, stations ship_id+2000+rank, bulkheads ship_id+3000+comp_rank*8+bh_rank; SHIP_ID_STRIDE поднят 100→10000 (server\src\main.rs, 64 корабля укладываются до 1_000_000 границы игроков). loader::resolve (индексная арифметика с вычитанием 1000/2000 и id без базы ship_id) удалён — связи строит Ship::new по именам (name→id). Заодно убран устаревший resolve_station_compartments в main.rs (лишний). Проверка: Tool\harness\tests\single_source.rs (entity_ids_do_not_collide_across_ships).

Баг№53
Баг в файле: C:\RFS-0.4.0\server_src\crates\ship\src\station.rs + ship.rs (два хранилища StationState)
В чём заключается баг: split-brain — Station держит внутреннее RwLock<StationState>, а Ship тикает копию в ShipState.station_states. Следствия: fire ставит cooldown во внутреннее, update тикает копию — орудие клинит после первого выстрела навсегда; set_target пишет target во внутреннее, update ведёт yaw копии к нулевому таргету — наведение заморожено; occupy пишет occupant в копию, can_occupy читает внутреннее (всегда None) — возможно двойное занятие. То же для отсеков (внутреннее vs копия).
Тип бага: Архитектурный (критический)
Статус: исправлен
Исправлен как: двойное хранилище ликвидировано — Station/Compartment стали держателями конфига без RwLock, единственное изменяемое состояние — ShipState.station_states/compartment_states. fire/set_target/reload/can_occupy/repair/apply_damage принимают &mut StationState, все вызовы идут через Ship::try_fire_station/set_station_target_angles/reload_station/station_angles (одна блокировка ship.state на вызов). update_stations кланяется без unwrap. Дополнительно: vacate_station больше не сбрасывает is_operational (иначе после выхода орудие не перезанять) — новый баг №76. Проверка: Tool\harness\tests\single_source.rs (occupancy_is_single_sourced_no_double_occupation, gun_fires_then_waits_out_cooldown_and_fires_again).

Баг№54
Баг в файле: C:\RFS-0.4.0\server_src\crates\ship\src\compartment.rs (строки 112-122, 264-277, 187-227)
В чём заключается баг: state.bulkhead_states всегда пуст (конфиг заполнен, стейт — нет): переборки неубиваемы, spread_water/spread_fire находят None и не ходят. Даже поверх этого spread_water только вычитает воду у себя без добавления соседу (объём уничтожается), а тело if в spread_fire пустое — огонь не перекидывается вообще.
Тип бага: Логический (критический для затопления)
Статус: исправлен
Исправлен как: initialize_compartments (ship.rs) строит состояние с рождения — bulkhead_states копируются в copy с связями connects_to (name→id), connected_compartments = реальные id отсеков. Урон по отсеку идёт в copy (Compartment::apply_damage(&mut CompartmentState)). Остойчивость/распространение воды-огня дорабатываются в §9/§11.

Баг№55
Баг в файле: C:\RFS-0.4.0\server_src\crates\ship\src\station.rs (строки 285-300), ship.rs (DamageTarget), compartment.rs (строки 264-277)
В чём заключается баг: нет двусторонних клампов — отрицательный damage оверхилит выше max (station/ship), bulkhead health уходит в минус бесконечно, отрицательный repair роняет health в минус при is_operational=true. Отрицательный урон лечит корпус.
Тип бага: Логический
Статус: исправлен
Исправлен как: Station::apply_damage клампится [0, max_health]; отрицательный repair игнорируется (max(0)); Ship::apply_health_damage клампится [0, max_health]; Compartment::apply_damage игнорирует damage<=0. Проверка: Tool\harness\tests\stability.rs (negative_damage_does_not_heal).

Баг№56
Баг в файле: C:\RFS-0.4.0\server_src\crates\ship\src\station.rs (строки 98-99, 147-174)
В чём заключается баг: диапазоны зеркалятся от верхней границы (yaw_range.0 игнорируется) — для асимметричных секторов неверно; наведение шагает без min(|diff|,step) — перелёт через цель и вечный джиттер; flooded/fire>0.5 ставит is_operational=false с защёлкой — само не восстанавливается после откачки; reload_time==0 из JSON даёт inf (мгновенная перезарядка) или вечный клин.
Тип бага: Логический
Статус: не исправлен

Баг№57
Баг в файле: C:\RFS-0.4.0\server_src\crates\ship\src\ship.rs (физика/потопление, строки 211-233, 252-265)
В чём заключается баг: нет проверки mass>0 (/mass → inf/NaN из JSON); check_sinking использует FIXED_DT вместо параметра dt (при смене тикрейта таймер врёт); пустой if sink_timer<=0 — потопление никогда не завершается, корабль не удаляется.
Тип бага: Логический
Статус: не исправлен

Баг№58
Баг в файле: C:\RFS-0.4.0\server_src\crates\server\src\main.rs (despawn_player) + crates\ship\src\ship.rs (occupy/vacate)
В чём заключается баг: despawn_player не зовёт vacate — occupant висит на отключившемся игроке, станция заблокирована навсегда и ghost-occupant реплицируется. Плюс occupy безусловно ставит is_operational=true (воскрешает мёртвую станцию посадкой), vacate безусловно ставит false (здоровая гаснет после выхода); проверка-и-запись неатомарны (TOCTOU).
Тип бага: Логический
Статус: не исправлен

Баг№59
Баг в файле: C:\RFS-0.4.0\server_src\crates\server\src\main.rs (apply_input, handle_station_interaction)
В чём заключается баг: нет арбитража и проверки владения — каждый инпут перезаписывает throttle/rudder (побеждает последний пакет тика), любой член экипажа может крутить/стрелять из любой пушки своего корабля без EnterStation и без occupant==игрок.
Тип бага: Архитектурный (авторизация)
Статус: не исправлен

Баг№60
Баг в файле: C:\RFS-0.4.0\server_src\crates\core\src\packet.rs (enum-дискриминанты)
В чём заключается баг: bincode 1.3 кодирует enum как u32-индекс варианта по порядку объявления, игнорируя явные =N. Перестановка вариантов молча ломает провод, хотя =N выглядит стабильным. Версионируется только PROTOCOL_VERSION.
Тип бага: Архитектурный (ловушка совместимости)
Статус: не исправлен

Баг№61
Баг в файле: C:\RFS-0.4.0\server_src\crates\core\src\packet.rs (StationStateData) vs entity.rs (StationEntity)
В чём заключается баг: сеть не везёт ammo_count/max_ammo/health/max_health/cooldown/max_cooldown станции и pump_active отсека — клиент никогда не узнает боезапас, здоровье и кулдаун. Потеря состояния в протоколе.
Тип бага: Архитектурный (протокол)
Статус: не исправлен

Баг№62
Баг в файле: C:\RFS-0.4.0\server_src\crates\core\src\spatial.rs (строки 130-137, update_position; константа MAX_ENTITIES_PER_CELL; query_radius)
В чём заключается баг: update_position игнорирует new_pos (remove+insert старой сущности — no-op); MAX_ENTITIES_PER_CELL=64 не enforced (insert безусловно пушит); query_radius без finite-проверок — radius=inf даёт цикл по миллиардам ячеек (hang), NaN-позиция схлопывается в ячейку 0.
Тип бага: Логический
Статус: не исправлен

Баг№63
Баг в файле: C:\RFS-0.4.0\server_src\crates\damage\src\propagation.rs (строки 88-162)
В чём заключается баг: снапшот клонируется один раз, затем до 10 итераций по stale-флагам делают intake/flow/pump — перелив до x10 за тик. pressure_diff — безразмерный ratio, результат — абсолютные уровни без нормировки на max_capacity (отсеки 10 и 10000 равняются одним куском, магия *100 недокументирована); переполнение цели молча отбрасывается — вода исчезает. flow_resistance/effective_resistance вообще не используется — сопротивление переборок не моделируется.
Тип бага: Логический + Математический
Статус: не исправлен
---
Баг№64
Баг в файле: C:\RFS-0.4.0\server_src\crates\damage\src\propagation.rs (строки 205-211, 256-276) + graph.rs (строки 126-131, 119-123, 89-91, 204-228, 249-268)
В чём заключается баг: распространение огня через fastrand — недетерминизм (реплеи/сетевой детерминизм невозможны), отсутствующий отсек считается горящим и блокирует спред; Flooded/Critical пушатся каждый тик без edge-триггера (флуд ивентов); remove_node инвалидирует NodeIndex без обновления node_map (чужой отсек или OOB); add_compartment при дубле оставляет сироту в графе; repair сбрасывает destroyed, но sealed остаётся false (отремонтированная переборка открыта); add_water/pump_water принимают отрицательные amount (дренаж/налив наоборот).
Тип бага: Логический
Статус: не исправлен
---
Баг№65
Баг в файле: C:\RFS-0.4.0\server_src\crates\damage\src\damage.rs (строки 68-102)
В чём заключается баг: дистанция считается до центра bounds — у края большого бокса недодамаг прямым и передоз соседям; apply_health_damage идёт вне falloff — прямое попадание даёт 0 по отсекам, но полные 100 по корпусу (подтверждено тестом damage.rs:185-187); отрицательный damage лечит корпус. Плюс лениво созданные ноды никогда не получают BulkheadEdge — граф продакшена без рёбер, течь физически некуда.
Тип бага: Логический
Статус: не исправлен
---
Баг№66
Баг в файле: C:\RFS-0.4.0\server_src\crates\stress\src\main.rs (BISECT-заглушка run_bot + run_bot_full с unimplemented!())
В чём заключается баг: временный отладочный скаффолд остался в дереве: run_bot урезан до sleep без чтения событий и входов (все метрики нули), рядом мёртвая run_bot_full с unimplemented!() (любой вызов — паника), плюс DBG eprintln-мусор. Оставлено при бисекции зависания (см. баг №69), требует удаления/восстановления полной версии.
Тип бага: Временный скаффолд
Статус: Исправлен
Исправлен как: полный run_bot восстановлен (цикл событий, входы с фазовым сдвигом, агрегация метрик), заглушка run_bot_full с unimplemented!() и DBG eprintln удалены.
---
Баг№67
Баг в файле: C:\RFS-0.4.0\server_src\crates\stress\src\main.rs + crates\net\src\client.rs
В чём заключается баг: event receiver бота не дренируется (или держится без чтения) — сервер шлёт ~30 Гц снапшоты, recv_loop пушит StateUpdate + N EntityUpdate в unbounded_channel. На 200 ботов за 20 секунд — сотни МБ/ГБ клонов Snapshot. Выглядит как hang/OOM на скейле.
Тип бага: Утечка памяти
Статус: не исправлен
---
Баг№68
Баг в файле: C:\RFS-0.4.0\server_src\crates\stress\src\main.rs (finish/disconnect) + net (handle_received_packet)
В чём заключается баг: disconnect не освобождает игрока: NetClient::disconnect шлёт UDP-пакет в фоне (может быть оборван шатдауном), сервер на Disconnect отвечает warn Unhandled и обновляет только last_received (что ПРОДЛЕВАЕТ таймаут). Фактическое освобождение — только через CONNECTION_TIMEOUT=10с. Комментарий «release immediately» в коде ложен.
Тип бага: Сетевой
Статус: не исправлен
---
Баг№69
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\client.rs (receive path, расследуется)
В чём заключается баг: traffic-triggered stall всего tokio-рантайма клиента: при входящем трафике от сервера 5-секундный sleep не срабатывает и за 10+ секунд, все ~40 потоков в Wait, CPU ~0. Без сервера тот же бинарник выходит чисто. Бисекция показала: RFS_PROBE_DROP=1 (не обрабатывать пакеты) — чистый выход; пропуск только State*/Fragment — всё равно виснет. Последний пакет перед фризом в трейсе — Heartbeat 0x05. Точный виновник внутри handle_received_packet не установлен.
Тип бага: Состояние гонки/Дедлок (расследовано, корень найден)
Статус: Исправлен
Исправлен как: корень — двойной rtt.lock() в одном выражении веток Heartbeat и HeartbeatAck клиента (внешний lock для присваивания + внутренний lock для as_millis; parking_lot нереентерабелен — поток парковался навсегда с нулевым CPU). Переписано на single acquisition через один guard; проверено пробами A–H (без обработки — выход чистый, только Heartbeat — виснет, без ack-send — виснет, try_lock вместо lock — чисто). Скаффолд проб удалён вместе с багом №70.
---
Баг№70
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\client.rs (временный скаффолд RFS_PROBE_DROP / RFS_PROBE_SKIP_STATE / RFS_PROBE_TRACE)
В чём заключается баг: отладочные env-ветки, добавленные для бисекции бага №69 (пропуск обработки пакетов, трейс типов в stderr). Инертны по умолчанию, но это мусор в прод-пути приёма пакетов — удалить после закрытия №69.
Тип бага: Временный скаффолд
Статус: Исправлен
Исправлен как: все env-ветки RFS_PROBE_DROP/SKIP_STATE/SKIP_HB/ONLY_HB/TRACE/HB_NOSEND и eprintln-трассировка удалены из receive path после закрытия №69.
---
Баг№71
Баг в файле: C:\RFS-0.4.0\server_src\crates\server\src\main.rs (ClientInput без ack) + crates\net\src\client.rs (pending_inputs)
В чём заключается баг: дроп ClientInput для неизвестного игрока/корабля идёт без отрицательного ack и без лога, а клиент чистит pending_inputs только по ack.accepted — очередь на клиенте растёт.
Тип бага: Сетевой
Статус: не исправлен
---
Баг№72
Баг в файле: C:\RFS-0.4.0\server_src\crates\stress\src\main.rs (summary) + crates\net\src\server.rs (max_connections=256)
В чём заключается баг: stress принимает до 1000 ботов, а сервер — 256 коннектов; лишние получают ServerFull, который харнесс не считает (ConnectionFailed не инкрементит errors) — саммари молча завышает средние (делит на успешные reports, а не на запрошенные bots).
Тип бага: Метрика
Статус: не исправлен
---
Баг№73
Баг в файле: C:\RFS-0.4.0\server_src\crates\core\src\entity.rs (StationEntity::position, CompartmentEntity::position) + crates\net\src\snapshot.rs (From<&StationEntity>, From<&CompartmentEntity>) + crates\server\src\main.rs (TrackedShip::compartment_entities/station_entities)
В чём заключается баг: позиции станций/отсеков хранились и отдавались в ship-local системе координат, а интерес меряет дистанцию в world. Для корабля в нуле случайно работало; для любого сдвинутого корабля все станции/отсеки выпадали из интереса (дистанция ~3000м против радиусов 100/200м). Симптом: 4 корабля + 1 бот давали 4 сущности вместо 26, 20 ботов — avg 13 вместо ~29. Клиент заодно получал локальные транформы (неверный рендер при движении корабля).
Тип бага: Логический (системы координат, план §8.1)
Статус: Исправлен
Исправлен как: добавлены world_bounds (CompartmentEntity) и world_transform (StationEntity) — вычисляются раз в тик из ship transform (позиция через transform_point, поворот композицией кватернионов; экстенты боксов без поворота, с докой); position()/bounds() и From-impls снапшотов переведены на мировые поля; локальные оставлены истиной. Проверка: 4×1 даёт 26.0 (свой комплект 24 + 2 соседа), 4×20 даёт avg 29.5, тик ~10мс.
---
Баг№74
Баг в файле: C:\RFS-0.4.0\server_src\crates\net\src\client.rs (send_connect_request)
В чём заключается баг: клиент шлёт ровно один Connect без ретрансмита. При burst-подключении (200 ботов за 3с) часть пакетов теряется даже на loopback — 16 из 200 так и не подключились, сервер их никогда не увидел. Замер §19.1: connected_at_end=184/200 при errors=0 (харнесс потерю тоже не считает).
Тип бага: Сетевой
Статус: не исправлен
---
Баг№75
Баг в файле: C:\RFS-0.4.0\server_src\crates\core\src\spatial.rs (SpatialGrid::query_radius)
В чём заключается баг: запрос всегда обходил все ячейки куба (radius 5000 / cell 1000 = 1331 hash-lookup'а), даже когда сущностей единицы. При 184 игроках это ~245к лукапов в тик только на корабли — тик деградировал до 46-119мс против бюджета 33мс (замер §19.1, debug). Плюс radius=inf давал цикл по миллиардам ячеек (hang).
Тип бага: Архитектурный (производительность)
Статус: Исправлен
Исправлен как: если число посещаемых ячеек сильно превышает число сущностей — линейный скан по всем сущностям (16 проверок вместо 1331 лукапа); добавлен guard на нефинитный/отрицательный радиус.
---
Баг№76
Баг в файле: C:\RFS-0.4.0\server_src\crates\ship\src\ship.rs (vacate_station)
В чём заключается баг: vacate_station ставил is_operational=false в копии. is_operational решает can_occupy — после первого выхода игрока орудие навсегда нельзя было занять (ремонта по §12 не было).
Тип бага: Логический
Статус: Исправлен
Исправлен как: vacate больше не трогает is_operational (операционность определяют health/затопление в update()); найден и закрыт боковым тестом occupancy_is_single_sourced_no_double_occupation.
---

Баг№77
Баг в файле: C:\RFS-0.4.0\src\rhi\resource\buffer.rs (структура Buffer, строки 20-30)
В чём заключается баг: Buffer не реализует Clone (14 мест: деривайте Clone в дескрипторных массивах, в т.ч. в связке (Buffer, u64)) и Default (1 место). cargo check: "the trait bound `resource::buffer::Buffer: Clone` is not satisfied" ×14 и "`: Default`" ×1 — 15 ошибок E0277 на один тип. Рендер-граф не может клонировать/дефолтить буферы.
Тип бага: Компиляция (этикетки трейтов)
Статус: не исправлен

Баг№78
Баг в файле: C:\RFS-0.4.0\src\rhi\types\flags.rs (bitflags AccessFlags ст.87, PipelineStage ст.71, ColorComponentFlags ст.142, ShaderStage ст.21+57, TextureAspectFlags ст.48)
В чём заключается баг: пять bitflags не реализуют Default: AccessFlags — 8 ошибок, PipelineStage — 4, штучно ColorComponentFlags/ShaderStage/TextureAspectFlags — по 1. Всего 15 ошибок E0277. Каждый `#[derive(Default)]` на структурах с этими полями (пайплайны, барьеры, сабпассы) не собирается; флагу нельзя задать «пустое» значение по умолчанию.
Тип бага: Компиляция (этикетки трейтов)
Статус: не исправлен

Баг№79
Баг в файле: C:\RFS-0.4.0\src\rhi\resource\texture.rs (enum TextureLayout ст.70, struct TextureView ст.63)
В чём заключается баг: TextureLayout не реализует Default (5 ошибок — везде используются дефолтные значения при создании вью/аспектов), TextureView не реализует Clone (4 ошибки). Итого 9 ошибок E0277. Нельзя сделать вью клонируемой и задать layout по умолчанию.
Тип бага: Компиляция (этикетки трейтов)
Статус: не исправлен

Баг№80
Баг в файле: C:\RFS-0.4.0\src\rhi\pipeline\graphics.rs (enum'ы ст.61-178) + C:\RFS-0.4.0\src\rhi\types\primitives.rs (PrimitiveTopology ст.184)
В чём заключается баг: enum'ы состояния растеризации не реализуют Default: BlendFactor (4), StencilOp (3), BlendOp (2), PipelineShaderStage (2), PolygonMode (1), CullMode (1), FrontFace (1), LogicOp (1) — в graphics.rs, PrimitiveTopology (1) — в primitives.rs; плюс GraphicsPipeline не реализует Clone (1). Итого 17 ошибок E0277. GraphcisPipelineDesc не может собраться с derive(Default).
Тип бага: Компиляция (этикетки трейтов)
Статус: не исправлен

Баг№81
Баг в файле: C:\RFS-0.4.0\src\rhi\resource\sampler.rs (enum FilterMode ст.11, AddressMode ст.18, CompareOp ст.28, BorderColor ст.41, struct Sampler ст.70)
В чём заключается баг: FilterMode (3), AddressMode (3), CompareOp (3), BorderColor (1) не реализуют Default, Sampler не реализует Clone (1). Итого 11 ошибок E0277. Нельзя создать сэмплер со значениями по умолчанию и клонировать его при передаче в декс.
Тип бага: Компиляция (этикетки трейтов)
Статус: не исправлен

Баг№82
Баг в файле: C:\RFS-0.4.0\src\rhi\shader\module.rs (enum ShaderFormat ст.11, struct ShaderModule ст.30) + C:\RFS-0.4.0\src\rhi\shader\compiler.rs (OptimizationLevel ст.30, ShaderTargetEnv ст.38)
В чём заключается баг: ShaderModule не реализует Clone (2), ShaderFormat/OptimizationLevel/ShaderTargetEnv не реализуют Default (по 1). Итого 5 ошибок. Модуль шейдера нельзя клонировать в кэш, компилятору нельзя задать параметры по умолчанию.
Тип бага: Компиляция (этикетки трейтов)
Статус: не исправлен

Баг№83
Баг в файле: C:\RFS-0.4.0\src\rhi\command\pass\render.rs (enum LoadOp ст.12, StoreOp ст.20, PipelineBindPoint ст.47, SubpassFlags ст.55, DependencyFlags ст.75; struct RenderPass ст.103, Framebuffer ст.136)
В чём заключается баг: LoadOp (2), StoreOp (2), PipelineBindPoint/SubpassFlags/DependencyFlags (по 1) не реализуют Default; RenderPass не реализует Clone (2)+Default (2), Framebuffer — Clone (1)+Default (1). Итого 13 ошибок. RenderPassDesc/FramebufferDesc с derive(Default) не собираются.
Тип бага: Компиляция (этикетки трейтов)
Статус: не исправлен

Баг№84
Баг в файле: C:\RFS-0.4.0\src\rhi\swapchain\swapchain.rs (ColorSpace ст.13, PresentMode ст.22, SurfaceTransform ст.31, CompositeAlpha ст.41, FullscreenExclusive ст.50) + C:\RFS-0.4.0\src\rhi\swapchain\surface.rs (struct Surface ст.16)
В чём заключается баг: все пять enum — по 1 ошибке Default, Surface — по 1 Default и 1 Clone. Итого 7 ошибок. SwapchainDesc не собирается с derive(Default), Surface нельзя клонировать при передаче в контекст/обмен.
Тип бага: Компиляция (этикетки трейтов)
Статус: не исправлен

Баг№85
Баг в файле: C:\RFS-0.4.0\src\rhi\command\buffer.rs (CommandBufferLevel ст.12, CommandBufferFlags ст.30) + C:\RFS-0.4.0\src\rhi\command\pool.rs (CommandPoolFlags ст.22)
В чём заключается баг: CommandBufferLevel, CommandBufferFlags и CommandPoolFlags не реализуют Default — по 1 ошибке на каждый, итого 3. Descriptor/дескс командного буфера и пула с derive(Default) не собираются.
Тип бага: Компиляция (этикетки трейтов)
Статус: не исправлен

Баг№86
Баг в файле: C:\RFS-0.4.0\src\rhi\query\pool.rs (enum QueryType ст.11, bitflags QueryPoolFlags ст.22)
В чём заключается баг: QueryType и QueryPoolFlags не реализуют Default — 2 ошибки. QueryPoolDesc с derive(Default) не собирается.
Тип бага: Компиляция (этикетки трейтов)
Статус: не исправлен

Баг№87
Баг в файле: C:\RFS-0.4.0\src\rhi\sync\semaphore.rs (bitflags SemaphoreFlags ст.12)
В чём заключается баг: SemaphoreFlags не реализует Default — 1 ошибка. SemaphoreDesc с derive(Default) не собирается.
Тип бага: Компиляция (этикетки трейтов)
Статус: не исправлен

Баг№88
Баг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\blas.rs (struct Blas ст.95) + C:\RFS-0.4.0\src\rhi\resource\acceleration\tlas.rs (struct Tlas ст.42)
В чём заключается баг: Blas не реализует Clone (2), Tlas — Clone (1). Итого 3 ошибки. BLAS/TLAS нельзя клонировать при построении иерархии ускорения.
Тип бага: Компиляция (этикетки трейтов)
Статус: не исправлен

Баг№89
Баг в файле: C:\RFS-0.4.0\src\rhi\descriptor\set.rs (struct DescriptorSet ст.12) + C:\RFS-0.4.0\src\rhi\descriptor\layout.rs (struct DescriptorSetLayout ст.53)
В чём заключается баг: DescriptorSet не реализует Clone (2), DescriptorSetLayout — Clone (1). Итого 3 ошибки. Дескрипторные сеты/лейауты нельзя клонировать при переиспользовании в пайплайнах.
Тип бага: Компиляция (этикетки трейтов)
Статус: не исправлен

Баг№90
Баг в файле: C:\RFS-0.4.0\src\rhi\pipeline\compute.rs (struct ComputePipeline ст.17)
В чём заключается баг: ComputePipeline не реализует Clone — 1 ошибка. Вычислительный пайплайн нельзя клонировать при реюзе в графе/контексте.
Тип бага: Компиляция (этикетки трейтов)
Статус: не исправлен

Баг№91
Баг в файле: C:\RFS-0.4.0\src\rhi\descriptor\layout.rs (строка 33)
В чём заключается баг: E0425 — "cannot find type `Sampler` in this scope". Поле immutable_samplers: Option<Vec<Sampler>> в DescriptorSetLayoutDesc объявлено без импорта типа Sampler (resource::sampler::Sampler). Даже после добавления Clone/Default дескс не скомпилируется без этого импорта.
Тип бага: Синтаксический (отсутствующий импорт)
Статус: не исправлен

Баг№92
Баг в файле: C:\RFS-0.4.0\src\rhi\shader\stage.rs (строка 15)
В чём заключается баг: E0599 — "no associated function or constant named `INTERSECTION` found for struct `flags::ShaderStage`". Констá INTERSECTION_SHADER использует ShaderStage::INTERSECTION, но в bitflags ShaderStage (types/flags.rs) такого флага нет (только VERTEX/FRAGMENT/COMPUTE/RAY_GEN/ANY_HIT/CLOSEST_HIT/MISS и т.п.). Нельзя объявить интерсекшн-шейдер.
Тип бага: Синтаксический (несуществующий член bitflags)
Статус: не исправлен

Баг№93
Баг в файле: C:\RFS-0.4.0\src\render (весь модуль, 78 файлов) — блокируется багами №77-92
В чём заключается баг: модуль Render зависит от RHI; пока пакет rhi не компилируется (137 ошибок выше), `cargo check` клиента падает на RHI раньше, чем доходит до Render. Реальные ошибки Render (битые пути, missing импорты, конфликты типов EffectSettings/EffectQuality между effects, particles и core::settings) сейчас не видны и будут видны только после закрытия №77-92.
Тип бага: Архитектурный (порядок сборки/блокер верификации)
Статус: не исправлен
---

Баг№94
Баг в файле: C:\RFS-0.4.0\src\rhi\lib.rs (строки 26-49)
В чём заключается баг: rhi не реэкспортирует на корень типы из resource/pipeline/shader/command/descriptor/sync/swapchain/query (реэкспортируются только config/error/types/core/backend::Backend). Любой `use crate::rhi::{Texture, Buffer, Sampler, CommandEncoder, RenderPass, Pipeline, ...}` в render-коде даёт E0432/E0433. Это корень десятков битых импортов во всех пассах, материалах, мешах, текстурах.
Тип бага: Битые связи модулей (реэкспорт)
Статус: не исправлен

Баг№95
Баг в файле: C:\RFS-0.4.0\src\lib.rs (строки 42-50)
В чём заключается баг: корень rfs-client делает `pub use rhi::{BufferDesc, CommandBuffer, DeviceFeatures, DeviceType, FormatCaps, GraphicsPipeline, Image, ImageDesc, ImageUsage, PipelineLayoutDesc, QueueType, Sampler, ShaderModule, SwapChain, Texture, ...}` — большинства этих имён нет в корне rhi (нет DeviceFeatures — есть Features, нет DeviceType — есть PhysicalDeviceType, нет FormatCaps — есть DeviceCaps, нет Image/ImageDesc/ImageUsage/QueueType/PipelineLayoutDesc вообще; текстурные/баферные/командные типы не реэкспортированы). E0432 прямо в lib.rs — весь клиент не собирается.
Тип бага: Битые связи модулей (реэкспорт)
Статус: не исправлен

Баг№96
Баг в файле: C:\RFS-0.4.0\src\rhi\mod.rs (строка 8) + C:\RFS-0.4.0\src\rhi\prelude.rs (строка 5)
В чём заключается баг: mod.rs объявляет `pub mod lib;` (циклическое самовключение), prelude.rs делает `pub use crate::rhi::{...}` на несуществующий модуль crate::rhi (корень — lib.rs). Оба файла «мёртвые», ни один не объявлен в lib.rs. Если их включить — крейт не соберётся.
Тип бага: Синтаксический (битые пути/цикл модулей)
Статус: не исправлен

Баг№97
Баг в файле: C:\RFS-0.4.0\Cargo.toml (строки 27-30, 54-62) + C:\RFS-0.4.0\src\lib.rs (строка 27)
В чём заключается баг: rhi одновременно зависит как path-крейт (`rhi = { path = "src/rhi" }`, `[workspace] members = ["src/rhi", "."]`, `[workspace.dependencies] rhi = ...`) И объявлен локальным модулем `pub mod rhi;`. Внутренние пути use crate::types::* / crate::error::* внутри src/rhi/ при компиляции как встроенного модуля ссылаются на rfs-client (где таких модулей нет) — каскад E0432. Зависимости неоднозначны, дублируются.
Тип бага: Архитектурный (структура крейта/workspace)
Статус: не исправлен

Баг№98
Баг в файле: C:\RFS-0.4.0\src\render\core\renderer.rs (строки 39,41,45-50,56-69,92-104,138,148,156-157,168-176,223)
В чём заключается баг: Renderer::new использует: raw_window_handle::HasRawWindowHandle (зависимость winit/raw-window-handle закомментирована в Cargo.toml — E0432); crate::rhi::create_device_and_queues(...), device.create_swapchain(...), device.name()/vendor()/memory()/features()/limits(), config.width/height/vsync (у RhiConfig таких полей нет), self.swapchain.present(&Queue) (реально present(u32)), swapchain.resize()/extent() (методов нет). RenderContext::new ждёт GraphicsSettings, а передаёт RendererSettings. Все — E0599/E0308. Плюс unwrap() на acquire_next_image/present/resize (паника на стабах).
Тип бага: Битые связи модулей (API Device/SwapChain/RhiConfig)
Статус: не исправлен

Баг№99
Баг в файле: C:\RFS-0.4.0\src\render\core\settings.rs (строки 278-285 и 640-646)
В чём заключается баг: `pub enum RayTracingQuality` объявлен ДВАЖДЫ в одном файле — E0428 (duplicate definition), плюс пресеты apply_high/apply_medium_preset (стр. 566 и др.) присваивают advanced.ray_tracing_quality. Файл не компилируется.
Тип бага: Синтаксический (дубликат типа)
Статус: не исправлен

Баг№100
Баг в файле: C:\RFS-0.4.0\src\render\core\context.rs (строки 8,20,53,68,75,79,83,90-103) + C:\RFS-0.4.0\src\render\core\resource_manager.rs (строки 5,8,161-170,369)
В чём заключается баг: context.rs импортирует rhi::{Swapchain, Texture, TextureView, TextureViewDesc} — их нет на корне rhi (есть SwapChain). Методы swapchain.extent()/get_current_view()/image_count(), texture.create_view()/extent(), device.create_texture(), Format::Depth32Float — не существуют. resource_manager.rs импортирует Texture дважды (rhi и render::textures — E0252), вызывает Mesh::load/cone/torus, Texture::load/color/procedural, Material::load, MaterialLibrary::insert, Device::default() — таких методов/реализаций нет.
Тип бага: Битые связи модулей (API RHI) + Синтаксический
Статус: не исправлен

Баг№101
Баг в файле: C:\RFS-0.4.0\src\render\graph\node.rs (строка 6,10) + C:\RFS-0.4.0\src\render\graph\resource.rs (строки 85,110)
В чём заключается баг: `ResourceUsage` импортирован приватно (`use super::types::ResourceUsage`) и не попал в `pub use` — все 7 пассов (base.rs:31, gbuffer.rs:18, lighting.rs:18, shadow.rs:16, transparent.rs:14, ui.rs:13, water.rs:17) дают `use crate::render::graph::node::ResourceUsage` → E0432. Также node.rs:6 и resource.rs:85,110 используют rhi::{CommandEncoder, TextureView} (нет в корне) и `Format::Undefined` (нет такого варианта в rhi).
Тип бага: Битые связи модулей (приватный реэкспорт)
Статус: не исправлен

Баг№102
Баг в файле: C:\RFS-0.4.0\src\render\graph\types.rs (строки 25-38)
В чём заключается баг: оператор BitOr у ResourceUsage НЕ коммутативен и теряет комбинации: Sampled | Storage → Sampled, Storage | Sampled → Storage, ColorAttachment | Sampled → ColorAttachment. Объединение usage ресурса, используемого несколькими пассами, схлопывается в неверную комбинацию — тихий отбор неправильного формата/вью.
Тип бага: Логический (операторы)
Статус: не исправлен

Баг№103
Баг в файле: C:\RFS-0.4.0\src\render\graph\graph.rs (строки ~33-43, 66-155, 199-232) + resource.rs (строка 121)
В чём заключается баг: RenderGraph фактически пуст: add_pass нигде не вызывается (grep по всему src — только определение), initialize() регистрирует ресурсы с texture: None, GraphResource::create() нигде не вызывается, execute() проходит пустой execution_order (no-op). Внутренние текстуры пассов (position_texture и т.п.) в resources не кладутся → любые super::base::get_texture(resources, "...") всегда вернут None. Даже при рабочей компиляции граф не отрисует ни одного кадра.
Тип бага: Архитектурный (пустой граф/нет регистрации пассов)
Статус: не исправлен

Баг№104
Баг в файле: C:\RFS-0.4.0\src\render\graph\graph.rs (строки ~118-124) vs C:\RFS-0.4.0\src\render\passes\shadow.rs (строка 62) vs lighting.rs (строки 86, 283-285)
В чём заключается баг: в графе регистрируется одна текстура "shadow_map", а пассы создают/читают "shadow_map_cascade_{0..3}" (итерация (0..4) в lighting.rs жёстко зашита, не зависит от cascade_count). Граф никогда не свяжет ресурс с пассом — тени молча не работают.
Тип бага: Логический (несогласованность имён ресурсов)
Статус: не исправлен

Баг№105
Баг в файле: C:\RFS-0.4.0\src\render\scene\scene.rs (строка 220) + C:\RFS-0.4.0\src\render\scene\culling.rs (строки 89-100, 188)
В чём заключается баг: frustum_culler.is_visible(&aabb, &view_proj) вызывается с двумя аргументами, а сигнатура is_visible(&self, aabb: &Aabb) — один (E0061). FrustumCuller::update нигде не вызывается → отсечение работает с placeholder-плоскостями Vec3::ZERO; параметр context в cull/update не используется.
Тип бага: Синтаксический (E0061) + Логический (неинициализированный фрустум)
Статус: не исправлен

Баг№106
Баг в файле: C:\RFS-0.4.0\src\render\scene\entity.rs (строка 206) + C:\RFS-0.4.0\src\render\scene\components.rs (строки 152, 180, 241, 267-271)
В чём заключается баг: context.camera_position() не существует (метод есть у Scene) — E0599, вся LOD-логика (distance_from_camera) сломана. Производные `#[derive(Clone, Debug)]` на SceneComponent/MeshComponent/MaterialComponent при полях Arc<Mesh>/Material/PbrMaterial/WaterMaterial и Arc<dyn Camera> без супертрейта Debug — E0277. Entity::create_standard_camera выдаёт заглушку.
Тип бага: Синтаксический (E0599/E0277)
Статус: не исправлен

Баг№107
Баг в файле: C:\RFS-0.4.0\src\render\camera\controller.rs (строки ~214-225) + C:\RFS-0.4.0\src\render\camera\camera.rs (строки 78, 82-86, 221-227)
В чём заключается баг: downcast_ref::<OrthographicCamera>() на Box<dyn Camera> без супертрейта Any — E0599; даже после каста запись camera.left = ... требует &mut, а downcast_ref даёт & (E0596). В look_at параметр up ИГНОРИРУЕТСЯ (всегда Vec3::Y) — для камеры с нестандартным up неверная матрица. viewport: (0,0, aspect*near, near) — физически бессмысленный прямоугольник. frustum_planes возвращает [Mat4; 6] — тип-заглушка, не стыкуется с Frustum из scene/culling.
Тип бага: Синтаксический + Логический (игнор up, мусорный viewport/frustum)
Статус: не исправлен

Баг№108
Баг в файле: C:\RFS-0.4.0\src\render\passes\water.rs (строки 116,120 vs 159,161 и 301,339)
В чём заключается баг: поля объявлены как reflection_framebuffer/water_framebuffer, конструктор инициализирует и использует reflection_frame_buffer/water_frame_buffer (другие имена) — E0560 (struct has no field) + E0063 (missing fields). Файл не компилируется.
Тип бага: Синтаксический (опечатка имён полей)
Статус: не исправлен

Баг№109
Баг в файле: C:\RFS-0.4.0\src\render\passes\transparent.rs (строки 15, 27, 257, 296)
В чём заключается баг: импорт `use crate::render::passes::base::{BaseRenderPass, BlendMode}` — в base.rs BlendMode НЕТ (E0432) + локально в transparent.rs объявлен СВОЙ pub enum BlendMode (E0255-конфликт, и ещё один BlendMode есть в materials::material). entity.blend_mode() возвращает materials::material::BlendMode, а TransparentObject.blend_mode — локальный transparent::BlendMode — несовпадение типов (E0308).
Тип бага: Синтаксический (несуществующий импорт/дубль типа) + Логический
Статус: не исправлен

Баг№110
Баг в файле: C:\RFS-0.4.0\src\render\passes\lighting.rs (строки 224, 292, 298, 301)
В чём заключается баг: `let clear_colors: Vec<[f32;4]> = vec![...]` затем push — нет mut (E0596); `attachments.len() - 1 as u32` — usize - u32 (E0308); (0..4) жёстко зашито в чтении каскадов теней вместо config.cascade_count (логический).
Тип бага: Синтаксический (E0596/E0308) + Логический
Статус: не исправлен

Баг№111
Баг в файле: C:\RFS-0.4.0\src\render\lighting\light.rs (строки 139-163, 365-381)
В чём заключается баг: UB-касты `unsafe { &*(self as *const Light as *const DirectionalLight) }` — реинтерпретация меньшей структуры Light как большей PointLight/SpotLight (чтение байтов за пределами объекта, мусор в position/direction/range). Плюс .with_color()/.with_intensity() вызываются на DirectionalLight/PointLight/SpotLight, где этих методов нет (E0599 — они объявлены только на базовом Light).
Тип бага: Логический (UB/небезопасные касты) + Синтаксический
Статус: не исправлен

Баг№112
Баг в файле: C:\RFS-0.4.0\src\render\passes\mod.rs (строка 13) + C:\RFS-0.4.0\src\render\passes\base.rs (строка 10) + C:\RFS-0.4.0\src\render\lighting\shadows.rs (строки 168, 259)
В чём заключается баг: passes/mod.rs делает `pub use base::RenderPass;`, но RenderPass в base.rs импортирован приватно (use crate::render::graph::node::{RenderPass, ...}) — E0365 (private re-export). shadows.rs вызывает crate::rhi::Device::default().create_texture(...) — Device не реализует Default и create_texture не существует (E0599/E0277).
Тип бага: Синтаксический (приватный реэкспорт, несуществующие методы)
Статус: не исправлен

Баг№113
Баг в файле: C:\RFS-0.4.0\src\render\passes\ui.rs (строки 211-212, 126-132) + C:\RFS-0.4.0\src\render\passes\gbuffer.rs (строки 81-85, 122, 246) + transparent.rs (строки 144, 166, 282) + shadow.rs (строки 103, 127, 193) + water.rs (строки 207, 250, 351)
В чём заключается баг: пассы используют несуществующие члены RHI: Format::RGBA16Float/RGBA8Unorm/R8Unorm/Depth32Float (в rhi есть SCREAMING_SNAKE имена: RGBA32_SFLOAT, D32_SFLOAT и т.п.), TextureLayout::ColorAttachmentOptimal (его нет), AttachmentDescription.samples передаётся числом 1 вместо SampleCount::X1, Rect2D задаётся плоскими полями {x,y,width,height} вместо {offset, extent}. В ui.rs методы трейта Camera используются без импорта трейта.
Тип бага: Синтаксический (несовпадение API с rhi)
Статус: не исправлен

Баг№114
Баг в файле: C:\RFS-0.4.0\src\render\meshes\mesh.rs (строки 120-121, 278-302, 316-366, 431-432, 445-450, 489-495, 509-521)
В чём заключается баг: `pub struct MeshFlags: u32` — невалидный синтаксис (E0403/E0658). Размер index-buffer считается `type.size() * indices.len()`, но indices всегда Vec<u32> — для U8/U16 буфер выделяется в 2-4 раза меньше данных (переполнение/обрезка). Более того, геометрия неверна: левая грань куба имеет обратный wind-порядок (невидима при backface-culling), сфера построена «наизнанку» (видна изнутри), плоскость norm-normal вниз (-Y) — вода с CullMode::Back невидима сверху, крышки цилиндра закручены в чужие кольца.
Тип бага: Синтаксический + Логический (геометрия/индексы)
Статус: не исправлен

Баг№115
Баг в файле: C:\RFS-0.4.0\src\render\water\surface.rs (строки 27,40,49-53,71-79) + C:\RFS-0.4.0\src\render\water\waves.rs (строки 124,138-152,142,152,160,165-180)
В чём заключается баг: Vec2 не импортирован в surface.rs (E0412); wave.direction.dot(position.xz()) — Vec3::dot(Vec2), 4 места (E0308). Двойное масштабирование воды: create_mesh уже строит plane размера size, model_matrix дополнительно умножает на size (реальный размер size² — при 128 → 16384). resolution/tessellation_factor не учитываются. get_simple_normal на спокойной воде (0,0,0).normalize() даёт NaN-нормаль; нормаль жёстко y=0 и не соответствует геометрии (height считает steepness², normal — amplitude*steepness).
Тип бага: Синтаксический + Логический (масштаб, NaN-нормали)
Статус: не исправлен

Баг№116
Баг в файле: C:\RFS-0.4.0\src\render\textures\texture.rs (строки 46-66, 282-300, 421-450) + C:\RFS-0.4.0\src\render\textures\library.rs (строки 95, 122-123) + C:\RFS-0.4.0\src\render\meshes\library.rs (строка 36)
В чём заключается баг: impl From<TextureFormat> for Format — 16 ветвей мапят на несуществующие варианты RHI Format (RGBA16Unorm, Depth32Float и т.п.) (E0599). new_cube(...layers...) — слой layers отбрасывается, куб-текстура всегда 1 слой вместо 6. TextureUsage::Storage → RhiTextureUsage::SAMPLED — STORAGE-бит теряется (SSAO/SSR в compute не получат STORAGE). library.rs: Device::default() не существует (E0599), vec![128u8,128u8,255u8,255u8; 256*256] — невалидный vec!-синтаксис с потроением списка. meshes/library.rs: (*mesh).clone() — Mesh не реализует Clone (E0277).
Тип бага: Синтаксический + Логический (слои, usage-бит)
Статус: не исправлен

Баг№117
Баг в файле: C:\RFS-0.4.0\src\render\materials\material.rs (строки 6-8, 92, 114-195) + materials\pbr.rs (строки 18, 314) + materials\water.rs (строки 113-114, 167-168) + materials\library.rs
В чём заключается баг: Material/PbrMaterial/WaterMaterial не реализуют Debug/Clone, но scene/components.rs диреивирует MaterialComponent с ними (E0277). material.rs строит PipelineDesc с полями shader_modules/vertex_input/rasterizer/depth_stencil/blend/layouts, которых нет у RHI GraphicsPipelineDesc (там vertex_shader/fragment_shader/input/...), плюс вызов device.create_graphics_pipeline (E0599). resource_manager ждёт PbrMaterial::new(name,config), а pbr.rs имеет new(name) (E0061). set_clarity клампит поле, но в uniform шлёт сырое значение — рассинхрон. PbrMaterial::water() даёт MaterialType::Pbr вместо Water.
Тип бага: Синтаксический + Логический
Статус: не исправлен

Баг№118
Баг в файле: C:\RFS-0.4.0\src\render\meshes\loader.rs (строки 12-30)
В чём заключается баг: OBJ/GLTF/FBX-лоадеры — заглушки: load_obj(...) → Some(Mesh::cube()), load_gltf → cube(), load_fbx → cube(). Любой .obj/.gltf/.fbx в игре молча рендерится кубом, без ошибок и предупреждений. Реального парсинга индексов нет.
Тип бага: Логический (молчаливая заглушка)
Статус: не исправлен

Баг№119
Баг в файле: C:\RFS-0.4.0\src\render\postprocess\manager.rs (строка 83) + C:\RFS-0.4.0\src\render\postprocess\mod.rs (строки 26-30) + C:\RFS-0.4.0\src\render\effects\manager.rs (строки 319-322)
В чём заключается баг: postprocess/manager.rs вызывает scene.get_velocity_texture() — у Scene такого метода/текстуры нет (E0599). PostProcessConfig.bloom/motion_blur/hdr — это bool, а effects/manager.rs читает их как вложенные `.bloom.enabled` (PostProcessSettings-модель из core/settings.rs) — код смешивает две несовместимые модели настроек. Плюс Format::RGBA16Float/Depth32Float в initialize (см. №113).
Тип бага: Синтаксический (E0599) + Логический (конфликт моделей настроек)
Статус: не исправлен

Баг№120
Баг в файле: C:\RFS-0.4.0\src\render\mod.rs (строки 63, 67) + C:\RFS-0.4.0\src\render\particles\mod.rs (строки 12-15) + C:\RFS-0.4.0\src\render\particles\effect_manager.rs + C:\RFS-0.4.0\src\render\effects\manager.rs + C:\RFS-0.4.0\src\render\core\settings.rs (строки 45, 192-199)
В чём заключается баг: render/mod.rs:63 реэкспортирует ParticleEffect/ParticleEffectType — таких типов нет (есть ParticleEffectManager, EffectHandle; ParticleEffect объявлен в particles/effects.rs? — фактически в модуле нет, E0432). Типы EffectSettings/EffectQuality определены ТРИ раза (effects/manager.rs, particles/effect_manager.rs, core/settings.rs) с разными полями; EffectSettings в core/settings.rs — третий тип, не реэкспортированный. render::EffectSettings из render/mod.rs приходит только из effects. Три параллих системы эффектов несовместимы.
Тип бага: Архитектурный (тройное дублирование типов) + Синтаксический
Статус: не исправлен

Баг№121
Баг в файле: C:\RFS-0.4.0\src\render\effects\manager.rs + C:\RFS-0.4.0\src\render\particles\effect_manager.rs
В чём заключается баг: оба менеджера вызывают `context.device()` как метод, но у RenderContext поле device (не метод) — E0599. Плюс в particles/effect_manager.rs и effects/manager.rs дублируются понятия EffectHandle/EffectSettings (см. №120).
Тип бага: Синтаксический (несуществующий метод)
Статус: не исправлен

Баг№122
Баг в файле: C:\RFS-0.4.0\src\render\water\mod.rs (строки 12, 63) + C:\RFS-0.4.0\src\render\passes\water.rs (строки 21, 35-46, 55-70) + C:\RFS-0.4.0\src\render\water\waves.rs (строки 9-12)
В чём заключается баг: water/mod.rs делает `pub use super::WaterRenderer;` И одновременно объявляет `pub struct WaterRenderer` — двойное определение WaterRenderer в модуле (E0255). WaterConfig/WaveMethod определены по два раза: render/water vs render/passes/water — два набора полей с одинаковыми именами, несовместимы. Передача одного типа в WaveSystem::new (ждущий WaveMethod из своего модуля) — конфликт типов.
Тип бага: Синтаксический (E0255 дубль) + Архитектурный (дубли типов)
Статус: не исправлен

Баг№123
Баг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\tlas.rs (строки 18-26, 42-45) + C:\RFS-0.4.0\src\rhi\resource\texture.rs (строки 63-66 vs 43-53)
В чём заключается баг: TlasInstance хранит BLAS по значению (вместе с Vec<AccelerationStructureGeometry>, содержащей Buffer) — бессмысленные копии GPU-описаний на каждый инстанс и семантически неверно. TextureView { texture: Texture, desc: TextureViewDesc } при том, что и TextureViewDesc содержит texture: Texture — одна сущность в двух местах без инварианта согласованности.
Тип бага: Архитектурный (дублирование данных)
Статус: не исправлен

Баг№124
Баг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs (строки 183-188, 150-154) + C:\RFS-0.4.0\src\rhi\pipeline\graphics.rs (строки 42-50) + C:\RFS-0.4.0\src\rhi\core\device.rs (строки 33-37) + C:\RFS-0.4.0\src\rhi\memory\heap.rs (строки 10-13) + C:\RFS-0.4.0\src\rhi\resource\sampler.rs (строка 52) vs C:\RFS-0.4.0\src\render\textures\texture.rs (строка 147)
В чём заключается баг: дублирование типов с одинаковым именем: PrimitiveTopology (types/primitives.rs и pipeline/graphics.rs), MemoryHeap (core/device.rs {size,flags} и memory/heap.rs {index,size,flags}), SamplerDesc (rhi/resource/sampler.rs и render/textures/texture.rs). При glob-импортах — E0659 ambiguity, тихая подмена значений по контракту.
Тип бага: Архитектурный (дубли типов)
Статус: не исправлен

Баг№125
Баг в файле: C:\RFS-0.4.0\src\rhi\sync\barrier.rs (строки 28-35, 57-60, 18-25) + C:\RFS-0.4.0\src\rhi\command\pass\render.rs (строки 83-92)
В чём заключается баг: TextureBarrier не имеет subresource_range (аспект/мип/слой) — нельзя переводить layout отдельного depth-аспекта; TextureAspectFlags определён, но нигде не используется (потеряно поле). BufferBarrier/TextureBarrier не имеют src/dst_queue_family_index (нет ownership transfer между очередями). BufferTextureCopy лишён image_offset/image_extent/image_subresource (физически нереализуемо). SubpassDependency не имеет VK_SUBPASS_EXTERNAL (=!0u32), поля u32 не выражают «весь пасс».
Тип бага: Архитектурный (неполнота до Vulkan)
Статус: не исправлен

Баг№126
Баг в файле: C:\RFS-0.4.0\src\rhi\types\flags.rs (строки 57-67, 65) + C:\RFS-0.4.0\src\rhi\shader\stage.rs (строки 8-18) + C:\RFS-0.4.0\src\rhi\types\features.rs (строки 10-12)
В чём заключается баг: наборы стадий шейдеров неполны и несовместимы: stage.rs не имеет GEOMETRY/TESSELLATION_CONTROL/TESSELLATION_EVALUATION/MESH/TASK/CALLABLE констант, flags.rs не имеет соответсвующих битов (включая INTERSECTION, баг №92). ALL_GRAPHICS = VERTEX|FRAGMENT неполон (не включает geometry/tessellation/mesh). При этом features.rs заявляет geometry_shader/tessellation_shader/mesh_shader — фичи невыразимы через ShaderStage. Mesh/RayTracing-пайплайны невозможно описать.
Тип бага: Архитектурный (некомпатные стадии шейдеров)
Статус: не исправлен

Баг№127
Баг в файле: C:\RFS-0.4.0\src\rhi\commands?.rs — C:\RFS-0.4.0\src\rhi\utils\conversion.rs (строки 15-26) + C:\RFS-0.4.0\src\rhi\utils\alignment.rs (строки 6-18) + C:\RFS-0.4.0\src\rhi\utils\hash.rs (строки 22-24)
В чём заключается баг: to_vulkan/to_dxgi/to_gl у всех Format — unimplemented!() (любая конвертация формата — гарантированная паника). align_up(value, 0) → alignment-1 = u64::MAX → value+u64::MAX переполнение (debug-паника); is_aligned(x,0) всегда false; нет guard на 0. hash_pipeline_desc всегда возвращает 0 — кэш пайплайнов будет иметь тотальные коллизии.
Тип бага: Логический (unimplemented/паника) + Архитектурный (кэш)
Статус: не исправлен

Баг№128
Баг в файле: C:\RFS-0.4.0\src\rhi\utils\hash.rs — кэш пайплайнов (см. №127) + C:\RFS-0.4.0\src\rhi\pipeline\graphics.rs (строки 231-241) + C:\RFS-0.4.0\src\rhi\pipeline\compute.rs (строки 12-14) + C:\RFS-0.4.0\src\rhi\pipeline\state.rs (строки 10-14)
В чём заключается баг: GraphicsPipelineDesc/ComputePipelineDesc НЕ содержат layout: PipelineLayout и render_pass/subpass — дескрипторные сеты и push-константы невозможно связать с пайплайном через типы; PipelineLayout/PushConstantRange — мёртвый код. line_width.rasterizer дефолт 0.0 (невалидно, API требует 1.0); дефолт CullMode=None (обычно Back). Device::wait_idle(), SwapChain::acquire_next_image/present, allocator/backend/semaphore/fence — unimplemented!() (паника при первом же вызове).
Тип бага: Архитектурный (неполный API пайплайнов/синхронизации)
Статус: не исправлен

Баг№129
Баг в файле: C:\RFS-0.4.0\src\render\passes\lighting.rs (строки 276-285) + transparent.rs (строки 96-99, 247) + water.rs (строки 131-134, 342-345) + C:\RFS-0.4.0\src\render\lighting\ (light.rs, shadows.rs, probe.rs) + C:\RFS-0.4.0\src\render\scene\components.rs (LightComponent)
В чём заключается баг: три несвязанных представления света: пассы не используют lighting::{Light, DirectionalLight, PointLight, SpotLight}, ShadowPass опирается на свой локальный ShadowConfig, сцена хранит LightComponent. Пассы fallback'ов полагаются на имена ресурсов (gbuffer_position, depth, lighting), которых нет в графе (см. №103). LightProbe::bake/get_light — заглушки (get_light всегда Vec3::ONE). lighting/shadows.rs vs passes/shadow.rs — два разных ShadowConfig с одним именем.
Тип бага: Архитектурный (разрыв световой модели) + Логический
Статус: не исправлен

Баг№130
Баг в файле: C:\RFS-0.4.0\src\rhi\resource\buffer.rs (строки 20-30, поле device_address) + C:\RFS-0.4.0\src\rhi\resource\sampler.rs (строки 70-72) + C:\RFS-0.4.0\src\rhi\swapchain\swapchain.rs (строки 76, 83, images) 
В чём заключается баг: Buffer.device_address всегда None, геттера нет — поле мёртвое (ускоренные бэкенды не могут получить адрес буфера). Sampler не имеет конструктора (публичных полей нет) — неконструируем. SwapChainImage — мёртвый тип: images = Vec::new() в new(), acquire/current_image_index никогда не заполняются.
Тип бага: Архитектурный (мёртвые поля/неконструируемые типы)
Статус: не исправлен

Баг№131
Баг в файле: C:\RFS-0.4.0\src\render\particles\effects.rs + C:\RFS-0.4.0\src\render\particles\system.rs + C:\RFS-0.4.0\src\render\particles\emitter.rs
В чём заключается баг: ParticleEffect/ParticleEffectType реэкспортированы в render/mod.rs:63, но фактически (по глубокой проверке) этих типов нет в particles/effects.rs (там SmokeEffect/FireEffect/... и EffectQuality) — E0432 на корне render. Плюс частицы не имеют подключения к RHI-квизам/пайплайнам компута — скорее всего система мертва. Требуется сверка фактического содержимого particles при исправлении.
Тип бага: Синтаксический (несуществующий реэкспорт)
Статус: не исправлен

Баг№132
Баг в файле: C:\RFS-0.4.0\src\render\ui\ (hud.rs, minimap.rs, menu.rs) + C:\RFS-0.4.0\src\render\passes\ui.rs (строки 57-58, 119-132, 195, 289)
В чём заключается баг: ui pass использует swapchain.load_op: Load и layout TransferDstOptimal→TransferSrcOptimal, но никто не делает TransferSrc-конверсию перед презентом — логическая незавершённость цепочки рендера UI. ui.rs:57-58 зависит от "water/water" и "transparent/final", но ресурс "water" в графе не зарегистрирован. self.output_view.as_ref().and_then(|v| v.texture().device()) — TextureView.texture()/device() не существуют.
Тип бага: Логический + Синтаксический
Статус: не исправлен

Баг№133
Баг в файле: C:\RFS-0.4.0\src\render\core\renderer.rs (строки ~132, ~151)
В чём заключается баг: frame_number инкрементится ДВАЖДЫ за кадр (в begin_frame и в конце render_frame) — статистика FPS/кадров завышена вдвое. RenderStats создаётся, но нигде не заполняется наружу (мёртвый код).
Тип бага: Логический (двойной инкремент)
Статус: не исправлен

Баг№134
Баг в файле: C:\RFS-0.4.0\src\rhi\command\commands.rs (строки 88-94) + C:\RFS-0.4.0\src\rhi\sync\barrier.rs (строки 38-45)
В чём заключается баг: Command::PipelineBarrier полностью дублирует структуру PipelineBarrier из sync/barrier.rs (src_stage/dst_stage/memory_barriers/buffer_barriers/texture_barriers). При эволюции типов они расползутся — заложенная мина рассинхронизации (сейчас поля совпадают).
Тип бага: Архитектурный (дублирование структуры)
Статус: не исправлен

Баг№135
Баг в файле: C:\RFS-0.4.0\src\render\particles\effect_manager.rs (EffectSettings, строки 41-66) + C:\RFS-0.4.0\src\render\effects\manager.rs (EffectSettings, строки 51-59) + C:\RFS-0.4.0\src\render\core\settings.rs (EffectSettings, строки 192-199, 315, 391)
В чём заключается баг: полевые наборы трёх EffectSettings различаются: particle-версия (quality, spawn_rate?, max_particles?), effects-версия (quality + поля эффектов), settings-версия (ssao/ssr/god_rays...). RendererSettings.effects: EffectSettings (core-версия) передаётся в EffectManager (effects-версия) — несовпадение типов при использовании. Система эффектов/частиц/настроек не работает как единое целое.
Тип бага: Архитектурный (конфликт типов настроек)
Статус: не исправлен

Баг№136
Баг в файле: C:\RFS-0.4.0\src\render\passes\base.rs (строки 25-40) + C:\RFS-0.4.0\src\render\passes\shadow.rs (строки 20-39)
В чём заключается баг: BaseRenderPass регистрирует пустые pipelines: HashMap::new() и заглушку execute() — «проход» ничего не рисует. ShadowPass имеет cascаде-count жёстко зашитый (0..4 в lighting, и 4 каскада в shadow.rs) без привязки к ShadowConfig из lighting — конфиги рассинхронизированы.
Тип бага: Логический (пустые пайплайны, жёсткие каскады)
Статус: не исправлен

Баг№137
Баг в файле: C:\RFS-0.4.0\src\render\passes\water.rs (строки 242, 453-460) + C:\RFS-0.4.0\src\render\water\ (surface.rs, waves.rs, interaction.rs, mod.rs)
В чём заключается баг: водный проход НЕ использует render/water систему (WaterSurface, WaveSystem, WaterInteraction, WaterRenderer): water_mesh: Option<Mesh> всегда None, вода никогда не получает геометрию; проход имеет собственные WaterConfig/WaveMethod/WaveData. Два параллельных «водных мира», не связанных между собой: рендер-модуль воды имеет реальную реализацию, а проход — заглушку.
Тип бага: Архитектурный (две несвязанные системы воды)
Статус: не исправлен

Баг№138
Баг в файле: C:\RFS-0.4.0\src\rhi\utils\pkg + backends: C:\RFS-0.4.0\src\rhi\backend\ (vulkan/d3d12/d3d11/opengl)
В чём заключается баг: все бэкенды при включённых фичах (default vulkan+d3d12+d3d11+opengl в rhi/Cargo.toml) — unimplemented!() и практически пустые стабы: нет никакой реальной ширины конверсии и создания объектов. Система рендеринга на данном этапе физически не может ничего отрисовать на любом бэкенде — это не «баг компиляции», а архитектурное состояние «скелета».
Тип бага: Архитектурный (пустые бэкенды)
Статус: не исправлен

Баг№139
Баг в файле: C:\RFS-0.4.0\src\render\core\renderer.rs (строки 111-118) + C:\RFS-0.4.0\src\render\graph\graph.rs
В чём заключается баг: Renderer::initialize() не вызывает self.context.initialize() и пассы не добавляются в граф (add_pass нигде не вызывается, см. №103). Даже после компиляционных правок инициализация рендера не выполнит ни создания текстуры глубины/свапчейна, ни регистрации пассов — пустой кадр.
Тип бага: Логический (неинициализированная цепочка)
Статус: не исправлен

Баг№140
Баг в файле: C:\RFS-0.4.0\src\rhi\types\flags.rs (строки 22, 35, 57, 71, 87, 107, 142, 48)
В чём заключается баг: разнобой в derive: BufferUsage/TextureUsage/MemoryPropertyFlags имеют Default, а ShaderStage/PipelineStage/AccessFlags/TextureAspectFlags/ColorComponentFlags — нет (см. №78). Это создаёт асимметричный контракт: половина bitflags дефолтна, половина нет — источник каскадных E0277 везде, где derive(Default). Требуется единая политика.
Тип бага: Архитектурный (асимметрия derive)
Статус: не исправлен
---