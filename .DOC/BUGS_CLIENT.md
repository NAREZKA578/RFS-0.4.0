# Детальный реестр ошибок компиляции клиента RFS (rfs-client)

**ИТОГОВЫЙ СТАТУС (обновлено 2026-09-16): весь клиент (crate rfs-client) успешно компилируется — `cargo check` = 0 ошибок.**
Все ошибки из этого реестра устранены (правки в rhi, render и связанных модулях).

---

# Детальный реестр ошибок компиляции клиента RFS (rfs-client) (исторический)

Создан для максимально подробной фиксации ВСЕХ ошибок компиляции с нумерацией.
Однотипные ошибки объединены в одну запись, внутри перечислены все строки-локации.
Источник: cargo check (клиент), 418 ошибок исходно, 206 групп.

Пример записи бага в реестор:

Баг№(Номер бага, по порядку начиная с 1)
Баг в файле: (Понлый путь до файла и самим файлом, пример: C:\RFS-0.4.0\.DOC\BUGS.md)
В чём заключается баг: (подробное описани что за баг и что он портит, и т.д.)
Тип бага: (Синтаксический, стилистический, архетектурный и так далее...)
Статус: (Исправлен\не исправлен)
Если исправлен в должно быть написано: "Исправлен как: (и тут полная роспись как исправлялся данный конкретный баг)".

---

Баг№1
Баг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Код ошибки: E0583
Строки-локации: 26
В чём заключается баг: file not found for module `config`
Тип бага: Ошибка компиляции (E0583)
Статус: исправлен

Баг№2
Баг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Код ошибки: E0583
Строки-локации: 27
В чём заключается баг: file not found for module `error`
Тип бага: Ошибка компиляции (E0583)
Статус: исправлен

Баг№3
Баг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Код ошибки: E0583
Строки-локации: 28
В чём заключается баг: file not found for module `types`
Тип бага: Ошибка компиляции (E0583)
Статус: исправлен

Баг№4
Баг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Код ошибки: E0583
Строки-локации: 29
В чём заключается баг: file not found for module `core`
Тип бага: Ошибка компиляции (E0583)
Статус: исправлен

Баг№5
Баг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Код ошибки: E0583
Строки-локации: 30
В чём заключается баг: file not found for module `backend`
Тип бага: Ошибка компиляции (E0583)
Статус: исправлен

Баг№6
Баг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Код ошибки: E0583
Строки-локации: 31
В чём заключается баг: file not found for module `resource`
Тип бага: Ошибка компиляции (E0583)
Статус: исправлен

Баг№7
Баг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Код ошибки: E0583
Строки-локации: 32
В чём заключается баг: file not found for module `pipeline`
Тип бага: Ошибка компиляции (E0583)
Статус: исправлен

Баг№8
Баг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Код ошибки: E0583
Строки-локации: 33
В чём заключается баг: file not found for module `shader`
Тип бага: Ошибка компиляции (E0583)
Статус: исправлен

Баг№9
Баг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Код ошибки: E0583
Строки-локации: 34
В чём заключается баг: file not found for module `command`
Тип бага: Ошибка компиляции (E0583)
Статус: исправлен

Баг№10
Баг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Код ошибки: E0583
Строки-локации: 35
В чём заключается баг: file not found for module `sync`
Тип бага: Ошибка компиляции (E0583)
Статус: исправлен

Баг№11
Баг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Код ошибки: E0583
Строки-локации: 36
В чём заключается баг: file not found for module `memory`
Тип бага: Ошибка компиляции (E0583)
Статус: исправлен

Баг№12
Баг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Код ошибки: E0583
Строки-локации: 37
В чём заключается баг: file not found for module `descriptor`
Тип бага: Ошибка компиляции (E0583)
Статус: исправлен

Баг№13
Баг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Код ошибки: E0583
Строки-локации: 38
В чём заключается баг: file not found for module `swapchain`
Тип бага: Ошибка компиляции (E0583)
Статус: исправлен

Баг№14
Баг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Код ошибки: E0583
Строки-локации: 39
В чём заключается баг: file not found for module `query`
Тип бага: Ошибка компиляции (E0583)
Статус: исправлен

Баг№15
Баг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Код ошибки: E0583
Строки-локации: 40
В чём заключается баг: file not found for module `debug`
Тип бага: Ошибка компиляции (E0583)
Статус: исправлен

Баг№16
Баг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Код ошибки: E0583
Строки-локации: 41
В чём заключается баг: file not found for module `utils`
Тип бага: Ошибка компиляции (E0583)
Статус: исправлен

Баг№17
Баг в файле: C:\RFS-0.4.0\src\render\core\settings.rs
Код ошибки: E0428
Строки-локации: 641
В чём заключается баг: the name `RayTracingQuality` is defined multiple times
Тип бага: Ошибка компиляции (E0428)
Статус: исправлен

Баг№18
Баг в файле: C:\RFS-0.4.0\src\render\particles\system.rs
Код ошибки: E0255
Строки-локации: 200
В чём заключается баг: the name `Particle` is defined multiple times
Тип бага: Ошибка компиляции (E0255)
Статус: исправлен

Баг№19
Баг в файле: C:\RFS-0.4.0\src\render\particles\mod.rs
Код ошибки: E0432
Строки-локации: 14
В чём заключается баг: unresolved imports `effects::ParticleEffect`, `effects::ParticleEffectType`
Тип бага: Ошибка компиляции (E0432)
Статус: исправлен

Баг№20
Баг в файле: C:\RFS-0.4.0\src\lib.rs
Код ошибки: E0432
Строки-локации: 35
В чём заключается баг: unresolved import `render::ParticleEffect`
Тип бага: Ошибка компиляции (E0432)
Статус: исправлен

Баг№21
Баг в файле: C:\RFS-0.4.0\без локации
Код ошибки: E0432
Строки-локации: —
В чём заключается баг: unresolved imports `super::RhiConfig`, `super::MemoryAllocatorConfig`, `super::ValidationConfig`, `super:
Тип бага: Ошибка компиляции (E0432)
Статус: исправлен

Баг№22
Баг в файле: C:\RFS-0.4.0\src\render\core\renderer.rs
Код ошибки: E0432
Строки-локации: 8
В чём заключается баг: unresolved imports `crate::rhi::Device`, `crate::rhi::GraphicsApi`, `crate::rhi::RhiConfig`
Тип бага: Ошибка компиляции (E0432)
Статус: исправлен

Баг№23
Баг в файле: C:\RFS-0.4.0\src\render\core\context.rs
Код ошибки: E0432
Строки-локации: 8
В чём заключается баг: unresolved import `crate::rhi::Device`
Тип бага: Ошибка компиляции (E0432)
Статус: исправлен

Баг№24
Баг в файле: C:\RFS-0.4.0\без локации
Код ошибки: E0432
Строки-локации: —
В чём заключается баг: unresolved imports `crate::rhi::Device`, `crate::rhi::Buffer`, `crate::rhi::Texture`, `crate::rhi::Shader
Тип бага: Ошибка компиляции (E0432)
Статус: исправлен

Баг№25
Баг в файле: C:\RFS-0.4.0\src\render\graph\graph.rs
Код ошибки: E0432
Строки-локации: 11
В чём заключается баг: unresolved imports `crate::rhi::Device`, `crate::rhi::CommandEncoder`
Тип бага: Ошибка компиляции (E0432)
Статус: исправлен

Баг№26
Баг в файле: C:\RFS-0.4.0\src\render\graph\node.rs
Код ошибки: E0432
Строки-локации: 6
В чём заключается баг: unresolved imports `crate::rhi::CommandEncoder`, `crate::rhi::TextureView`
Тип бага: Ошибка компиляции (E0432)
Статус: исправлен

Баг№27
Баг в файле: C:\RFS-0.4.0\без локации
Код ошибки: E0432
Строки-локации: —
В чём заключается баг: unresolved imports `crate::rhi::Texture`, `crate::rhi::Buffer`, `crate::rhi::TextureView`, `crate::rhi::F
Тип бага: Ошибка компиляции (E0432)
Статус: исправлен

Баг№28
Баг в файле: C:\RFS-0.4.0\src\render\passes\base.rs
Код ошибки: E0432
Строки-локации: 6
В чём заключается баг: unresolved import `crate::rhi::CommandEncoder`
Тип бага: Ошибка компиляции (E0432)
Статус: исправлен

Баг№29
Баг в файле: C:\RFS-0.4.0\без локации
Код ошибки: E0432
Строки-локации: —
В чём заключается баг: unresolved imports `crate::rhi::CommandEncoder`, `crate::rhi::Pipeline`, `crate::rhi::PipelineDesc`, `cra
Тип бага: Ошибка компиляции (E0432)
Статус: исправлен

Баг№30
Баг в файле: C:\RFS-0.4.0\без локации
Код ошибки: E0432
Строки-локации: —
В чём заключается баг: unresolved imports `crate::rhi::CommandEncoder`, `crate::rhi::Pipeline`, `crate::rhi::TextureView`, `crat
Тип бага: Ошибка компиляции (E0432)
Статус: исправлен

Баг№31
Баг в файле: C:\RFS-0.4.0\без локации
Код ошибки: E0432
Строки-локации: —
В чём заключается баг: unresolved imports `crate::rhi::Device`, `crate::rhi::ShaderModule`, `crate::rhi::Pipeline`, `crate::rhi:
Тип бага: Ошибка компиляции (E0432)
Статус: исправлен

Баг№32
Баг в файле: C:\RFS-0.4.0\src\render\meshes\mesh.rs
Код ошибки: E0432
Строки-локации: 7
В чём заключается баг: unresolved imports `crate::rhi::Buffer`, `crate::rhi::BufferUsage`
Тип бага: Ошибка компиляции (E0432)
Статус: исправлен

Баг№33
Баг в файле: C:\RFS-0.4.0\без локации
Код ошибки: E0432
Строки-локации: —
В чём заключается баг: unresolved imports `crate::rhi::Texture`, `crate::rhi::TextureView`, `crate::rhi::Format`, `crate::rhi::T
Тип бага: Ошибка компиляции (E0432)
Статус: исправлен

Баг№34
Баг в файле: C:\RFS-0.4.0\без локации
Код ошибки: E0432
Строки-локации: —
В чём заключается баг: unresolved imports `crate::rhi::Device`, `crate::rhi::CommandEncoder`, `crate::rhi::Pipeline`, `crate::rh
Тип бага: Ошибка компиляции (E0432)
Статус: исправлен

Баг№35
Баг в файле: C:\RFS-0.4.0\без локации
Код ошибки: E0432
Строки-локации: —
В чём заключается баг: unresolved imports `crate::rhi::Device`, `crate::rhi::CommandEncoder`, `crate::rhi::TextureView`, `crate:
Тип бага: Ошибка компиляции (E0432)
Статус: исправлен

Баг№36
Баг в файле: C:\RFS-0.4.0\без локации
Код ошибки: E0432
Строки-локации: —
В чём заключается баг: unresolved imports `rhi::Buffer`, `rhi::BufferDesc`, `rhi::CommandBuffer`, `rhi::CommandEncoder`, `rhi::D
Тип бага: Ошибка компиляции (E0432)
Статус: исправлен

