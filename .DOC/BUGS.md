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