Баг№37
Баг в файле: C:\RFS-0.4.0\src\render\graph\resource.rs
Код ошибки: E0433
Строки-локации: 147
В чём заключается баг: cannot find `TextureViewDesc` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№38
Баг в файле: C:\RFS-0.4.0\src\render\passes\gbuffer.rs
Код ошибки: E0433
Строки-локации: 122, 133, 144, 155, 166
В чём заключается баг: cannot find `SampleCount` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№39
Баг в файле: C:\RFS-0.4.0\src\render\passes\gbuffer.rs
Код ошибки: E0433
Строки-локации: 123, 125, 134, 136, 145, 147, 156, 158, 167, 169
В чём заключается баг: cannot find `LoadOp` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№40
Баг в файле: C:\RFS-0.4.0\src\render\passes\gbuffer.rs
Код ошибки: E0433
Строки-локации: 124, 126, 135, 137, 146, 148, 157, 159, 168, 170
В чём заключается баг: cannot find `StoreOp` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№41
Баг в файле: C:\RFS-0.4.0\src\render\passes\gbuffer.rs
Код ошибки: E0433
Строки-локации: 127, 128, 138, 139, 149, 150, 160, 161, 171, 172, 179, 180, 181, 182, 184
В чём заключается баг: cannot find `TextureLayout` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№42
Баг в файле: C:\RFS-0.4.0\src\render\passes\gbuffer.rs
Код ошибки: E0433
Строки-локации: 177
В чём заключается баг: cannot find `PipelineBindPoint` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№43
Баг в файле: C:\RFS-0.4.0\src\render\passes\shadow.rs
Код ошибки: E0433
Строки-локации: 118
В чём заключается баг: cannot find `TextureViewDesc` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№44
Баг в файле: C:\RFS-0.4.0\src\render\passes\shadow.rs
Код ошибки: E0433
Строки-локации: 126
В чём заключается баг: cannot find `SampleCount` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№45
Баг в файле: C:\RFS-0.4.0\src\render\passes\shadow.rs
Код ошибки: E0433
Строки-локации: 127, 129
В чём заключается баг: cannot find `LoadOp` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№46
Баг в файле: C:\RFS-0.4.0\src\render\passes\shadow.rs
Код ошибки: E0433
Строки-локации: 128, 130
В чём заключается баг: cannot find `StoreOp` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№47
Баг в файле: C:\RFS-0.4.0\src\render\passes\shadow.rs
Код ошибки: E0433
Строки-локации: 131, 132, 140
В чём заключается баг: cannot find `TextureLayout` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№48
Баг в файле: C:\RFS-0.4.0\src\render\passes\shadow.rs
Код ошибки: E0433
Строки-локации: 136
В чём заключается баг: cannot find `PipelineBindPoint` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№49
Баг в файле: C:\RFS-0.4.0\src\render\passes\lighting.rs
Код ошибки: E0433
Строки-локации: 177, 179, 198, 200, 216, 218
В чём заключается баг: cannot find `LoadOp` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№50
Баг в файле: C:\RFS-0.4.0\src\render\passes\lighting.rs
Код ошибки: E0433
Строки-локации: 178, 180, 199, 201, 217, 219
В чём заключается баг: cannot find `StoreOp` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№51
Баг в файле: C:\RFS-0.4.0\src\render\passes\lighting.rs
Код ошибки: E0433
Строки-локации: 181, 182, 189, 202, 203, 207, 220, 221, 225
В чём заключается баг: cannot find `TextureLayout` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№52
Баг в файле: C:\RFS-0.4.0\src\render\passes\lighting.rs
Код ошибки: E0433
Строки-локации: 230
В чём заключается баг: cannot find `PipelineBindPoint` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№53
Баг в файле: C:\RFS-0.4.0\src\render\passes\transparent.rs
Код ошибки: E0433
Строки-локации: 166, 177
В чём заключается баг: cannot find `SampleCount` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№54
Баг в файле: C:\RFS-0.4.0\src\render\passes\transparent.rs
Код ошибки: E0433
Строки-локации: 167, 169, 178, 180
В чём заключается баг: cannot find `LoadOp` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№55
Баг в файле: C:\RFS-0.4.0\src\render\passes\transparent.rs
Код ошибки: E0433
Строки-локации: 168, 170, 179, 181
В чём заключается баг: cannot find `StoreOp` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№56
Баг в файле: C:\RFS-0.4.0\src\render\passes\transparent.rs
Код ошибки: E0433
Строки-локации: 171, 172, 182, 183, 192, 197
В чём заключается баг: cannot find `TextureLayout` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№57
Баг в файле: C:\RFS-0.4.0\src\render\passes\transparent.rs
Код ошибки: E0433
Строки-локации: 188
В чём заключается баг: cannot find `PipelineBindPoint` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№58
Баг в файле: C:\RFS-0.4.0\src\render\passes\water.rs
Код ошибки: E0433
Строки-локации: 249, 260
В чём заключается баг: cannot find `SampleCount` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№59
Баг в файле: C:\RFS-0.4.0\src\render\passes\water.rs
Код ошибки: E0433
Строки-локации: 250, 252, 261, 263
В чём заключается баг: cannot find `LoadOp` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№60
Баг в файле: C:\RFS-0.4.0\src\render\passes\water.rs
Код ошибки: E0433
Строки-локации: 251, 253, 262, 264
В чём заключается баг: cannot find `StoreOp` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№61
Баг в файле: C:\RFS-0.4.0\src\render\passes\water.rs
Код ошибки: E0433
Строки-локации: 254, 255, 265, 266, 275, 280
В чём заключается баг: cannot find `TextureLayout` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№62
Баг в файле: C:\RFS-0.4.0\src\render\passes\water.rs
Код ошибки: E0433
Строки-локации: 271
В чём заключается баг: cannot find `PipelineBindPoint` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№63
Баг в файле: C:\RFS-0.4.0\src\render\passes\water.rs
Код ошибки: E0407
Строки-локации: 387
В чём заключается баг: method `render_reflection` is not a member of trait `RenderPass`
Тип бага: Ошибка компиляции (E0407)
Статус: исправлен

Баг№64
Баг в файле: C:\RFS-0.4.0\src\render\passes\water.rs
Код ошибки: E0407
Строки-локации: 405
В чём заключается баг: method `render_refraction` is not a member of trait `RenderPass`
Тип бага: Ошибка компиляции (E0407)
Статус: исправлен

Баг№65
Баг в файле: C:\RFS-0.4.0\src\render\passes\water.rs
Код ошибки: E0407
Строки-локации: 421
В чём заключается баг: method `update_waves` is not a member of trait `RenderPass`
Тип бага: Ошибка компиляции (E0407)
Статус: исправлен

Баг№66
Баг в файле: C:\RFS-0.4.0\src\render\passes\ui.rs
Код ошибки: E0433
Строки-локации: 127
В чём заключается баг: cannot find `SampleCount` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№67
Баг в файле: C:\RFS-0.4.0\src\render\passes\ui.rs
Код ошибки: E0433
Строки-локации: 128, 130
В чём заключается баг: cannot find `LoadOp` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№68
Баг в файле: C:\RFS-0.4.0\src\render\passes\ui.rs
Код ошибки: E0433
Строки-локации: 129, 131
В чём заключается баг: cannot find `StoreOp` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№69
Баг в файле: C:\RFS-0.4.0\src\render\passes\ui.rs
Код ошибки: E0433
Строки-локации: 132, 133, 142
В чём заключается баг: cannot find `TextureLayout` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№70
Баг в файле: C:\RFS-0.4.0\src\render\passes\ui.rs
Код ошибки: E0433
Строки-локации: 138
В чём заключается баг: cannot find `PipelineBindPoint` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№71
Баг в файле: C:\RFS-0.4.0\src\render\passes\ui.rs
Код ошибки: E0407
Строки-локации: 236
В чём заключается баг: method `render_hud` is not a member of trait `RenderPass`
Тип бага: Ошибка компиляции (E0407)
Статус: исправлен

Баг№72
Баг в файле: C:\RFS-0.4.0\src\render\passes\ui.rs
Код ошибки: E0407
Строки-локации: 254
В чём заключается баг: method `render_menu` is not a member of trait `RenderPass`
Тип бага: Ошибка компиляции (E0407)
Статус: исправлен

Баг№73
Баг в файле: C:\RFS-0.4.0\src\render\passes\ui.rs
Код ошибки: E0407
Строки-локации: 270
В чём заключается баг: method `render_minimap` is not a member of trait `RenderPass`
Тип бага: Ошибка компиляции (E0407)
Статус: исправлен

Баг№74
Баг в файле: C:\RFS-0.4.0\src\render\materials\material.rs
Код ошибки: E0433
Строки-локации: 158
В чём заключается баг: cannot find `VertexInputDesc` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№75
Баг в файле: C:\RFS-0.4.0\src\render\materials\material.rs
Код ошибки: E0433
Строки-локации: 161, 163
В чём заключается баг: cannot find `PolygonMode` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№76
Баг в файле: C:\RFS-0.4.0\src\render\materials\material.rs
Код ошибки: E0433
Строки-локации: 170
В чём заключается баг: cannot find `FrontFace` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№77
Баг в файле: C:\RFS-0.4.0\src\render\materials\material.rs
Код ошибки: E0433
Строки-локации: 177
В чём заключается баг: cannot find `CompareOp` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№78
Баг в файле: C:\RFS-0.4.0\src\render\materials\material.rs
Код ошибки: E0433
Строки-локации: 181, 182, 183, 184, 185
В чём заключается баг: cannot find `BlendDesc` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№79
Баг в файле: C:\RFS-0.4.0\src\render\textures\texture.rs
Код ошибки: E0433
Строки-локации: 363, 364, 365, 366
В чём заключается баг: cannot find `AddressMode` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№80
Баг в файле: C:\RFS-0.4.0\src\render\textures\texture.rs
Код ошибки: E0433
Строки-локации: 370, 371, 372, 373, 374, 375, 376
В чём заключается баг: cannot find `FilterMode` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№81
Баг в файле: C:\RFS-0.4.0\src\render\graph\node.rs
Код ошибки: E0425
Строки-локации: 46, 118
В чём заключается баг: cannot find type `Device` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№82
Баг в файле: C:\RFS-0.4.0\src\render\graph\resource.rs
Код ошибки: E0425
Строки-локации: 121
В чём заключается баг: cannot find type `Device` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№83
Баг в файле: C:\RFS-0.4.0\src\render\passes\base.rs
Код ошибки: E0425
Строки-локации: 76
В чём заключается баг: cannot find type `TextureView` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№84
Баг в файле: C:\RFS-0.4.0\src\render\passes\base.rs
Код ошибки: E0425
Строки-локации: 84
В чём заключается баг: cannot find type `Texture` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№85
Баг в файле: C:\RFS-0.4.0\src\render\passes\base.rs
Код ошибки: E0425
Строки-локации: 92
В чём заключается баг: cannot find type `Buffer` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№86
Баг в файле: C:\RFS-0.4.0\src\render\passes\gbuffer.rs
Код ошибки: E0425
Строки-локации: 26, 27, 28, 29, 31
В чём заключается баг: cannot find type `Texture` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№87
Баг в файле: C:\RFS-0.4.0\src\render\passes\gbuffer.rs
Код ошибки: E0425
Строки-локации: 33
В чём заключается баг: cannot find type `RenderPass` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№88
Баг в файле: C:\RFS-0.4.0\src\render\passes\gbuffer.rs
Код ошибки: E0425
Строки-локации: 35
В чём заключается баг: cannot find type `Framebuffer` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№89
Баг в файле: C:\RFS-0.4.0\src\render\passes\gbuffer.rs
Код ошибки: E0425
Строки-локации: 76
В чём заключается баг: cannot find type `Device` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№90
Баг в файле: C:\RFS-0.4.0\src\render\passes\gbuffer.rs
Код ошибки: E0433
Строки-локации: 81, 82, 83, 84, 85
В чём заключается баг: cannot find `Format` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№91
Баг в файле: C:\RFS-0.4.0\src\render\passes\gbuffer.rs
Код ошибки: E0433
Строки-локации: 89, 89, 95, 95, 101, 101, 107, 107, 113, 113
В чём заключается баг: cannot find `TextureUsage` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№92
Баг в файле: C:\RFS-0.4.0\src\render\passes\gbuffer.rs
Код ошибки: E0422
Строки-локации: 246
В чём заключается баг: cannot find struct, variant or union type `Rect2D` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№93
Баг в файле: C:\RFS-0.4.0\src\render\passes\gbuffer.rs
Код ошибки: E0422
Строки-локации: 246
В чём заключается баг: cannot find struct, variant or union type `Offset2D` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№94
Баг в файле: C:\RFS-0.4.0\src\render\passes\gbuffer.rs
Код ошибки: E0422
Строки-локации: 246
В чём заключается баг: cannot find struct, variant or union type `Extent2D` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№95
Баг в файле: C:\RFS-0.4.0\src\render\passes\shadow.rs
Код ошибки: E0425
Строки-локации: 51
В чём заключается баг: cannot find type `RenderPass` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№96
Баг в файле: C:\RFS-0.4.0\src\render\passes\shadow.rs
Код ошибки: E0425
Строки-локации: 53
В чём заключается баг: cannot find type `Framebuffer` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№97
Баг в файле: C:\RFS-0.4.0\src\render\passes\shadow.rs
Код ошибки: E0425
Строки-локации: 101
В чём заключается баг: cannot find type `Device` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№98
Баг в файле: C:\RFS-0.4.0\src\render\passes\shadow.rs
Код ошибки: E0433
Строки-локации: 103
В чём заключается баг: cannot find `Format` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№99
Баг в файле: C:\RFS-0.4.0\src\render\passes\shadow.rs
Код ошибки: E0433
Строки-локации: 112, 112
В чём заключается баг: cannot find `TextureUsage` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№100
Баг в файле: C:\RFS-0.4.0\src\render\passes\shadow.rs
Код ошибки: E0422
Строки-локации: 124
В чём заключается баг: cannot find struct, variant or union type `AttachmentDescription` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№101
Баг в файле: C:\RFS-0.4.0\src\render\passes\shadow.rs
Код ошибки: E0422
Строки-локации: 135
В чём заключается баг: cannot find struct, variant or union type `SubpassDescription` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№102
Баг в файле: C:\RFS-0.4.0\src\render\passes\shadow.rs
Код ошибки: E0422
Строки-локации: 138
В чём заключается баг: cannot find struct, variant or union type `AttachmentReference` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№103
Баг в файле: C:\RFS-0.4.0\src\render\passes\shadow.rs
Код ошибки: E0422
Строки-локации: 145
В чём заключается баг: cannot find struct, variant or union type `RenderPassDesc` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№104
Баг в файле: C:\RFS-0.4.0\src\render\passes\shadow.rs
Код ошибки: E0422
Строки-локации: 193
В чём заключается баг: cannot find struct, variant or union type `Rect2D` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№105
Баг в файле: C:\RFS-0.4.0\src\render\passes\shadow.rs
Код ошибки: E0422
Строки-локации: 193
В чём заключается баг: cannot find struct, variant or union type `Offset2D` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№106
Баг в файле: C:\RFS-0.4.0\src\render\passes\shadow.rs
Код ошибки: E0422
Строки-локации: 193
В чём заключается баг: cannot find struct, variant or union type `Extent2D` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№107
Баг в файле: C:\RFS-0.4.0\src\render\passes\lighting.rs
Код ошибки: E0425
Строки-локации: 60, 63, 66
В чём заключается баг: cannot find type `Texture` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№108
Баг в файле: C:\RFS-0.4.0\src\render\passes\lighting.rs
Код ошибки: E0425
Строки-локации: 69
В чём заключается баг: cannot find type `RenderPass` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№109
Баг в файле: C:\RFS-0.4.0\src\render\passes\lighting.rs
Код ошибки: E0425
Строки-локации: 71
В чём заключается баг: cannot find type `Framebuffer` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№110
Баг в файле: C:\RFS-0.4.0\src\render\passes\lighting.rs
Код ошибки: E0425
Строки-локации: 138
В чём заключается баг: cannot find type `Device` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№111
Баг в файле: C:\RFS-0.4.0\src\render\passes\lighting.rs
Код ошибки: E0433
Строки-локации: 146, 146, 155, 155, 165, 165
В чём заключается баг: cannot find `TextureUsage` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№112
Баг в файле: C:\RFS-0.4.0\src\render\passes\lighting.rs
Код ошибки: E0422
Строки-локации: 174, 195, 213
В чём заключается баг: cannot find struct, variant or union type `AttachmentDescription` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№113
Баг в файле: C:\RFS-0.4.0\src\render\passes\lighting.rs
Код ошибки: E0422
Строки-локации: 187, 205, 223
В чём заключается баг: cannot find struct, variant or union type `AttachmentReference` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№114
Баг в файле: C:\RFS-0.4.0\src\render\passes\lighting.rs
Код ошибки: E0422
Строки-локации: 229
В чём заключается баг: cannot find struct, variant or union type `SubpassDescription` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№115
Баг в файле: C:\RFS-0.4.0\src\render\passes\lighting.rs
Код ошибки: E0422
Строки-локации: 236
В чём заключается баг: cannot find struct, variant or union type `RenderPassDesc` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№116
Баг в файле: C:\RFS-0.4.0\src\render\passes\lighting.rs
Код ошибки: E0422
Строки-локации: 307
В чём заключается баг: cannot find struct, variant or union type `Rect2D` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№117
Баг в файле: C:\RFS-0.4.0\src\render\passes\transparent.rs
Код ошибки: E0425
Строки-локации: 80, 83
В чём заключается баг: cannot find type `Texture` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№118
Баг в файле: C:\RFS-0.4.0\src\render\passes\transparent.rs
Код ошибки: E0425
Строки-локации: 86
В чём заключается баг: cannot find type `RenderPass` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№119
Баг в файле: C:\RFS-0.4.0\src\render\passes\transparent.rs
Код ошибки: E0425
Строки-локации: 88
В чём заключается баг: cannot find type `Framebuffer` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№120
Баг в файле: C:\RFS-0.4.0\src\render\passes\transparent.rs
Код ошибки: E0425
Строки-локации: 139
В чём заключается баг: cannot find type `Device` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№121
Баг в файле: C:\RFS-0.4.0\src\render\passes\transparent.rs
Код ошибки: E0433
Строки-локации: 144, 153
В чём заключается баг: cannot find `Format` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№122
Баг в файле: C:\RFS-0.4.0\src\render\passes\transparent.rs
Код ошибки: E0433
Строки-локации: 147, 147, 156, 156
В чём заключается баг: cannot find `TextureUsage` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№123
Баг в файле: C:\RFS-0.4.0\src\render\passes\transparent.rs
Код ошибки: E0422
Строки-локации: 164, 175
В чём заключается баг: cannot find struct, variant or union type `AttachmentDescription` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№124
Баг в файле: C:\RFS-0.4.0\src\render\passes\transparent.rs
Код ошибки: E0422
Строки-локации: 187
В чём заключается баг: cannot find struct, variant or union type `SubpassDescription` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№125
Баг в файле: C:\RFS-0.4.0\src\render\passes\transparent.rs
Код ошибки: E0422
Строки-локации: 190, 195
В чём заключается баг: cannot find struct, variant or union type `AttachmentReference` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№126
Баг в файле: C:\RFS-0.4.0\src\render\passes\transparent.rs
Код ошибки: E0422
Строки-локации: 202
В чём заключается баг: cannot find struct, variant or union type `RenderPassDesc` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№127
Баг в файле: C:\RFS-0.4.0\src\render\passes\transparent.rs
Код ошибки: E0422
Строки-локации: 288
В чём заключается баг: cannot find struct, variant or union type `Rect2D` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№128
Баг в файле: C:\RFS-0.4.0\src\render\passes\transparent.rs
Код ошибки: E0422
Строки-локации: 288
В чём заключается баг: cannot find struct, variant or union type `Offset2D` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№129
Баг в файле: C:\RFS-0.4.0\src\render\passes\transparent.rs
Код ошибки: E0422
Строки-локации: 288
В чём заключается баг: cannot find struct, variant or union type `Extent2D` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№130
Баг в файле: C:\RFS-0.4.0\src\render\passes\water.rs
Код ошибки: E0425
Строки-локации: 98, 101, 104, 107, 122
В чём заключается баг: cannot find type `Texture` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№131
Баг в файле: C:\RFS-0.4.0\src\render\passes\water.rs
Код ошибки: E0425
Строки-локации: 114, 118
В чём заключается баг: cannot find type `RenderPass` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№132
Баг в файле: C:\RFS-0.4.0\src\render\passes\water.rs
Код ошибки: E0425
Строки-локации: 116, 120
В чём заключается баг: cannot find type `Framebuffer` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№133
Баг в файле: C:\RFS-0.4.0\src\render\passes\water.rs
Код ошибки: E0425
Строки-локации: 189
В чём заключается баг: cannot find type `Device` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№134
Баг в файле: C:\RFS-0.4.0\src\render\passes\water.rs
Код ошибки: E0433
Строки-локации: 207, 217, 232, 248, 259, 295
В чём заключается баг: cannot find `Format` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№135
Баг в файле: C:\RFS-0.4.0\src\render\passes\water.rs
Код ошибки: E0433
Строки-локации: 208, 208, 218, 218, 233, 233, 296
В чём заключается баг: cannot find `TextureUsage` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№136
Баг в файле: C:\RFS-0.4.0\src\render\passes\water.rs
Код ошибки: E0422
Строки-локации: 247, 258
В чём заключается баг: cannot find struct, variant or union type `AttachmentDescription` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№137
Баг в файле: C:\RFS-0.4.0\src\render\passes\water.rs
Код ошибки: E0422
Строки-локации: 270
В чём заключается баг: cannot find struct, variant or union type `SubpassDescription` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№138
Баг в файле: C:\RFS-0.4.0\src\render\passes\water.rs
Код ошибки: E0422
Строки-локации: 273, 278
В чём заключается баг: cannot find struct, variant or union type `AttachmentReference` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№139
Баг в файле: C:\RFS-0.4.0\src\render\passes\water.rs
Код ошибки: E0422
Строки-локации: 285
В чём заключается баг: cannot find struct, variant or union type `RenderPassDesc` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№140
Баг в файле: C:\RFS-0.4.0\src\render\passes\water.rs
Код ошибки: E0422
Строки-локации: 351
В чём заключается баг: cannot find struct, variant or union type `Rect2D` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№141
Баг в файле: C:\RFS-0.4.0\src\render\passes\water.rs
Код ошибки: E0422
Строки-локации: 351
В чём заключается баг: cannot find struct, variant or union type `Offset2D` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№142
Баг в файле: C:\RFS-0.4.0\src\render\passes\water.rs
Код ошибки: E0422
Строки-локации: 351
В чём заключается баг: cannot find struct, variant or union type `Extent2D` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№143
Баг в файле: C:\RFS-0.4.0\src\render\passes\ui.rs
Код ошибки: E0425
Строки-локации: 48
В чём заключается баг: cannot find type `RenderPass` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№144
Баг в файле: C:\RFS-0.4.0\src\render\passes\ui.rs
Код ошибки: E0425
Строки-локации: 50
В чём заключается баг: cannot find type `Framebuffer` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№145
Баг в файле: C:\RFS-0.4.0\src\render\passes\ui.rs
Код ошибки: E0425
Строки-локации: 112
В чём заключается баг: cannot find type `Device` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№146
Баг в файле: C:\RFS-0.4.0\src\render\passes\ui.rs
Код ошибки: E0422
Строки-локации: 125
В чём заключается баг: cannot find struct, variant or union type `AttachmentDescription` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№147
Баг в файле: C:\RFS-0.4.0\src\render\passes\ui.rs
Код ошибки: E0422
Строки-локации: 137
В чём заключается баг: cannot find struct, variant or union type `SubpassDescription` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№148
Баг в файле: C:\RFS-0.4.0\src\render\passes\ui.rs
Код ошибки: E0422
Строки-локации: 140
В чём заключается баг: cannot find struct, variant or union type `AttachmentReference` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№149
Баг в файле: C:\RFS-0.4.0\src\render\passes\ui.rs
Код ошибки: E0422
Строки-локации: 149
В чём заключается баг: cannot find struct, variant or union type `RenderPassDesc` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№150
Баг в файле: C:\RFS-0.4.0\src\render\passes\ui.rs
Код ошибки: E0422
Строки-локации: 196
В чём заключается баг: cannot find struct, variant or union type `Rect2D` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№151
Баг в файле: C:\RFS-0.4.0\src\render\passes\ui.rs
Код ошибки: E0422
Строки-локации: 196
В чём заключается баг: cannot find struct, variant or union type `Offset2D` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№152
Баг в файле: C:\RFS-0.4.0\src\render\passes\ui.rs
Код ошибки: E0422
Строки-локации: 196
В чём заключается баг: cannot find struct, variant or union type `Extent2D` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№153
Баг в файле: C:\RFS-0.4.0\src\render\materials\material.rs
Код ошибки: E0422
Строки-локации: 156
В чём заключается баг: cannot find struct, variant or union type `PipelineDesc` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№154
Баг в файле: C:\RFS-0.4.0\src\render\materials\material.rs
Код ошибки: E0422
Строки-локации: 159
В чём заключается баг: cannot find struct, variant or union type `RasterizerDesc` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№155
Баг в файле: C:\RFS-0.4.0\src\render\materials\material.rs
Код ошибки: E0433
Строки-локации: 166, 167, 168
В чём заключается баг: cannot find `CullMode` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№156
Баг в файле: C:\RFS-0.4.0\src\render\materials\material.rs
Код ошибки: E0422
Строки-локации: 174
В чём заключается баг: cannot find struct, variant or union type `DepthStencilDesc` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№157
Баг в файле: C:\RFS-0.4.0\src\render\materials\material.rs
Код ошибки: E0425
Строки-локации: 245
В чём заключается баг: cannot find type `CommandEncoder` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№158
Баг в файле: C:\RFS-0.4.0\src\render\materials\pbr.rs
Код ошибки: E0425
Строки-локации: 165
В чём заключается баг: cannot find type `Device` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№159
Баг в файле: C:\RFS-0.4.0\src\render\materials\pbr.rs
Код ошибки: E0425
Строки-локации: 170
В чём заключается баг: cannot find type `CommandEncoder` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№160
Баг в файле: C:\RFS-0.4.0\src\render\materials\water.rs
Код ошибки: E0425
Строки-локации: 180
В чём заключается баг: cannot find type `Device` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№161
Баг в файле: C:\RFS-0.4.0\src\render\materials\water.rs
Код ошибки: E0425
Строки-локации: 185
В чём заключается баг: cannot find type `CommandEncoder` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№162
Баг в файле: C:\RFS-0.4.0\src\render\materials\library.rs
Код ошибки: E0425
Строки-локации: 22, 37
В чём заключается баг: cannot find type `Device` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№163
Баг в файле: C:\RFS-0.4.0\src\render\meshes\mesh.rs
Код ошибки: E0425
Строки-локации: 227, 246, 265
В чём заключается баг: cannot find type `Device` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№164
Баг в файле: C:\RFS-0.4.0\src\render\meshes\mesh.rs
Код ошибки: E0425
Строки-локации: 316
В чём заключается баг: cannot find type `CommandEncoder` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№165
Баг в файле: C:\RFS-0.4.0\src\render\meshes\loader.rs
Код ошибки: E0425
Строки-локации: 14, 26
В чём заключается баг: cannot find type `Device` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№166
Баг в файле: C:\RFS-0.4.0\src\render\meshes\library.rs
Код ошибки: E0425
Строки-локации: 31
В чём заключается баг: cannot find type `Device` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№167
Баг в файле: C:\RFS-0.4.0\src\render\textures\texture.rs
Код ошибки: E0425
Строки-локации: 189, 388
В чём заключается баг: cannot find type `Sampler` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№168
Баг в файле: C:\RFS-0.4.0\src\render\textures\texture.rs
Код ошибки: E0425
Строки-локации: 204, 245, 268, 289, 307, 361, 418, 487
В чём заключается баг: cannot find type `Device` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№169
Баг в файле: C:\RFS-0.4.0\src\render\textures\texture.rs
Код ошибки: E0422
Строки-локации: 336, 350
В чём заключается баг: cannot find struct, variant or union type `TextureViewDesc` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№170
Баг в файле: C:\RFS-0.4.0\src\render\textures\loader.rs
Код ошибки: E0425
Строки-локации: 14, 26, 48, 75, 101, 106, 111, 128
В чём заключается баг: cannot find type `Device` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№171
Баг в файле: C:\RFS-0.4.0\src\render\textures\library.rs
Код ошибки: E0425
Строки-локации: 31
В чём заключается баг: cannot find type `Device` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№172
Баг в файле: C:\RFS-0.4.0\src\render\lighting\shadows.rs
Код ошибки: E0425
Строки-локации: 77, 141, 156, 246
В чём заключается баг: cannot find type `Texture` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№173
Баг в файле: C:\RFS-0.4.0\src\render\lighting\shadows.rs
Код ошибки: E0425
Строки-локации: 79, 145, 158, 233, 248
В чём заключается баг: cannot find type `TextureView` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№174
Баг в файле: C:\RFS-0.4.0\src\render\lighting\shadows.rs
Код ошибки: E0425
Строки-локации: 166, 258
В чём заключается баг: cannot find type `Device` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№175
Баг в файле: C:\RFS-0.4.0\src\render\lighting\shadows.rs
Код ошибки: E0433
Строки-локации: 172, 263
В чём заключается баг: cannot find `Format` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№176
Баг в файле: C:\RFS-0.4.0\src\render\lighting\shadows.rs
Код ошибки: E0433
Строки-локации: 173, 173, 264, 264
В чём заключается баг: cannot find `TextureUsage` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№177
Баг в файле: C:\RFS-0.4.0\src\render\lighting\shadows.rs
Код ошибки: E0422
Строки-локации: 180
В чём заключается баг: cannot find struct, variant or union type `TextureViewDesc` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0422)
Статус: исправлен

Баг№178
Баг в файле: C:\RFS-0.4.0\src\render\lighting\probe.rs
Код ошибки: E0425
Строки-локации: 38
В чём заключается баг: cannot find type `Texture` in module `crate::rhi`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№179
Баг в файле: C:\RFS-0.4.0\src\render\postprocess\bloom.rs
Код ошибки: E0433
Строки-локации: 75, 75, 87, 87, 99, 99
В чём заключается баг: cannot find `TextureUsage` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№180
Баг в файле: C:\RFS-0.4.0\src\render\postprocess\motion_blur.rs
Код ошибки: E0433
Строки-локации: 55, 55, 62, 62
В чём заключается баг: cannot find `TextureUsage` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№181
Баг в файле: C:\RFS-0.4.0\src\render\postprocess\dof.rs
Код ошибки: E0433
Строки-локации: 55, 55, 62, 62
В чём заключается баг: cannot find `TextureUsage` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№182
Баг в файле: C:\RFS-0.4.0\src\render\postprocess\hdr.rs
Код ошибки: E0433
Строки-локации: 50, 50
В чём заключается баг: cannot find `TextureUsage` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№183
Баг в файле: C:\RFS-0.4.0\src\render\postprocess\fxaa.rs
Код ошибки: E0433
Строки-локации: 41, 41
В чём заключается баг: cannot find `TextureUsage` in `rhi`
Тип бага: Ошибка компиляции (E0433)
Статус: исправлен

Баг№184
Баг в файле: C:\RFS-0.4.0\src\render\particles\effect_manager.rs
Код ошибки: E0425
Строки-локации: 86
В чём заключается баг: cannot find type `SmokeEffectConfig` in module `super::effects`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№185
Баг в файле: C:\RFS-0.4.0\src\render\particles\effect_manager.rs
Код ошибки: E0425
Строки-локации: 96
В чём заключается баг: cannot find type `FireEffectConfig` in module `super::effects`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№186
Баг в файле: C:\RFS-0.4.0\src\render\particles\effect_manager.rs
Код ошибки: E0425
Строки-локации: 106
В чём заключается баг: cannot find type `SplashEffectConfig` in module `super::effects`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№187
Баг в файле: C:\RFS-0.4.0\src\render\particles\effect_manager.rs
Код ошибки: E0425
Строки-локации: 116
В чём заключается баг: cannot find type `ExplosionEffectConfig` in module `super::effects`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№188
Баг в файле: C:\RFS-0.4.0\src\render\particles\effect_manager.rs
Код ошибки: E0425
Строки-локации: 126
В чём заключается баг: cannot find type `SparksEffectConfig` in module `super::effects`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№189
Баг в файле: C:\RFS-0.4.0\src\render\particles\effect_manager.rs
Код ошибки: E0425
Строки-локации: 136
В чём заключается баг: cannot find type `DustEffectConfig` in module `super::effects`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№190
Баг в файле: C:\RFS-0.4.0\src\render\particles\effect_manager.rs
Код ошибки: E0425
Строки-локации: 146
В чём заключается баг: cannot find type `BloodEffectConfig` in module `super::effects`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№191
Баг в файле: C:\RFS-0.4.0\src\render\particles\effect_manager.rs
Код ошибки: E0425
Строки-локации: 156
В чём заключается баг: cannot find type `MagicEffectConfig` in module `super::effects`
Тип бага: Ошибка компиляции (E0425)
Статус: исправлен

Баг№192
Баг в файле: C:\RFS-0.4.0\src\render\postprocess\mod.rs
Код ошибки: E0603
Строки-локации: 20
В чём заключается баг: struct import `DepthOfFieldConfig` is private
Тип бага: Ошибка компиляции (E0603)
Статус: исправлен

Баг№193
Баг в файле: C:\RFS-0.4.0\src\render\postprocess\mod.rs
Код ошибки: E0603
Строки-локации: 21
В чём заключается баг: struct import `HDRConfig` is private
Тип бага: Ошибка компиляции (E0603)
Статус: исправлен

Баг№194
Баг в файле: C:\RFS-0.4.0\src\render\passes\gbuffer.rs
Код ошибки: E0603
Строки-локации: 18
В чём заключается баг: enum import `ResourceUsage` is private
Тип бага: Ошибка компиляции (E0603)
Статус: исправлен

Баг№195
Баг в файле: C:\RFS-0.4.0\src\render\passes\shadow.rs
Код ошибки: E0603
Строки-локации: 16
В чём заключается баг: enum import `ResourceUsage` is private
Тип бага: Ошибка компиляции (E0603)
Статус: исправлен

Баг№196
Баг в файле: C:\RFS-0.4.0\src\render\passes\lighting.rs
Код ошибки: E0603
Строки-локации: 18
В чём заключается баг: enum import `ResourceUsage` is private
Тип бага: Ошибка компиляции (E0603)
Статус: исправлен

Баг№197
Баг в файле: C:\RFS-0.4.0\src\render\passes\transparent.rs
Код ошибки: E0603
Строки-локации: 14
В чём заключается баг: enum import `ResourceUsage` is private
Тип бага: Ошибка компиляции (E0603)
Статус: исправлен

Баг№198
Баг в файле: C:\RFS-0.4.0\src\render\passes\water.rs
Код ошибки: E0603
Строки-локации: 17
В чём заключается баг: enum import `ResourceUsage` is private
Тип бага: Ошибка компиляции (E0603)
Статус: исправлен

Баг№199
Баг в файле: C:\RFS-0.4.0\src\render\passes\ui.rs
Код ошибки: E0603
Строки-локации: 13
В чём заключается баг: enum import `ResourceUsage` is private
Тип бага: Ошибка компиляции (E0603)
Статус: исправлен

Баг№200
Баг в файле: C:\RFS-0.4.0\src\render\passes\base.rs
Код ошибки: E0603
Строки-локации: 31
В чём заключается баг: enum import `ResourceUsage` is private
Тип бага: Ошибка компиляции (E0603)
Статус: исправлен

Баг№201
Баг в файле: C:\RFS-0.4.0\src\render\core\settings.rs
Код ошибки: E0119
Строки-локации: 640
В чём заключается баг: conflicting implementations of trait `Debug` for type `settings::RayTracingQuality`
Тип бага: Ошибка компиляции (E0119)
Статус: исправлен

Баг№202
Баг в файле: C:\RFS-0.4.0\src\render\core\settings.rs
Код ошибки: E0119
Строки-локации: 640
В чём заключается баг: conflicting implementations of trait `Clone` for type `settings::RayTracingQuality`
Тип бага: Ошибка компиляции (E0119)
Статус: исправлен

Баг№203
Баг в файле: C:\RFS-0.4.0\src\render\core\settings.rs
Код ошибки: E0119
Строки-локации: 640
В чём заключается баг: conflicting implementations of trait `Copy` for type `settings::RayTracingQuality`
Тип бага: Ошибка компиляции (E0119)
Статус: исправлен

Баг№204
Баг в файле: C:\RFS-0.4.0\src\render\core\settings.rs
Код ошибки: E0119
Строки-локации: 640
В чём заключается баг: conflicting implementations of trait `StructuralPartialEq` for type `settings::RayTracingQuality`
Тип бага: Ошибка компиляции (E0119)
Статус: исправлен

Баг№205
Баг в файле: C:\RFS-0.4.0\src\render\core\settings.rs
Код ошибки: E0119
Строки-локации: 640
В чём заключается баг: conflicting implementations of trait `PartialEq` for type `settings::RayTracingQuality`
Тип бага: Ошибка компиляции (E0119)
Статус: исправлен

Баг№206
Баг в файле: C:\RFS-0.4.0\src\render\core\settings.rs
Код ошибки: E0119
Строки-локации: 640
В чём заключается баг: conflicting implementations of trait `Eq` for type `settings::RayTracingQuality`
Тип бага: Ошибка компиляции (E0119)
Статус: исправлен

---
Итого групп: 206
---
# ВАРНИНГИ компиляции клиента RFS (rfs-client)

Все предупреждения из cargo check, 1320 исходно, 368 групп.

Варн№207
Варнинг в файле: C:\RFS-0.4.0\src\rhi\error.rs
Строки-локации: 6
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

Варн№208
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 6
В чём заключается варнинг: unused import: `bitflags::bitflags`
Тип: Предупреждение компиляции

Варн№209
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 12
В чём заключается варнинг: variant `R8_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№210
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 12
В чём заключается варнинг: variant `R8_SNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№211
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 12
В чём заключается варнинг: variant `R8_UINT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№212
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 12
В чём заключается варнинг: variant `R8_SINT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№213
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 14
В чём заключается варнинг: variant `R16_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№214
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 14
В чём заключается варнинг: variant `R16_SNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№215
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 14
В чём заключается варнинг: variant `R16_UINT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№216
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 14
В чём заключается варнинг: variant `R16_SINT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№217
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 14
В чём заключается варнинг: variant `R16_SFLOAT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№218
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 15
В чём заключается варнинг: variant `RG8_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№219
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 15
В чём заключается варнинг: variant `RG8_SNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№220
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 15
В чём заключается варнинг: variant `RG8_UINT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№221
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 15
В чём заключается варнинг: variant `RG8_SINT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№222
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 17
В чём заключается варнинг: variant `R32_UINT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№223
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 17
В чём заключается варнинг: variant `R32_SINT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№224
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 17
В чём заключается варнинг: variant `R32_SFLOAT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№225
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 18
В чём заключается варнинг: variant `RG16_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№226
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 18
В чём заключается варнинг: variant `RG16_SNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№227
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 18
В чём заключается варнинг: variant `RG16_UINT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№228
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 18
В чём заключается варнинг: variant `RG16_SINT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№229
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 18
В чём заключается варнинг: variant `RG16_SFLOAT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№230
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 19
В чём заключается варнинг: variant `RGBA8_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№231
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 19
В чём заключается варнинг: variant `RGBA8_SNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№232
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 19
В чём заключается варнинг: variant `RGBA8_UINT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№233
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 19
В чём заключается варнинг: variant `RGBA8_SINT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№234
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 21
В чём заключается варнинг: variant `R32G32_UINT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№235
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 21
В чём заключается варнинг: variant `R32G32_SINT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№236
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 21
В чём заключается варнинг: variant `R32G32_SFLOAT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№237
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 23
В чём заключается варнинг: variant `R32G32B32_UINT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№238
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 23
В чём заключается варнинг: variant `R32G32B32_SINT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№239
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 23
В чём заключается варнинг: variant `R32G32B32_SFLOAT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№240
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 25
В чём заключается варнинг: variant `RGBA32_UINT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№241
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 25
В чём заключается варнинг: variant `RGBA32_SINT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№242
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 25
В чём заключается варнинг: variant `RGBA32_SFLOAT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№243
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 27
В чём заключается варнинг: variant `D16_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№244
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 27
В чём заключается варнинг: variant `D24_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№245
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 27
В чём заключается варнинг: variant `D32_SFLOAT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№246
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 28
В чём заключается варнинг: variant `D24_UNORM_S8_UINT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№247
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 28
В чём заключается варнинг: variant `D32_SFLOAT_S8_UINT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№248
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 28
В чём заключается варнинг: variant `S8_UINT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№249
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 30
В чём заключается варнинг: variant `BC1_RGB_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№250
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 30
В чём заключается варнинг: variant `BC1_RGB_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№251
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 30
В чём заключается варнинг: variant `BC1_RGBA_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№252
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 30
В чём заключается варнинг: variant `BC1_RGBA_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№253
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 31
В чём заключается варнинг: variant `BC2_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№254
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 31
В чём заключается варнинг: variant `BC2_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№255
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 31
В чём заключается варнинг: variant `BC3_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№256
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 31
В чём заключается варнинг: variant `BC3_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№257
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 32
В чём заключается варнинг: variant `BC4_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№258
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 32
В чём заключается варнинг: variant `BC4_SNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№259
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 32
В чём заключается варнинг: variant `BC5_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№260
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 32
В чём заключается варнинг: variant `BC5_SNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№261
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 33
В чём заключается варнинг: variant `BC6H_UFLOAT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№262
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 33
В чём заключается варнинг: variant `BC6H_SFLOAT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№263
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 33
В чём заключается варнинг: variant `BC7_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№264
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 33
В чём заключается варнинг: variant `BC7_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№265
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 35
В чём заключается варнинг: variant `ASTC_4x4_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№266
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 35
В чём заключается варнинг: variant `ASTC_4x4_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№267
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 36
В чём заключается варнинг: variant `ASTC_5x4_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№268
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 36
В чём заключается варнинг: variant `ASTC_5x4_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№269
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 37
В чём заключается варнинг: variant `ASTC_5x5_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№270
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 37
В чём заключается варнинг: variant `ASTC_5x5_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№271
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 38
В чём заключается варнинг: variant `ASTC_6x5_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№272
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 38
В чём заключается варнинг: variant `ASTC_6x5_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№273
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 39
В чём заключается варнинг: variant `ASTC_6x6_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№274
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 39
В чём заключается варнинг: variant `ASTC_6x6_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№275
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 40
В чём заключается варнинг: variant `ASTC_8x5_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№276
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 40
В чём заключается варнинг: variant `ASTC_8x5_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№277
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 41
В чём заключается варнинг: variant `ASTC_8x6_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№278
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 41
В чём заключается варнинг: variant `ASTC_8x6_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№279
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 42
В чём заключается варнинг: variant `ASTC_8x8_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№280
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 42
В чём заключается варнинг: variant `ASTC_8x8_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№281
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 44
В чём заключается варнинг: variant `ETC2_R8G8B8_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№282
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 44
В чём заключается варнинг: variant `ETC2_R8G8B8_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№283
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 45
В чём заключается варнинг: variant `ETC2_R8G8B8A1_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№284
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 45
В чём заключается варнинг: variant `ETC2_R8G8B8A1_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№285
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 46
В чём заключается варнинг: variant `ETC2_R8G8B8A8_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№286
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 46
В чём заключается варнинг: variant `ETC2_R8G8B8A8_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№287
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 47
В чём заключается варнинг: variant `EAC_R11_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№288
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 47
В чём заключается варнинг: variant `EAC_R11_SNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№289
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 48
В чём заключается варнинг: variant `EAC_R11G11_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№290
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 48
В чём заключается варнинг: variant `EAC_R11G11_SNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№291
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 50
В чём заключается варнинг: variant `B10G11R11_UFLOAT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№292
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 50
В чём заключается варнинг: variant `E5B9G9R9_UFLOAT` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№293
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 52
В чём заключается варнинг: variant `A8_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

Варн№294
Варнинг в файле: C:\RFS-0.4.0\src\rhi\core\device.rs
Строки-локации: 5
В чём заключается варнинг: unused imports: `Arc` and `RwLock`
Тип: Предупреждение компиляции

Варн№295
Варнинг в файле: C:\RFS-0.4.0\src\rhi\backend\common.rs
Строки-локации: 5
В чём заключается варнинг: unused import: `super::Backend`
Тип: Предупреждение компиляции

Варн№296
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\buffer.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::error::*`
Тип: Предупреждение компиляции

Варн№297
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\texture.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::error::*`
Тип: Предупреждение компиляции

Варн№298
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\sampler.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

Варн№299
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\blas.rs
Строки-локации: 18, 37
В чём заключается варнинг: unused doc comment
Тип: Предупреждение компиляции

Варн№300
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\tlas.rs
Строки-локации: 30
В чём заключается варнинг: unused doc comment
Тип: Предупреждение компиляции

Варн№301
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\tlas.rs
Строки-локации: 6
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

Варн№302
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\build.rs
Строки-локации: 6
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

Варн№303
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\build.rs
Строки-локации: 8
В чём заключается варнинг: unused import: `AccelerationStructureFlags`
Тип: Предупреждение компиляции

Варн№304
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\query.rs
Строки-локации: 6
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

Варн№305
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\compute.rs
Строки-локации: 6
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

Варн№306
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\compute.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::shader::ShaderModule`
Тип: Предупреждение компиляции

Варн№307
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\ray_tracing.rs
Строки-локации: 6
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

Варн№308
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\mesh_shading.rs
Строки-локации: 6
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

Варн№309
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\mesh_shading.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::shader::ShaderModule`
Тип: Предупреждение компиляции

Варн№310
Варнинг в файле: C:\RFS-0.4.0\src\rhi\shader\module.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

Варн№311
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\buffer.rs
Строки-локации: 28
В чём заключается варнинг: unused doc comment
Тип: Предупреждение компиляции

Варн№312
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\buffer.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

Варн№313
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pool.rs
Строки-локации: 9
В чём заключается варнинг: unused doc comment
Тип: Предупреждение компиляции

Варн№314
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pool.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

Варн№315
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\encoder.rs
Строки-локации: 6
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

Варн№316
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pass\render.rs
Строки-локации: 55, 75
В чём заключается варнинг: unused doc comment
Тип: Предупреждение компиляции

Варн№317
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\commands.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `super::buffer::CommandBuffer`
Тип: Предупреждение компиляции

Варн№318
Варнинг в файле: C:\RFS-0.4.0\src\rhi\sync\semaphore.rs
Строки-локации: 9
В чём заключается варнинг: unused doc comment
Тип: Предупреждение компиляции

Варн№319
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\layout.rs
Строки-локации: 43
В чём заключается варнинг: unused doc comment
Тип: Предупреждение компиляции

Варн№320
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\set.rs
Строки-локации: 6
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

Варн№321
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\allocator.rs
Строки-локации: 6
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

Варн№322
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\binding.rs
Строки-локации: 6
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

Варн№323
Варнинг в файле: C:\RFS-0.4.0\src\rhi\query\pool.rs
Строки-локации: 20
В чём заключается варнинг: unused doc comment
Тип: Предупреждение компиляции

Варн№324
Варнинг в файле: C:\RFS-0.4.0\src\rhi\query\pool.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

Варн№325
Варнинг в файле: C:\RFS-0.4.0\src\rhi\query\results.rs
Строки-локации: 6
В чём заключается варнинг: unused import: `super::pool::QueryType`
Тип: Предупреждение компиляции

Варн№326
Варнинг в файле: C:\RFS-0.4.0\src\rhi\debug\capture.rs
Строки-локации: 6
В чём заключается варнинг: unused import: `std::collections::HashMap`
Тип: Предупреждение компиляции

Варн№327
Варнинг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Строки-локации: 45, 46, 50, 53, 53, 53, 54
В чём заключается варнинг: ambiguous glob re-exports
Тип: Предупреждение компиляции

Варн№328
Варнинг в файле: C:\RFS-0.4.0\src\rhi\core\caps.rs
Строки-локации: 15
В чём заключается варнинг: unused variable: `required`
Тип: Предупреждение компиляции

Варн№329
Варнинг в файле: C:\RFS-0.4.0\src\rhi\backend\mod.rs:53:9 ; --> src\rhi\backend\mod.rs
Строки-локации: 53
В чём заключается варнинг: unreachable pattern
Тип: Предупреждение компиляции

Варн№330
Варнинг в файле: C:\RFS-0.4.0\src\rhi\sync\fence.rs
Строки-локации: 26
В чём заключается варнинг: unused variable: `timeout`
Тип: Предупреждение компиляции

Варн№331
Варнинг в файле: C:\RFS-0.4.0\src\rhi\sync\semaphore.rs
Строки-локации: 50, 54
В чём заключается варнинг: unused variable: `value`
Тип: Предупреждение компиляции

Варн№332
Варнинг в файле: C:\RFS-0.4.0\src\rhi\sync\semaphore.rs
Строки-локации: 54
В чём заключается варнинг: unused variable: `timeout`
Тип: Предупреждение компиляции

Варн№333
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\buddy.rs
Строки-локации: 37
В чём заключается варнинг: unused variable: `desc`
Тип: Предупреждение компиляции

Варн№334
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\buddy.rs
Строки-локации: 41
В чём заключается варнинг: unused variable: `allocation`
Тип: Предупреждение компиляции

Варн№335
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\linear.rs
Строки-локации: 31
В чём заключается варнинг: unused variable: `desc`
Тип: Предупреждение компиляции

Варн№336
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\pool.rs
Строки-локации: 25
В чём заключается варнинг: unused variable: `initial_blocks`
Тип: Предупреждение компиляции

Варн№337
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\pool.rs
Строки-локации: 35
В чём заключается варнинг: unused variable: `desc`
Тип: Предупреждение компиляции

Варн№338
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\pool.rs
Строки-локации: 39
В чём заключается варнинг: unused variable: `allocation`
Тип: Предупреждение компиляции

Варн№339
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\allocator.rs
Строки-локации: 47
В чём заключается варнинг: unused variable: `layout`
Тип: Предупреждение компиляции

Варн№340
Варнинг в файле: C:\RFS-0.4.0\src\rhi\swapchain\swapchain.rs
Строки-локации: 110
В чём заключается варнинг: unused variable: `image_index`
Тип: Предупреждение компиляции

Варн№341
Варнинг в файле: C:\RFS-0.4.0\src\rhi\config\settings.rs:58:5 ; --> src\rhi\lib.rs
Строки-локации: 22
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№342
Варнинг в файле: C:\RFS-0.4.0\src\rhi\config\settings.rs
Строки-локации: 59, 60, 61, 62, 80, 81, 82, 83
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№343
Варнинг в файле: C:\RFS-0.4.0\src\rhi\config\settings.rs
Строки-локации: 100, 101, 102, 103
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

Варн№344
Варнинг в файле: C:\RFS-0.4.0\src\rhi\error.rs
Строки-локации: 16, 19, 23, 26, 29, 32, 36, 39, 42, 46, 49, 53, 56, 60, 63, 66, 70, 74, 78, 82, 85
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

Варн№345
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 12, 12, 12, 12, 14, 14, 14, 14, 14, 15, 15, 15, 15, 17, 17, 17, 18, 18, 18, 18, 18, 19, 19, 19, 19, 21, 21, 21, 23, 23, 23, 25, 25, 25, 27, 27, 27, 28, 28, 28, 30, 30, 30, 30, 31, 31, 31, 31, 32, 32, 32, 32, 33, 33, 33, 33, 35, 35, 36, 36, 37, 37, 38, 38, 39, 39, 40, 40, 41, 41, 42, 42, 44, 44, 45, 45, 46, 46, 47, 47, 48, 48, 50, 50, 52, 102, 102, 102, 102, 102, 102, 102, 172, 173, 185, 186, 186, 187, 187, 187, 193, 193, 200, 200, 200, 200, 207, 208, 214, 215, 216, 217
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

Варн№346
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 62, 70, 77, 106
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№347
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 118, 119, 129, 130, 131, 137, 138, 144, 145, 146, 152, 153, 159, 160, 161, 162, 163, 164, 172, 172, 172, 172, 173, 173
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№348
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 123, 177
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№349
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 167
В чём заключается варнинг: missing documentation for a type alias
Тип: Предупреждение компиляции

Варн№350
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\flags.rs
Строки-локации: 8, 20, 34, 46, 55, 70, 86, 107, 117, 124, 132, 141
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

Варн№351
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\flags.rs
Строки-локации: 8, 20, 34, 46, 55, 70, 86, 107, 117, 124, 132, 141
В чём заключается варнинг: missing documentation for an associated constant
Тип: Предупреждение компиляции

Варн№352
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\features.rs
Строки-локации: 8
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

Варн№353
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\features.rs
Строки-локации: 10, 11, 12, 13, 14, 17, 20, 21, 24, 25, 26, 27, 30, 31, 34, 35, 38, 39, 42, 45, 48, 51, 54, 57
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№354
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\limits.rs
Строки-локации: 9
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

Варн№355
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\limits.rs
Строки-локации: 11, 12, 13, 16, 17, 18, 21, 24, 25, 28, 29, 32, 35, 36
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№356
Варнинг в файле: C:\RFS-0.4.0\src\rhi\core\device.rs
Строки-локации: 12, 13, 14, 15
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

Варн№357
Варнинг в файле: C:\RFS-0.4.0\src\rhi\core\device.rs
Строки-локации: 21, 22, 28, 29, 35, 36, 42, 43, 44, 45, 46, 52, 53, 54, 55, 56, 57, 58, 59, 71, 72, 73, 85, 86, 87
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№358
Варнинг в файле: C:\RFS-0.4.0\src\rhi\core\device.rs
Строки-локации: 63, 77, 78, 79, 97, 98, 99, 102
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№359
Варнинг в файле: C:\RFS-0.4.0\src\rhi\core\caps.rs
Строки-локации: 8
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

Варн№360
Варнинг в файле: C:\RFS-0.4.0\src\rhi\core\caps.rs
Строки-локации: 9, 10, 11
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№361
Варнинг в файле: C:\RFS-0.4.0\src\rhi\core\caps.rs
Строки-локации: 15
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№362
Варнинг в файле: C:\RFS-0.4.0\src\rhi\backend\mod.rs
Строки-локации: 9, 10, 11, 12, 13
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№363
Варнинг в файле: C:\RFS-0.4.0\src\rhi\backend\common.rs
Строки-локации: 10, 11
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№364
Варнинг в файле: C:\RFS-0.4.0\src\rhi\backend\vulkan\mod.rs
Строки-локации: 8
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

Варн№365
Варнинг в файле: C:\RFS-0.4.0\src\rhi\backend\vulkan\mod.rs
Строки-локации: 11
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№366
Варнинг в файле: C:\RFS-0.4.0\src\rhi\backend\d3d12\mod.rs
Строки-локации: 8
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

Варн№367
Варнинг в файле: C:\RFS-0.4.0\src\rhi\backend\d3d12\mod.rs
Строки-локации: 11
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№368
Варнинг в файле: C:\RFS-0.4.0\src\rhi\backend\d3d11\mod.rs
Строки-локации: 8
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

Варн№369
Варнинг в файле: C:\RFS-0.4.0\src\rhi\backend\d3d11\mod.rs
Строки-локации: 11
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№370
Варнинг в файле: C:\RFS-0.4.0\src\rhi\backend\opengl\mod.rs
Строки-локации: 8
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

Варн№371
Варнинг в файле: C:\RFS-0.4.0\src\rhi\backend\opengl\mod.rs
Строки-локации: 11
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№372
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\buffer.rs
Строки-локации: 12, 13, 14, 15, 16
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№373
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\buffer.rs
Строки-локации: 27
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№374
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\buffer.rs
Строки-локации: 34, 35
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№375
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\texture.rs
Строки-локации: 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 45, 46, 47, 48, 49, 50, 51, 52
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№376
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\texture.rs
Строки-локации: 32
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№377
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\texture.rs
Строки-локации: 36, 37, 38, 39
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№378
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\texture.rs
Строки-локации: 59, 59, 59, 59, 59, 59, 59, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

Варн№379
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\sampler.rs
Строки-локации: 13, 14, 21, 22, 23, 24, 25, 32, 33, 34, 35, 36, 37, 38, 39, 46, 47, 48, 49, 50, 51
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

Варн№380
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\sampler.rs
Строки-локации: 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№381
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\sampler.rs
Строки-локации: 80
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№382
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\sampler.rs
Строки-локации: 84
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№383
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\blas.rs
Строки-локации: 12, 13, 49, 50, 51, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 80, 81, 82, 83, 89, 90, 91
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№384
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\blas.rs
Строки-локации: 19, 38
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

Варн№385
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\blas.rs
Строки-локации: 19, 38
В чём заключается варнинг: missing documentation for an associated constant
Тип: Предупреждение компиляции

Варн№386
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\blas.rs
Строки-локации: 32, 33, 34, 57, 58, 59
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

Варн№387
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\blas.rs
Строки-локации: 101
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№388
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\tlas.rs
Строки-локации: 12, 13, 14, 20, 21, 22, 23, 24, 25
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№389
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\tlas.rs
Строки-локации: 31
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

Варн№390
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\tlas.rs
Строки-локации: 31
В чём заключается варнинг: missing documentation for an associated constant
Тип: Предупреждение компиляции

Варн№391
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\tlas.rs
Строки-локации: 49
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№392
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\build.rs
Строки-локации: 14, 15, 30, 31, 37, 38
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

Варн№393
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\build.rs
Строки-локации: 21, 22, 23, 24, 44, 45, 46, 47
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№394
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\query.rs
Строки-локации: 11, 12, 13
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

Варн№395
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\graphics.rs
Строки-локации: 13, 14, 15, 16, 22, 23, 24, 37, 38, 56, 57, 90, 91, 92, 93, 94, 95, 96, 97, 98, 99, 119, 120, 121, 122, 123, 124, 125, 131, 132, 133, 134, 135, 136, 137, 138, 139, 173, 174, 175, 176, 177, 178, 179, 180, 208, 209, 210, 211, 217, 218, 224, 225, 226, 227, 233, 234, 235, 241, 242, 243, 244, 245, 246, 247, 248
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№396
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\graphics.rs
Строки-локации: 30, 31, 45, 46, 47, 48, 49, 50, 64, 65, 66, 73, 74, 75, 76, 83, 84, 106, 107, 108, 109, 110, 111, 112, 113, 146, 147, 148, 149, 150, 151, 152, 153, 154, 155, 156, 163, 164, 165, 166, 167, 187, 188, 189, 190, 191, 192, 193, 194, 195, 196, 197, 198, 199, 200, 201, 202
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

Варн№397
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\graphics.rs
Строки-локации: 258
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№398
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\graphics.rs
Строки-локации: 262
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№399
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\compute.rs
Строки-локации: 13
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№400
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\compute.rs
Строки-локации: 23
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№401
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\compute.rs
Строки-локации: 27
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№402
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\ray_tracing.rs
Строки-локации: 13, 14, 15, 16, 17, 18
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

Варн№403
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\ray_tracing.rs
Строки-локации: 24, 25, 31, 32, 38, 39, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 74, 75, 76, 77
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№404
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\ray_tracing.rs
Строки-локации: 48
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№405
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\ray_tracing.rs
Строки-локации: 52
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№406
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\mesh_shading.rs
Строки-локации: 13, 14, 15
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№407
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\mesh_shading.rs
Строки-локации: 24
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№408
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\mesh_shading.rs
Строки-локации: 28
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№409
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\state.rs
Строки-локации: 12, 13, 19, 20, 21
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№410
Варнинг в файле: C:\RFS-0.4.0\src\rhi\shader\module.rs
Строки-локации: 13, 14, 15, 16, 17, 18
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

Варн№411
Варнинг в файле: C:\RFS-0.4.0\src\rhi\shader\module.rs
Строки-локации: 24, 25, 26, 27
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№412
Варнинг в файле: C:\RFS-0.4.0\src\rhi\shader\module.rs
Строки-локации: 37
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№413
Варнинг в файле: C:\RFS-0.4.0\src\rhi\shader\module.rs
Строки-локации: 41
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№414
Варнинг в файле: C:\RFS-0.4.0\src\rhi\shader\reflection.rs
Строки-локации: 11, 12, 13, 14, 40, 41, 47, 48, 49, 50, 56, 57, 58, 59, 60, 85, 86, 87, 88, 89, 90, 91, 97, 98, 99, 100
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№415
Варнинг в файле: C:\RFS-0.4.0\src\rhi\shader\reflection.rs
Строки-локации: 20, 21, 22, 23, 24, 30, 31, 32, 33, 34, 66, 67, 68, 74, 75, 76, 77, 78, 79
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

Варн№416
Варнинг в файле: C:\RFS-0.4.0\src\rhi\shader\compiler.rs
Строки-локации: 12, 13, 14, 15, 16
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№417
Варнинг в файле: C:\RFS-0.4.0\src\rhi\shader\compiler.rs
Строки-локации: 22, 23, 24, 25
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№418
Варнинг в файле: C:\RFS-0.4.0\src\rhi\shader\compiler.rs
Строки-локации: 32, 33, 34, 41, 42, 43
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

Варн№419
Варнинг в файле: C:\RFS-0.4.0\src\rhi\shader\stage.rs
Строки-локации: 9, 10, 11, 12, 13, 14, 15
В чём заключается варнинг: missing documentation for a constant
Тип: Предупреждение компиляции

Варн№420
Варнинг в файле: C:\RFS-0.4.0\src\rhi\shader\library.rs
Строки-локации: 11, 12
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№421
Варнинг в файле: C:\RFS-0.4.0\src\rhi\shader\library.rs
Строки-локации: 22
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№422
Варнинг в файле: C:\RFS-0.4.0\src\rhi\shader\library.rs
Строки-локации: 29, 33
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№423
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\buffer.rs
Строки-локации: 14, 15, 21, 22, 23, 24, 25
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

Варн№424
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\buffer.rs
Строки-локации: 29
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

Варн№425
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\buffer.rs
Строки-локации: 29
В чём заключается варнинг: missing documentation for an associated constant
Тип: Предупреждение компиляции

Варн№426
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\buffer.rs
Строки-локации: 42, 43, 44
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№427
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\buffer.rs
Строки-локации: 55
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№428
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\buffer.rs
Строки-локации: 63, 64, 65, 67, 71
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№429
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pool.rs
Строки-локации: 10
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

Варн№430
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pool.rs
Строки-локации: 10
В чём заключается варнинг: missing documentation for an associated constant
Тип: Предупреждение компиляции

Варн№431
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pool.rs
Строки-локации: 22, 23
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№432
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pool.rs
Строки-локации: 33
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№433
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pool.rs
Строки-локации: 40, 41
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№434
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\encoder.rs
Строки-локации: 15, 28, 39, 50
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№435
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\encoder.rs
Строки-локации: 19
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№436
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pass\render.rs
Строки-локации: 14, 15, 16, 23, 24, 51, 52
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

Варн№437
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pass\render.rs
Строки-локации: 30, 31, 32, 33, 34, 35, 36, 37, 43, 44, 66, 67, 68, 69, 70, 71, 72, 88, 89, 90, 91, 92, 93, 94, 100, 101, 102, 122, 123, 124, 125, 126, 132, 133, 134, 154, 155, 156, 157
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№438
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pass\render.rs
Строки-локации: 56, 76
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

Варн№439
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pass\render.rs
Строки-локации: 56, 76
В чём заключается варнинг: missing documentation for an associated constant
Тип: Предупреждение компиляции

Варн№440
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pass\render.rs
Строки-локации: 112, 146
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№441
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pass\render.rs
Строки-локации: 116
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№442
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pass\compute.rs
Строки-локации: 13
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№443
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pass\ray_tracing.rs
Строки-локации: 9, 10, 11
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№444
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pass\ray_tracing.rs
Строки-локации: 20
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№445
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pass\ray_tracing.rs
Строки-локации: 24
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№446
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\commands.rs
Строки-локации: 18, 19, 22, 28, 32, 39, 40, 43, 49, 56, 63, 68, 76, 81, 88, 97, 98, 99, 102, 106, 109, 114
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

Варн№447
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\commands.rs
Строки-локации: 23, 24, 29, 30, 33, 34, 35, 44, 45, 46, 47, 50, 51, 52, 53, 54, 57, 58, 59, 64, 65, 66, 69, 70, 71, 72, 77, 78, 79, 82, 83, 84, 89, 90, 91, 92, 93, 103, 104, 110, 111, 112
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№448
Варнинг в файле: C:\RFS-0.4.0\src\rhi\sync\fence.rs
Строки-локации: 11
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№449
Варнинг в файле: C:\RFS-0.4.0\src\rhi\sync\fence.rs
Строки-локации: 20
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№450
Варнинг в файле: C:\RFS-0.4.0\src\rhi\sync\fence.rs
Строки-локации: 24, 26, 30, 34
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№451
Варнинг в файле: C:\RFS-0.4.0\src\rhi\sync\semaphore.rs
Строки-локации: 10
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

Варн№452
Варнинг в файле: C:\RFS-0.4.0\src\rhi\sync\semaphore.rs
Строки-локации: 10
В чём заключается варнинг: missing documentation for an associated constant
Тип: Предупреждение компиляции

Варн№453
Варнинг в файле: C:\RFS-0.4.0\src\rhi\sync\semaphore.rs
Строки-локации: 20
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№454
Варнинг в файле: C:\RFS-0.4.0\src\rhi\sync\semaphore.rs
Строки-локации: 29, 43
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№455
Варнинг в файле: C:\RFS-0.4.0\src\rhi\sync\semaphore.rs
Строки-локации: 33, 50, 54, 58
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№456
Варнинг в файле: C:\RFS-0.4.0\src\rhi\sync\barrier.rs
Строки-локации: 13, 14, 20, 21, 22, 23, 24, 30, 31, 32, 33, 34, 40, 41, 42, 43, 44, 50, 51, 52, 58, 59
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№457
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\allocator.rs
Строки-локации: 12, 13, 14, 15, 16, 17, 22, 23, 24, 30, 31, 32, 33, 34, 35
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№458
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\allocator.rs
Строки-локации: 40, 41, 42, 43
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№459
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\buddy.rs
Строки-локации: 12, 13, 14
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№460
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\buddy.rs
Строки-локации: 26
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№461
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\linear.rs
Строки-локации: 17
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№462
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\linear.rs
Строки-локации: 25
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№463
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\pool.rs
Строки-локации: 12, 13, 14
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№464
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\pool.rs
Строки-локации: 25
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№465
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\heap.rs
Строки-локации: 11, 12, 13
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№466
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\budget.rs
Строки-локации: 9, 10, 11, 12, 13, 14, 15
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№467
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\budget.rs
Строки-локации: 24
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№468
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\budget.rs
Строки-локации: 30
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№469
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\layout.rs
Строки-локации: 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

Варн№470
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\layout.rs
Строки-локации: 30, 31, 32, 33, 34, 40
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№471
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\layout.rs
Строки-локации: 44
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

Варн№472
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\layout.rs
Строки-локации: 44
В чём заключается варнинг: missing documentation for an associated constant
Тип: Предупреждение компиляции

Варн№473
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\layout.rs
Строки-локации: 60
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№474
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\layout.rs
Строки-локации: 64
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№475
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\set.rs
Строки-локации: 18
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№476
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\set.rs
Строки-локации: 22
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№477
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\set.rs
Строки-локации: 28, 29, 30, 31
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№478
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\set.rs
Строки-локации: 37, 38, 39, 40
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

Варн№479
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\allocator.rs
Строки-локации: 13, 14, 20, 21
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№480
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\allocator.rs
Строки-локации: 32, 43
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№481
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\allocator.rs
Строки-локации: 47
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№482
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\binding.rs
Строки-локации: 12, 13, 14, 15, 16, 17, 33, 34
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№483
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\binding.rs
Строки-локации: 26
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№484
Варнинг в файле: C:\RFS-0.4.0\src\rhi\swapchain\swapchain.rs
Строки-локации: 15, 16, 17, 18, 25, 26, 27, 28, 35, 36, 37, 38, 39, 46, 47, 48, 49, 56, 57, 58
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

Варн№485
Варнинг в файле: C:\RFS-0.4.0\src\rhi\swapchain\swapchain.rs
Строки-локации: 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 82, 83, 84
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№486
Варнинг в файле: C:\RFS-0.4.0\src\rhi\swapchain\swapchain.rs
Строки-локации: 95
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№487
Варнинг в файле: C:\RFS-0.4.0\src\rhi\swapchain\swapchain.rs
Строки-локации: 103, 104, 106, 110
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№488
Варнинг в файле: C:\RFS-0.4.0\src\rhi\swapchain\surface.rs
Строки-локации: 11, 12, 32, 33, 34, 35, 36, 37, 38, 39
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№489
Варнинг в файле: C:\RFS-0.4.0\src\rhi\swapchain\surface.rs
Строки-локации: 22
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№490
Варнинг в файле: C:\RFS-0.4.0\src\rhi\swapchain\surface.rs
Строки-локации: 26
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№491
Варнинг в файле: C:\RFS-0.4.0\src\rhi\query\pool.rs
Строки-локации: 13, 14, 15, 16, 17
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

Варн№492
Варнинг в файле: C:\RFS-0.4.0\src\rhi\query\pool.rs
Строки-локации: 21
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

Варн№493
Варнинг в файле: C:\RFS-0.4.0\src\rhi\query\pool.rs
Строки-локации: 21
В чём заключается варнинг: missing documentation for an associated constant
Тип: Предупреждение компиляции

Варн№494
Варнинг в файле: C:\RFS-0.4.0\src\rhi\query\pool.rs
Строки-локации: 31, 32, 33
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№495
Варнинг в файле: C:\RFS-0.4.0\src\rhi\query\pool.rs
Строки-локации: 42
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№496
Варнинг в файле: C:\RFS-0.4.0\src\rhi\query\pool.rs
Строки-локации: 46
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№497
Варнинг в файле: C:\RFS-0.4.0\src\rhi\query\results.rs
Строки-локации: 11, 12, 13, 14
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

Варн№498
Варнинг в файле: C:\RFS-0.4.0\src\rhi\query\results.rs
Строки-локации: 20, 21, 22, 23, 24
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№499
Варнинг в файле: C:\RFS-0.4.0\src\rhi\debug\markers.rs
Строки-локации: 9, 10, 22, 33, 34
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№500
Варнинг в файле: C:\RFS-0.4.0\src\rhi\debug\markers.rs
Строки-локации: 14, 26, 38
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№501
Варнинг в файле: C:\RFS-0.4.0\src\rhi\debug\validation.rs
Строки-локации: 11, 12, 13, 14, 15, 16, 17, 18, 19
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

Варн№502
Варнинг в файле: C:\RFS-0.4.0\src\rhi\debug\validation.rs
Строки-локации: 25, 26, 27
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№503
Варнинг в файле: C:\RFS-0.4.0\src\rhi\debug\validation.rs
Строки-локации: 31
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№504
Варнинг в файле: C:\RFS-0.4.0\src\rhi\debug\stats.rs
Строки-локации: 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 25, 26, 27, 28, 29, 30, 31, 37, 38, 39
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№505
Варнинг в файле: C:\RFS-0.4.0\src\rhi\debug\capture.rs
Строки-локации: 11, 12, 26, 27, 28
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

Варн№506
Варнинг в файле: C:\RFS-0.4.0\src\rhi\debug\capture.rs
Строки-локации: 16, 32
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

Варн№507
Варнинг в файле: C:\RFS-0.4.0\src\rhi\debug\capture.rs
Строки-локации: 40, 46
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№508
Варнинг в файле: C:\RFS-0.4.0\src\rhi\utils\conversion.rs
Строки-локации: 10, 11, 12
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

Варн№509
Варнинг в файле: C:\RFS-0.4.0\src\rhi\utils\alignment.rs
Строки-локации: 22, 23, 24
В чём заключается варнинг: missing documentation for a constant
Тип: Предупреждение компиляции

Варн№510
Варнинг в файле: C:\RFS-0.4.0\без локации
Строки-локации: —
В чём заключается варнинг: `rhi` (lib) generated 1330 warnings (75 duplicates) (run `cargo fix --lib -p rhi` to apply 40 suggestions)
Тип: Предупреждение компиляции

Варн№511
Варнинг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Строки-локации: 43
В чём заключается варнинг: unused import: `config::*`
Тип: Предупреждение компиляции

Варн№512
Варнинг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Строки-локации: 44
В чём заключается варнинг: unused import: `error::*`
Тип: Предупреждение компиляции

Варн№513
Варнинг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Строки-локации: 45
В чём заключается варнинг: unused import: `types::*`
Тип: Предупреждение компиляции

Варн№514
Варнинг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Строки-локации: 46
В чём заключается варнинг: unused import: `core::*`
Тип: Предупреждение компиляции

Варн№515
Варнинг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Строки-локации: 50
В чём заключается варнинг: unused import: `resource::*`
Тип: Предупреждение компиляции

Варн№516
Варнинг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Строки-локации: 51
В чём заключается варнинг: unused import: `pipeline::*`
Тип: Предупреждение компиляции

Варн№517
Варнинг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Строки-локации: 52
В чём заключается варнинг: unused import: `shader::*`
Тип: Предупреждение компиляции

Варн№518
Варнинг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Строки-локации: 53
В чём заключается варнинг: unused import: `command::*`
Тип: Предупреждение компиляции

Варн№519
Варнинг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Строки-локации: 54
В чём заключается варнинг: unused import: `descriptor::*`
Тип: Предупреждение компиляции

Варн№520
Варнинг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Строки-локации: 55
В чём заключается варнинг: unused import: `sync::*`
Тип: Предупреждение компиляции

Варн№521
Варнинг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Строки-локации: 56
В чём заключается варнинг: unused import: `swapchain::*`
Тип: Предупреждение компиляции

Варн№522
Варнинг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Строки-локации: 57
В чём заключается варнинг: unused import: `query::*`
Тип: Предупреждение компиляции

Варн№523
Варнинг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Строки-локации: 58
В чём заключается варнинг: unused import: `memory::*`
Тип: Предупреждение компиляции

Варн№524
Варнинг в файле: C:\RFS-0.4.0\src\render\core\settings.rs
Строки-локации: 8
В чём заключается варнинг: unused imports: `Vec3` and `Vec4`
Тип: Предупреждение компиляции

Варн№525
Варнинг в файле: C:\RFS-0.4.0\src\render\core\resource_manager.rs
Строки-локации: 8
В чём заключается варнинг: unused imports: `PathBuf` and `Path`
Тип: Предупреждение компиляции

Варн№526
Варнинг в файле: C:\RFS-0.4.0\src\render\core\resource_manager.rs
Строки-локации: 9
В чём заключается варнинг: unused import: `RwLock`
Тип: Предупреждение компиляции

Варн№527
Варнинг в файле: C:\RFS-0.4.0\src\render\core\resource_manager.rs
Строки-локации: 10
В чём заключается варнинг: unused import: `Vec3`
Тип: Предупреждение компиляции

Варн№528
Варнинг в файле: C:\RFS-0.4.0\src\render\graph\graph.rs
Строки-локации: 8
В чём заключается варнинг: unused import: `HashSet`
Тип: Предупреждение компиляции

Варн№529
Варнинг в файле: C:\RFS-0.4.0\src\render\graph\graph.rs
Строки-локации: 14
В чём заключается варнинг: unused import: `PassDependency`
Тип: Предупреждение компиляции

Варн№530
Варнинг в файле: C:\RFS-0.4.0\src\render\passes\gbuffer.rs
Строки-локации: 12
В чём заключается варнинг: unused import: `std::sync::Arc`
Тип: Предупреждение компиляции

Варн№531
Варнинг в файле: C:\RFS-0.4.0\src\render\materials\pbr.rs
Строки-локации: 15
В чём заключается варнинг: unused imports: `MaterialBuilder` and `MaterialParameters`
Тип: Предупреждение компиляции

Варн№532
Варнинг в файле: C:\RFS-0.4.0\src\render\materials\library.rs
Строки-локации: 10
В чём заключается варнинг: unused import: `crate::render::textures::Texture`
Тип: Предупреждение компиляции

Варн№533
Варнинг в файле: C:\RFS-0.4.0\src\render\materials\library.rs
Строки-локации: 11
В чём заключается варнинг: unused import: `MaterialType`
Тип: Предупреждение компиляции

Варн№534
Варнинг в файле: C:\RFS-0.4.0\src\render\meshes\loader.rs
Строки-локации: 9
В чём заключается варнинг: unused imports: `IndexType`, `PrimitiveType`, and `Vertex`
Тип: Предупреждение компиляции

Варн№535
Варнинг в файле: C:\RFS-0.4.0\src\render\textures\loader.rs
Строки-локации: 9
В чём заключается варнинг: unused import: `TextureType`
Тип: Предупреждение компиляции

Варн№536
Варнинг в файле: C:\RFS-0.4.0\src\render\textures\library.rs
Строки-локации: 10
В чём заключается варнинг: unused import: `TextureType`
Тип: Предупреждение компиляции

Варн№537
Варнинг в файле: C:\RFS-0.4.0\src\render\scene\scene.rs
Строки-локации: 10
В чём заключается варнинг: unused imports: `MaterialComponent`, `MeshComponent`, and `Transform`
Тип: Предупреждение компиляции

Варн№538
Варнинг в файле: C:\RFS-0.4.0\src\render\scene\scene.rs
Строки-локации: 11
В чём заключается варнинг: unused import: `CullingResult`
Тип: Предупреждение компиляции

Варн№539
Варнинг в файле: C:\RFS-0.4.0\src\render\scene\entity.rs
Строки-локации: 7
В чём заключается варнинг: unused imports: `Mat4`, `Quat`, `Vec3`, and `Vec4`
Тип: Предупреждение компиляции

Варн№540
Варнинг в файле: C:\RFS-0.4.0\src\render\scene\entity.rs
Строки-локации: 8
В чём заключается варнинг: unused import: `crate::render::meshes::Mesh`
Тип: Предупреждение компиляции

Варн№541
Варнинг в файле: C:\RFS-0.4.0\src\render\scene\entity.rs
Строки-локации: 9
В чём заключается варнинг: unused import: `crate::render::materials::Material`
Тип: Предупреждение компиляции

Варн№542
Варнинг в файле: C:\RFS-0.4.0\src\render\scene\components.rs
Строки-локации: 5
В чём заключается варнинг: unused import: `Vec4`
Тип: Предупреждение компиляции

Варн№543
Варнинг в файле: C:\RFS-0.4.0\src\render\scene\components.rs
Строки-локации: 8
В чём заключается варнинг: unused imports: `BlendMode as MaterialBlendMode` and `CullMode as MaterialCullMode`
Тип: Предупреждение компиляции

Варн№544
Варнинг в файле: C:\RFS-0.4.0\src\render\scene\components.rs
Строки-локации: 9
В чём заключается варнинг: unused import: `LightType`
Тип: Предупреждение компиляции

Варн№545
Варнинг в файле: C:\RFS-0.4.0\src\render\camera\camera.rs
Строки-локации: 5
В чём заключается варнинг: unused import: `Vec2`
Тип: Предупреждение компиляции

Варн№546
Варнинг в файле: C:\RFS-0.4.0\src\render\camera\controller.rs
Строки-локации: 7
В чём заключается варнинг: unused imports: `Mat4` and `Vec2`
Тип: Предупреждение компиляции

Варн№547
Варнинг в файле: C:\RFS-0.4.0\src\render\lighting\light.rs
Строки-локации: 5
В чём заключается варнинг: unused import: `Vec4`
Тип: Предупреждение компиляции

Варн№548
Варнинг в файле: C:\RFS-0.4.0\src\render\lighting\probe.rs
Строки-локации: 8
В чём заключается варнинг: unused import: `Mat4`
Тип: Предупреждение компиляции

Варн№549
Варнинг в файле: C:\RFS-0.4.0\src\render\postprocess\bloom.rs
Строки-локации: 11
В чём заключается варнинг: unused import: `PostProcessConfig`
Тип: Предупреждение компиляции

Варн№550
Варнинг в файле: C:\RFS-0.4.0\src\render\postprocess\hdr.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `ToneMapping`
Тип: Предупреждение компиляции

Варн№551
Варнинг в файле: C:\RFS-0.4.0\src\render\particles\system.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `std::collections::HashMap`
Тип: Предупреждение компиляции

Варн№552
Варнинг в файле: C:\RFS-0.4.0\src\render\particles\system.rs
Строки-локации: 10
В чём заключается варнинг: unused import: `Mat4`
Тип: Предупреждение компиляции

Варн№553
Варнинг в файле: C:\RFS-0.4.0\src\render\particles\system.rs
Строки-локации: 14
В чём заключается варнинг: unused import: `super::particle::Particle`
Тип: Предупреждение компиляции

Варн№554
Варнинг в файле: C:\RFS-0.4.0\src\render\particles\emitter.rs
Строки-локации: 252
В чём заключается варнинг: unused import: `glam::Vec3Swizzles`
Тип: Предупреждение компиляции

Варн№555
Варнинг в файле: C:\RFS-0.4.0\src\render\particles\effects\smoke.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::rhi::types::*`
Тип: Предупреждение компиляции

Варн№556
Варнинг в файле: C:\RFS-0.4.0\src\render\particles\effects\fire.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::rhi::types::*`
Тип: Предупреждение компиляции

Варн№557
Варнинг в файле: C:\RFS-0.4.0\src\render\particles\effects\splash.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::rhi::types::*`
Тип: Предупреждение компиляции

Варн№558
Варнинг в файле: C:\RFS-0.4.0\src\render\particles\effects\explosion.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::rhi::types::*`
Тип: Предупреждение компиляции

Варн№559
Варнинг в файле: C:\RFS-0.4.0\src\render\particles\effects\sparks.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::rhi::types::*`
Тип: Предупреждение компиляции

Варн№560
Варнинг в файле: C:\RFS-0.4.0\src\render\particles\effects\dust.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::rhi::types::*`
Тип: Предупреждение компиляции

Варн№561
Варнинг в файле: C:\RFS-0.4.0\src\render\particles\effects\blood.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::rhi::types::*`
Тип: Предупреждение компиляции

Варн№562
Варнинг в файле: C:\RFS-0.4.0\src\render\particles\effects\magic.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::rhi::types::*`
Тип: Предупреждение компиляции

Варн№563
Варнинг в файле: C:\RFS-0.4.0\src\render\particles\effect_manager.rs
Строки-локации: 8
В чём заключается варнинг: unused imports: `Vec3` and `Vec4`
Тип: Предупреждение компиляции

Варн№564
Варнинг в файле: C:\RFS-0.4.0\src\render\particles\effect_manager.rs
Строки-локации: 10
В чём заключается варнинг: unused import: `ParticleEmitter`
Тип: Предупреждение компиляции

Варн№565
Варнинг в файле: C:\RFS-0.4.0\src\render\water\interaction.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `Vec2`
Тип: Предупреждение компиляции

Варн№566
Варнинг в файле: C:\RFS-0.4.0\src\render\ui\hud.rs
Строки-локации: 5
В чём заключается варнинг: unused imports: `Mat4` and `Vec3`
Тип: Предупреждение компиляции

Варн№567
Варнинг в файле: C:\RFS-0.4.0\src\render\ui\minimap.rs
Строки-локации: 5
В чём заключается варнинг: unused import: `Mat4`
Тип: Предупреждение компиляции

Варн№568
Варнинг в файле: C:\RFS-0.4.0\src\render\effects\manager.rs
Строки-локации: 8
В чём заключается варнинг: unused imports: `Mat4` and `Vec4`
Тип: Предупреждение компиляции

Варн№569
Варнинг в файле: C:\RFS-0.4.0\src\render\effects\manager.rs
Строки-локации: 10
В чём заключается варнинг: unused imports: `BloomConfig` and `MotionBlurConfig`
Тип: Предупреждение компиляции

Варн№570
Варнинг в файле: C:\RFS-0.4.0\src\render\effects\underwater.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `Mat4`
Тип: Предупреждение компиляции

Варн№571
Варнинг в файле: C:\RFS-0.4.0\src\render\effects\underwater.rs
Строки-локации: 8
В чём заключается варнинг: unused import: `std::time::Duration`
Тип: Предупреждение компиляции

Варн№572
Варнинг в файле: C:\RFS-0.4.0\src\render\effects\screen_space.rs
Строки-локации: 7
В чём заключается варнинг: unused imports: `Mat4` and `Vec4`
Тип: Предупреждение компиляции

Варн№573
Варнинг в файле: C:\RFS-0.4.0\src\render\effects\screen_space.rs
Строки-локации: 123
В чём заключается варнинг: unused import: `std::f32::consts::PI`
Тип: Предупреждение компиляции

Варн№574
Варнинг в файле: C:\RFS-0.4.0\без локации
Строки-локации: —
В чём заключается варнинг: `rfs-client` (lib) generated 63 warnings
Тип: Предупреждение компиляции

