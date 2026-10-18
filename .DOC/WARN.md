# WARN — реестр предупреждений компиляции клиента RFS

Вынесен из `BUGS.md` по решению владельца. Причина не в размере: `BUGS.md`
вырос до шести тысяч строк, и в нём смешаны дефекты (у них есть статус) и
снимок вывода компилятора (статуса нет). Смешение выглядит безобидно, пока
кто-нибудь не посчитает «что открыто» построчным поиском по `Статус:` — и
получит в ответ записи, которые никто никогда не открывал и не закрывал.

## Что это и что это не

Это **снимок** вывода `cargo check` по клиенту: 1320 предупреждений исходно,
368 групп. Не список дефектов.

- У записей здесь **нет поля `Статус`**, и не потому, что забыли: снимок
  фиксирует то, что компилятор сказал в тот день. Переписывать его в
  «открыто/закрыто» значило бы приписать предупреждениям решения, которых
  никто не принимал.
- Текущее состояние **не перепроверялось**. Часть записей помечена устаревшей
  по факту проверки 2026-09-21, когда клиент не выдавал ни одного warning.

Когда понадобится честный ответ «сколько предупреждений сейчас», раздел надо
перегенерировать из свежего вывода компилятора, а не отсюда считать.

## Нумерация

`КВарн№207` … `КВарн№574`. Нумерация начинается не с единицы и **пересекается с
`Баг№`** в `BUGS.md`. Это наследие слияния двух файлов, а не решение: отличать
записи можно только по номеру. Причина та же, что описана в `BUGS.md` — читать
надо по номеру, а не по полю «Статус».

## Перенос из BUGS.md — состояние

Раздел переносится из `BUGS.md` по частям, а не одним движением: 2217 строк
уникального текста, переносить их иначе нечем.

**ПЕРЕНЕСЕНО: КВарн№207 – КВарн№574, все 368.**
**ОСТАЛОСЬ: ничего.**

Записи перенесены частями, по ходу проверяя диапазон на пропуски в обе стороны.
`BUGS.md` пока держит копию того же раздела — она помечена как устаревшая.
Удалить её оттуда можно только движением, затрагивающим все 2217 строк сразу,
а переносить такой объём по частям нельзя: `edit` заменяет непрерывный фрагмент,
а не диапазон строк. Поэтому копия остаётся, и это указано явно, чтобы её нельзя
было принять за оригинал.

До окончания переноса записи продолжают лежать в `BUGS.md`. Файл неполон, и
это указано здесь явно, чтобы его нельзя было принять за готовый.

---

КВарн№207
Варнинг в файле: C:\RFS-0.4.0\src\rhi\error.rs
Строки-локации: 6
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

КВарн№208
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 6
В чём заключается варнинг: unused import: `bitflags::bitflags`
Тип: Предупреждение компиляции

КВарн№209
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 12
В чём заключается варнинг: variant `R8_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№210
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 12
В чём заключается варнинг: variant `R8_SNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№211
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 12
В чём заключается варнинг: variant `R8_UINT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№212
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 12
В чём заключается варнинг: variant `R8_SINT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№213
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 14
В чём заключается варнинг: variant `R16_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№214
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 14
В чём заключается варнинг: variant `R16_SNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№215
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 14
В чём заключается варнинг: variant `R16_UINT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№216
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 14
В чём заключается варнинг: variant `R16_SINT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№217
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 14
В чём заключается варнинг: variant `R16_SFLOAT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№218
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 15
В чём заключается варнинг: variant `RG8_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№219
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 15
В чём заключается варнинг: variant `RG8_SNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№220
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 15
В чём заключается варнинг: variant `RG8_UINT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№221
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 15
В чём заключается варнинг: variant `RG8_SINT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№222
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 17
В чём заключается варнинг: variant `R32_UINT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№223
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 17
В чём заключается варнинг: variant `R32_SINT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№224
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 17
В чём заключается варнинг: variant `R32_SFLOAT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№225
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 18
В чём заключается варнинг: variant `RG16_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№226
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 18
В чём заключается варнинг: variant `RG16_SNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№227
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 18
В чём заключается варнинг: variant `RG16_UINT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№228
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 18
В чём заключается варнинг: variant `RG16_SINT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№229
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 18
В чём заключается варнинг: variant `RG16_SFLOAT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№230
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 19
В чём заключается варнинг: variant `RGBA8_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№231
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 19
В чём заключается варнинг: variant `RGBA8_SNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№232
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 19
В чём заключается варнинг: variant `RGBA8_UINT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№233
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 19
В чём заключается варнинг: variant `RGBA8_SINT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№234
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 21
В чём заключается варнинг: variant `R32G32_UINT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№235
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 21
В чём заключается варнинг: variant `R32G32_SINT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№236
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 21
В чём заключается варнинг: variant `R32G32_SFLOAT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№237
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 23
В чём заключается варнинг: variant `R32G32B32_UINT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№238
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 23
В чём заключается варнинг: variant `R32G32B32_SINT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№239
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 23
В чём заключается варнинг: variant `R32G32B32_SFLOAT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№240
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 25
В чём заключается варнинг: variant `RGBA32_UINT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№241
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 25
В чём заключается варнинг: variant `RGBA32_SINT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№242
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 25
В чём заключается варнинг: variant `RGBA32_SFLOAT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№243
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 27
В чём заключается варнинг: variant `D16_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№244
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 27
В чём заключается варнинг: variant `D24_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№245
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 27
В чём заключается варнинг: variant `D32_SFLOAT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№246
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 28
В чём заключается варнинг: variant `D24_UNORM_S8_UINT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№247
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 28
В чём заключается варнинг: variant `D32_SFLOAT_S8_UINT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№248
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 28
В чём заключается варнинг: variant `S8_UINT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№249
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 30
В чём заключается варнинг: variant `BC1_RGB_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№250
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 30
В чём заключается варнинг: variant `BC1_RGB_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№251
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 30
В чём заключается варнинг: variant `BC1_RGBA_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№252
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 30
В чём заключается варнинг: variant `BC1_RGBA_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№253
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 31
В чём заключается варнинг: variant `BC2_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№254
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 31
В чём заключается варнинг: variant `BC2_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№255
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 31
В чём заключается варнинг: variant `BC3_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№256
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 31
В чём заключается варнинг: variant `BC3_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№257
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 32
В чём заключается варнинг: variant `BC4_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№258
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 32
В чём заключается варнинг: variant `BC4_SNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№259
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 32
В чём заключается варнинг: variant `BC5_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№260
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 32
В чём заключается варнинг: variant `BC5_SNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№261
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 33
В чём заключается варнинг: variant `BC6H_UFLOAT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№262
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 33
В чём заключается варнинг: variant `BC6H_SFLOAT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№263
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 33
В чём заключается варнинг: variant `BC7_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№264
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 33
В чём заключается варнинг: variant `BC7_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№265
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 35
В чём заключается варнинг: variant `ASTC_4x4_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№266
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 35
В чём заключается варнинг: variant `ASTC_4x4_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№267
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 36
В чём заключается варнинг: variant `ASTC_5x4_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№268
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 36
В чём заключается варнинг: variant `ASTC_5x4_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№269
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 37
В чём заключается варнинг: variant `ASTC_5x5_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№270
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 37
В чём заключается варнинг: variant `ASTC_5x5_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№271
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 38
В чём заключается варнинг: variant `ASTC_6x5_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№272
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 38
В чём заключается варнинг: variant `ASTC_6x5_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№273
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 39
В чём заключается варнинг: variant `ASTC_6x6_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№274
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 39
В чём заключается варнинг: variant `ASTC_6x6_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№275
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 40
В чём заключается варнинг: variant `ASTC_8x5_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№276
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 40
В чём заключается варнинг: variant `ASTC_8x5_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№277
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 41
В чём заключается варнинг: variant `ASTC_8x6_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№278
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 41
В чём заключается варнинг: variant `ASTC_8x6_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№279
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 42
В чём заключается варнинг: variant `ASTC_8x8_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№280
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 42
В чём заключается варнинг: variant `ASTC_8x8_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№281
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 44
В чём заключается варнинг: variant `ETC2_R8G8B8_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№282
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 44
В чём заключается варнинг: variant `ETC2_R8G8B8_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№283
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 45
В чём заключается варнинг: variant `ETC2_R8G8B8A1_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№284
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 45
В чём заключается варнинг: variant `ETC2_R8G8B8A1_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№285
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 46
В чём заключается варнинг: variant `ETC2_R8G8B8A8_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№286
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 46
В чём заключается варнинг: variant `ETC2_R8G8B8A8_SRGB` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№287
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 47
В чём заключается варнинг: variant `EAC_R11_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№288
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 47
В чём заключается варнинг: variant `EAC_R11_SNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№289
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 48
В чём заключается варнинг: variant `EAC_R11G11_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№290
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 48
В чём заключается варнинг: variant `EAC_R11G11_SNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№291
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 50
В чём заключается варнинг: variant `B10G11R11_UFLOAT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№292
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 50
В чём заключается варнинг: variant `E5B9G9R9_UFLOAT` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№293
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 52
В чём заключается варнинг: variant `A8_UNORM` should have an upper camel case name
Тип: Предупреждение компиляции

КВарн№294
Варнинг в файле: C:\RFS-0.4.0\src\rhi\core\device.rs
Строки-локации: 5
В чём заключается варнинг: unused imports: `Arc` and `RwLock`
Тип: Предупреждение компиляции

КВарн№295
Варнинг в файле: C:\RFS-0.4.0\src\rhi\backend\common.rs
Строки-локации: 5
В чём заключается варнинг: unused import: `super::Backend`
Тип: Предупреждение компиляции

КВарн№296
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\buffer.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::error::*`
Тип: Предупреждение компиляции

КВарн№297
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\texture.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::error::*`
Тип: Предупреждение компиляции

КВарн№298
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\sampler.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

КВарн№299
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\blas.rs
Строки-локации: 18, 37
В чём заключается варнинг: unused doc comment
Тип: Предупреждение компиляции

КВарн№300
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\tlas.rs
Строки-локации: 30
В чём заключается варнинг: unused doc comment
Тип: Предупреждение компиляции

КВарн№301
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\tlas.rs
Строки-локации: 6
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

КВарн№302
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\build.rs
Строки-локации: 6
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

КВарн№303
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\build.rs
Строки-локации: 8
В чём заключается варнинг: unused import: `AccelerationStructureFlags`
Тип: Предупреждение компиляции

КВарн№304
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\query.rs
Строки-локации: 6
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

КВарн№305
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\compute.rs
Строки-локации: 6
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

КВарн№306
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\compute.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::shader::ShaderModule`
Тип: Предупреждение компиляции

КВарн№307
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\ray_tracing.rs
Строки-локации: 6
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

КВарн№308
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\mesh_shading.rs
Строки-локации: 6
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

КВарн№309
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\mesh_shading.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::shader::ShaderModule`
Тип: Предупреждение компиляции

КВарн№310
Варнинг в файле: C:\RFS-0.4.0\src\rhi\shader\module.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

КВарн№311
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\buffer.rs
Строки-локации: 28
В чём заключается варнинг: unused doc comment
Тип: Предупреждение компиляции

КВарн№312
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\buffer.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

КВарн№313
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pool.rs
Строки-локации: 9
В чём заключается варнинг: unused doc comment
Тип: Предупреждение компиляции

КВарн№314
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pool.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

КВарн№315
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\encoder.rs
Строки-локации: 6
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

КВарн№316
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pass\render.rs
Строки-локации: 55, 75
В чём заключается варнинг: unused doc comment
Тип: Предупреждение компиляции

КВарн№317
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\commands.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `super::buffer::CommandBuffer`
Тип: Предупреждение компиляции

КВарн№318
Варнинг в файле: C:\RFS-0.4.0\src\rhi\sync\semaphore.rs
Строки-локации: 9
В чём заключается варнинг: unused doc comment
Тип: Предупреждение компиляции

КВарн№319
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\layout.rs
Строки-локации: 43
В чём заключается варнинг: unused doc comment
Тип: Предупреждение компиляции

КВарн№320
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\set.rs
Строки-локации: 6
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

КВарн№321
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\allocator.rs
Строки-локации: 6
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

КВарн№322
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\binding.rs
Строки-локации: 6
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

КВарн№323
Варнинг в файле: C:\RFS-0.4.0\src\rhi\query\pool.rs
Строки-локации: 20
В чём заключается варнинг: unused doc comment
Тип: Предупреждение компиляции

КВарн№324
Варнинг в файле: C:\RFS-0.4.0\src\rhi\query\pool.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::types::*`
Тип: Предупреждение компиляции

КВарн№325
Варнинг в файле: C:\RFS-0.4.0\src\rhi\query\results.rs
Строки-локации: 6
В чём заключается варнинг: unused import: `super::pool::QueryType`
Тип: Предупреждение компиляции

КВарн№326
Варнинг в файле: C:\RFS-0.4.0\src\rhi\debug\capture.rs
Строки-локации: 6
В чём заключается варнинг: unused import: `std::collections::HashMap`
Тип: Предупреждение компиляции

КВарн№327
Варнинг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Строки-локации: 45, 46, 50, 53, 53, 53, 54
В чём заключается варнинг: ambiguous glob re-exports
Тип: Предупреждение компиляции

КВарн№328
Варнинг в файле: C:\RFS-0.4.0\src\rhi\core\caps.rs
Строки-локации: 15
В чём заключается варнинг: unused variable: `required`
Тип: Предупреждение компиляции

КВарн№329
Варнинг в файле: C:\RFS-0.4.0\src\rhi\backend\mod.rs:53:9 ; --> src\rhi\backend\mod.rs
Строки-локации: 53
В чём заключается варнинг: unreachable pattern
Тип: Предупреждение компиляции

КВарн№330
Варнинг в файле: C:\RFS-0.4.0\src\rhi\sync\fence.rs
Строки-локации: 26
В чём заключается варнинг: unused variable: `timeout`
Тип: Предупреждение компиляции

КВарн№331
Варнинг в файле: C:\RFS-0.4.0\src\rhi\sync\semaphore.rs
Строки-локации: 50, 54
В чём заключается варнинг: unused variable: `value`
Тип: Предупреждение компиляции

КВарн№332
Варнинг в файле: C:\RFS-0.4.0\src\rhi\sync\semaphore.rs
Строки-локации: 54
В чём заключается варнинг: unused variable: `timeout`
Тип: Предупреждение компиляции

КВарн№333
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\buddy.rs
Строки-локации: 37
В чём заключается варнинг: unused variable: `desc`
Тип: Предупреждение компиляции

КВарн№334
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\buddy.rs
Строки-локации: 41
В чём заключается варнинг: unused variable: `allocation`
Тип: Предупреждение компиляции

КВарн№335
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\linear.rs
Строки-локации: 31
В чём заключается варнинг: unused variable: `desc`
Тип: Предупреждение компиляции

КВарн№336
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\pool.rs
Строки-локации: 25
В чём заключается варнинг: unused variable: `initial_blocks`
Тип: Предупреждение компиляции

КВарн№337
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\pool.rs
Строки-локации: 35
В чём заключается варнинг: unused variable: `desc`
Тип: Предупреждение компиляции

КВарн№338
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\pool.rs
Строки-локации: 39
В чём заключается варнинг: unused variable: `allocation`
Тип: Предупреждение компиляции

КВарн№339
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\allocator.rs
Строки-локации: 47
В чём заключается варнинг: unused variable: `layout`
Тип: Предупреждение компиляции

КВарн№340
Варнинг в файле: C:\RFS-0.4.0\src\rhi\swapchain\swapchain.rs
Строки-локации: 110
В чём заключается варнинг: unused variable: `image_index`
Тип: Предупреждение компиляции

КВарн№341
Варнинг в файле: C:\RFS-0.4.0\src\rhi\config\settings.rs:58:5 ; --> src\rhi\lib.rs
Строки-локации: 22
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№342
Варнинг в файле: C:\RFS-0.4.0\src\rhi\config\settings.rs
Строки-локации: 59, 60, 61, 62, 80, 81, 82, 83
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№343
Варнинг в файле: C:\RFS-0.4.0\src\rhi\config\settings.rs
Строки-локации: 100, 101, 102, 103
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

КВарн№344
Варнинг в файле: C:\RFS-0.4.0\src\rhi\error.rs
Строки-локации: 16, 19, 23, 26, 29, 32, 36, 39, 42, 46, 49, 53, 56, 60, 63, 66, 70, 74, 78, 82, 85
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

КВарн№345
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 12, 12, 12, 12, 14, 14, 14, 14, 14, 15, 15, 15, 15, 17, 17, 17, 18, 18, 18, 18, 18, 19, 19, 19, 19, 21, 21, 21, 23, 23, 23, 25, 25, 25, 27, 27, 27, 28, 28, 28, 30, 30, 30, 30, 31, 31, 31, 31, 32, 32, 32, 32, 33, 33, 33, 33, 35, 35, 36, 36, 37, 37, 38, 38, 39, 39, 40, 40, 41, 41, 42, 42, 44, 44, 45, 45, 46, 46, 47, 47, 48, 48, 50, 50, 52, 102, 102, 102, 102, 102, 102, 102, 172, 173, 185, 186, 186, 187, 187, 187, 193, 193, 200, 200, 200, 200, 207, 208, 214, 215, 216, 217
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

КВарн№346
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 62, 70, 77, 106
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№347
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 118, 119, 129, 130, 131, 137, 138, 144, 145, 146, 152, 153, 159, 160, 161, 162, 163, 164, 172, 172, 172, 172, 173, 173
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№348
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 123, 177
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№349
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\primitives.rs
Строки-локации: 167
В чём заключается варнинг: missing documentation for a type alias
Тип: Предупреждение компиляции

КВарн№350
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\flags.rs
Строки-локации: 8, 20, 34, 46, 55, 70, 86, 107, 117, 124, 132, 141
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

КВарн№351
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\flags.rs
Строки-локации: 8, 20, 34, 46, 55, 70, 86, 107, 117, 124, 132, 141
В чём заключается варнинг: missing documentation for an associated constant
Тип: Предупреждение компиляции

КВарн№352
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\features.rs
Строки-локации: 8
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

КВарн№353
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\features.rs
Строки-локации: 8, 10, 11, 12, 13, 14, 17, 20, 21, 24, 25, 26, 27, 30, 31, 34, 35, 38, 39, 42, 45, 48, 51, 54, 57
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№354
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\limits.rs
Строки-локации: 9
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

КВарн№355
Варнинг в файле: C:\RFS-0.4.0\src\rhi\types\limits.rs
Строки-локации: 11, 12, 13, 16, 17, 18, 21, 24, 25, 28, 29, 32, 35, 36
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№356
Варнинг в файле: C:\RFS-0.4.0\src\rhi\core\device.rs
Строки-локации: 12, 13, 14, 15
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

КВарн№357
Варнинг в файле: C:\RFS-0.4.0\src\rhi\core\device.rs
Строки-локации: 21, 22, 28, 29, 35, 36, 42, 43, 44, 45, 46, 52, 53, 54, 55, 56, 57, 58, 59, 71, 72, 73, 85, 86, 87
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№358
Варнинг в файле: C:\RFS-0.4.0\src\rhi\core\device.rs
Строки-локации: 63, 77, 78, 79, 97, 98, 99, 102
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№359
Варнинг в файле: C:\RFS-0.4.0\src\rhi\core\caps.rs
Строки-локации: 8
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

КВарн№360
Варнинг в файле: C:\RFS-0.4.0\src\rhi\core\caps.rs
Строки-локации: 9, 10, 11
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№361
Варнинг в файле: C:\RFS-0.4.0\src\rhi\core\caps.rs
Строки-локации: 15
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№362
Варнинг в файле: C:\RFS-0.4.0\src\rhi\backend\mod.rs
Строки-локации: 9, 10, 11, 12, 13
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№363
Варнинг в файле: C:\RFS-0.4.0\src\rhi\backend\common.rs
Строки-локации: 10, 11
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№364
Варнинг в файле: C:\RFS-0.4.0\src\rhi\backend\vulkan\mod.rs
Строки-локации: 8
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

КВарн№365
Варнинг в файле: C:\RFS-0.4.0\src\rhi\backend\vulkan\mod.rs
Строки-локации: 11
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№366
Варнинг в файле: C:\RFS-0.4.0\src\rhi\backend\d3d12\mod.rs
Строки-локации: 8
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

КВарн№367
Варнинг в файле: C:\RFS-0.4.0\src\rhi\backend\d3d12\mod.rs
Строки-локации: 11
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№368
Варнинг в файле: C:\RFS-0.4.0\src\rhi\backend\d3d11\mod.rs
Строки-локации: 8
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

КВарн№369
Варнинг в файле: C:\RFS-0.4.0\src\rhi\backend\d3d11\mod.rs
Строки-локации: 11
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№370
Варнинг в файле: C:\RFS-0.4.0\src\rhi\backend\opengl\mod.rs
Строки-локации: 8
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

КВарн№371
Варнинг в файле: C:\RFS-0.4.0\src\rhi\backend\opengl\mod.rs
Строки-локации: 11
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№372
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\buffer.rs
Строки-локации: 12, 13, 14, 15, 16
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№373
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\buffer.rs
Строки-локации: 27
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№374
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\buffer.rs
Строки-локации: 34, 35
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№375
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\texture.rs
Строки-локации: 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 45, 46, 47, 48, 49, 50, 51, 52
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№376
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\texture.rs
Строки-локации: 32
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№377
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\texture.rs
Строки-локации: 36, 37, 38, 39
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№378
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\texture.rs
Строки-локации: 59, 59, 59, 59, 59, 59, 59, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

КВарн№379
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\sampler.rs
Строки-локации: 13, 14, 21, 22, 23, 24, 25, 32, 33, 34, 35, 36, 37, 38, 39, 46, 47, 48, 49, 50, 51
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

КВарн№380
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\sampler.rs
Строки-локации: 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№381
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\sampler.rs
Строки-локации: 80
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№382
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\sampler.rs
Строки-локации: 84
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№383
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\blas.rs
Строки-локации: 12, 13, 49, 50, 51, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 80, 81, 82, 83, 89, 90, 91
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№384
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\blas.rs
Строки-локации: 19, 38
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

КВарн№385
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\blas.rs
Строки-локации: 19, 38
В чём заключается варнинг: missing documentation for an associated constant
Тип: Предупреждение компиляции

КВарн№386
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\blas.rs
Строки-локации: 32, 33, 34, 57, 58, 59
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

КВарн№387
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\blas.rs
Строки-локации: 101
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№388
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\tlas.rs
Строки-локации: 12, 13, 14, 20, 21, 22, 23, 24, 25
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№389
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\tlas.rs
Строки-локации: 31
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

КВарн№390
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\tlas.rs
Строки-локации: 31
В чём заключается варнинг: missing documentation for an associated constant
Тип: Предупреждение компиляции

КВарн№391
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\tlas.rs
Строки-локации: 49
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№392
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\build.rs
Строки-локации: 14, 15, 30, 31, 37, 38
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

КВарн№393
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\build.rs
Строки-локации: 21, 22, 23, 24, 44, 45, 46, 47
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№394
Варнинг в файле: C:\RFS-0.4.0\src\rhi\resource\acceleration\query.rs
Строки-локации: 11, 12, 13
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

КВарн№395
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\graphics.rs
Строки-локации: 13, 14, 15, 16, 22, 23, 24, 37, 38, 56, 57, 90, 91, 92, 93, 94, 95, 96, 97, 98, 99, 119, 120, 121, 122, 123, 124, 125, 131, 132, 133, 134, 135, 136, 137, 138, 139, 173, 174, 175, 176, 177, 178, 179, 180, 208, 209, 210, 211, 217, 218, 224, 225, 226, 227, 233, 234, 235, 241, 242, 243, 244, 245, 246, 247, 248
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№396
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\graphics.rs
Строки-локации: 30, 31, 45, 46, 47, 48, 49, 50, 64, 65, 66, 73, 74, 75, 76, 83, 84, 106, 107, 108, 109, 110, 111, 112, 113, 146, 147, 148, 149, 150, 151, 152, 153, 154, 155, 156, 163, 164, 165, 166, 167, 187, 188, 189, 190, 191, 192, 193, 194, 195, 196, 197, 198, 199, 200, 201, 202
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

КВарн№397
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\graphics.rs
Строки-локации: 258
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№398
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\graphics.rs
Строки-локации: 262
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№399
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\compute.rs
Строки-локации: 13
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№400
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\compute.rs
Строки-локации: 23
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№401
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\compute.rs
Строки-локации: 27
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№402
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\ray_tracing.rs
Строки-локации: 13, 14, 15, 16, 17, 18
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

КВарн№403
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\ray_tracing.rs
Строки-локации: 24, 25, 31, 32, 38, 39, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 74, 75, 76, 77
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№404
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\ray_tracing.rs
Строки-локации: 48
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№405
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\ray_tracing.rs
Строки-локации: 52
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№406
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\mesh_shading.rs
Строки-локации: 13, 14, 15
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№407
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\mesh_shading.rs
Строки-локации: 24
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№408
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\mesh_shading.rs
Строки-локации: 28
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№409
Варнинг в файле: C:\RFS-0.4.0\src\rhi\pipeline\state.rs
Строки-локации: 12, 13, 19, 20, 21
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№410
Варнинг в файле: C:\RFS-0.4.0\src\rhi\shader\module.rs
Строки-локации: 13, 14, 15, 16, 17, 18
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

КВарн№411
Варнинг в файле: C:\RFS-0.4.0\src\rhi\shader\module.rs
Строки-локации: 24, 25, 26, 27
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№412
Варнинг в файле: C:\RFS-0.4.0\src\rhi\shader\module.rs
Строки-локации: 37
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№413
Варнинг в файле: C:\RFS-0.4.0\src\rhi\shader\module.rs
Строки-локации: 41
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№414
Варнинг в файле: C:\RFS-0.4.0\src\rhi\shader\reflection.rs
Строки-локации: 11, 12, 13, 14, 40, 41, 47, 48, 49, 50, 56, 57, 58, 59, 60, 85, 86, 87, 88, 89, 90, 91, 97, 98, 99, 100
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№415
Варнинг в файле: C:\RFS-0.4.0\src\rhi\shader\reflection.rs
Строки-локации: 20, 21, 22, 23, 24, 30, 31, 32, 33, 34, 66, 67, 68, 74, 75, 76, 77, 78, 79
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

КВарн№416
Варнинг в файле: C:\RFS-0.4.0\src\rhi\shader\compiler.rs
Строки-локации: 12, 13, 14, 15, 16
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№417
Варнинг в файле: C:\RFS-0.4.0\src\rhi\shader\compiler.rs
Строки-локации: 22, 23, 24, 25
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№418
Варнинг в файле: C:\RFS-0.4.0\src\rhi\shader\compiler.rs
Строки-локации: 32, 33, 34, 41, 42, 43
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

КВарн№419
Варнинг в файле: C:\RFS-0.4.0\src\rhi\shader\stage.rs
Строки-локации: 9, 10, 11, 12, 13, 14, 15
В чём заключается варнинг: missing documentation for a constant
Тип: Предупреждение компиляции

КВарн№420
Варнинг в файле: C:\RFS-0.4.0\src\rhi\shader\library.rs
Строки-локации: 11, 12
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№421
Варнинг в файле: C:\RFS-0.4.0\src\rhi\shader\library.rs
Строки-локации: 22
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№422
Варнинг в файле: C:\RFS-0.4.0\src\rhi\shader\library.rs
Строки-локации: 29, 33
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№423
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\buffer.rs
Строки-локации: 14, 15, 21, 22, 23, 24, 25
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

КВарн№424
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\buffer.rs
Строки-локации: 29
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

КВарн№425
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\buffer.rs
Строки-локации: 29
В чём заключается варнинг: missing documentation for an associated constant
Тип: Предупреждение компиляции

КВарн№426
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\buffer.rs
Строки-локации: 42, 43, 44
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№427
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\buffer.rs
Строки-локации: 55
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№428
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\buffer.rs
Строки-локации: 63, 64, 65, 67, 71
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№429
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pool.rs
Строки-локации: 10
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

КВарн№430
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pool.rs
Строки-локации: 10
В чём заключается варнинг: missing documentation for an associated constant
Тип: Предупреждение компиляции

КВарн№431
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pool.rs
Строки-локации: 22, 23
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№432
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pool.rs
Строки-локации: 33
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№433
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pool.rs
Строки-локации: 40, 41
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№434
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\encoder.rs
Строки-локации: 15, 28, 39, 50
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№435
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\encoder.rs
Строки-локации: 19
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№436
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pass\render.rs
Строки-локации: 14, 15, 16, 23, 24, 51, 52
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

КВарн№437
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pass\render.rs
Строки-локации: 30, 31, 32, 33, 34, 35, 36, 37, 43, 44, 66, 67, 68, 69, 70, 71, 72, 88, 89, 90, 91, 92, 93, 94, 100, 101, 102, 122, 123, 124, 125, 126, 132, 133, 134, 154, 155, 156, 157
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№438
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pass\render.rs
Строки-локации: 56, 76
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

КВарн№439
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pass\render.rs
Строки-локации: 56, 76
В чём заключается варнинг: missing documentation for an associated constant
Тип: Предупреждение компиляции

КВарн№440
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pass\render.rs
Строки-локации: 112, 146
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№441
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pass\render.rs
Строки-локации: 116
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№442
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pass\compute.rs
Строки-локации: 13
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№443
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pass\ray_tracing.rs
Строки-локации: 9, 10, 11
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№444
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pass\ray_tracing.rs
Строки-локации: 20
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№445
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\pass\ray_tracing.rs
Строки-локации: 24
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№446
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\commands.rs
Строки-локации: 18, 19, 22, 28, 32, 39, 40, 43, 49, 56, 63, 68, 76, 81, 88, 97, 98, 99, 102, 106, 109, 114
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

КВарн№447
Варнинг в файле: C:\RFS-0.4.0\src\rhi\command\commands.rs
Строки-локации: 23, 24, 29, 30, 33, 34, 35, 44, 45, 46, 47, 50, 51, 52, 53, 54, 57, 58, 59, 64, 65, 66, 69, 70, 71, 72, 77, 78, 79, 82, 83, 84, 89, 90, 91, 92, 93, 103, 104, 110, 111, 112
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№448
Варнинг в файле: C:\RFS-0.4.0\src\rhi\sync\fence.rs
Строки-локации: 11
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№449
Варнинг в файле: C:\RFS-0.4.0\src\rhi\sync\fence.rs
Строки-локации: 20
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№450
Варнинг в файле: C:\RFS-0.4.0\src\rhi\sync\fence.rs
Строки-локации: 24, 26, 30, 34
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№451
Варнинг в файле: C:\RFS-0.4.0\src\rhi\sync\semaphore.rs
Строки-локации: 10
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

КВарн№452
Варнинг в файле: C:\RFS-0.4.0\src\rhi\sync\semaphore.rs
Строки-локации: 10
В чём заключается варнинг: missing documentation for an associated constant
Тип: Предупреждение компиляции

КВарн№453
Варнинг в файле: C:\RFS-0.4.0\src\rhi\sync\semaphore.rs
Строки-локации: 20
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№454
Варнинг в файле: C:\RFS-0.4.0\src\rhi\sync\semaphore.rs
Строки-локации: 29, 43
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№455
Варнинг в файле: C:\RFS-0.4.0\src\rhi\sync\semaphore.rs
Строки-локации: 33, 50, 54, 58
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№456
Варнинг в файле: C:\RFS-0.4.0\src\rhi\sync\barrier.rs
Строки-локации: 13, 14, 20, 21, 22, 23, 24, 30, 31, 32, 33, 34, 40, 41, 42, 43, 44, 50, 51, 52, 58, 59
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№457
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\allocator.rs
Строки-локации: 12, 13, 14, 15, 16, 17, 22, 23, 24, 30, 31, 32, 33, 34, 35
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№458
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\allocator.rs
Строки-локации: 40, 41, 42, 43
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№459
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\buddy.rs
Строки-локации: 12, 13, 14
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№460
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\buddy.rs
Строки-локации: 26
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№461
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\linear.rs
Строки-локации: 17
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№462
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\linear.rs
Строки-локации: 25
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№463
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\pool.rs
Строки-локации: 12, 13, 14
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№464
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\pool.rs
Строки-локации: 25
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№465
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\heap.rs
Строки-локации: 11, 12, 13
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№466
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\budget.rs
Строки-локации: 9, 10, 11, 12, 13, 14, 15
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№467
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\budget.rs
Строки-локации: 24
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№468
Варнинг в файле: C:\RFS-0.4.0\src\rhi\memory\budget.rs
Строки-локации: 30
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№469
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\layout.rs
Строки-локации: 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

КВарн№470
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\layout.rs
Строки-локации: 30, 31, 32, 33, 34, 40
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№471
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\layout.rs
Строки-локации: 44
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

КВарн№472
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\layout.rs
Строки-локации: 44
В чём заключается варнинг: missing documentation for an associated constant
Тип: Предупреждение компиляции

КВарн№473
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\layout.rs
Строки-локации: 60
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№474
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\layout.rs
Строки-локации: 64
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№475
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\set.rs
Строки-локации: 18
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№476
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\set.rs
Строки-локации: 22
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№477
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\set.rs
Строки-локации: 28, 29, 30, 31
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№478
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\set.rs
Строки-локации: 37, 38, 39, 40
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

КВарн№479
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\allocator.rs
Строки-локации: 13, 14, 20, 21
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№480
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\allocator.rs
Строки-локации: 32, 43
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№481
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\allocator.rs
Строки-локации: 47
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№482
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\binding.rs
Строки-локации: 12, 13, 14, 15, 16, 17, 33, 34
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№483
Варнинг в файле: C:\RFS-0.4.0\src\rhi\descriptor\binding.rs
Строки-локации: 26
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№484
Варнинг в файле: C:\RFS-0.4.0\src\rhi\swapchain\swapchain.rs
Строки-локации: 15, 16, 17, 18, 25, 26, 27, 28, 35, 36, 37, 38, 39, 46, 47, 48, 49, 56, 57, 58
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

КВарн№485
Варнинг в файле: C:\RFS-0.4.0\src\rhi\swapchain\swapchain.rs
Строки-локации: 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 82, 83, 84
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№486
Варнинг в файле: C:\RFS-0.4.0\src\rhi\swapchain\swapchain.rs
Строки-локации: 95
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№487
Варнинг в файле: C:\RFS-0.4.0\src\rhi\swapchain\swapchain.rs
Строки-локации: 103, 104, 106, 110
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№488
Варнинг в файле: C:\RFS-0.4.0\src\rhi\swapchain\surface.rs
Строки-локации: 11, 12, 32, 33, 34, 35, 36, 37, 38, 39
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№489
Варнинг в файле: C:\RFS-0.4.0\src\rhi\swapchain\surface.rs
Строки-локации: 22
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№490
Варнинг в файле: C:\RFS-0.4.0\src\rhi\swapchain\surface.rs
Строки-локации: 26
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№491
Варнинг в файле: C:\RFS-0.4.0\src\rhi\query\pool.rs
Строки-локации: 13, 14, 15, 16, 17
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

КВарн№492
Варнинг в файле: C:\RFS-0.4.0\src\rhi\query\pool.rs
Строки-локации: 21
В чём заключается варнинг: missing documentation for a struct
Тип: Предупреждение компиляции

КВарн№493
Варнинг в файле: C:\RFS-0.4.0\src\rhi\query\pool.rs
Строки-локации: 21
В чём заключается варнинг: missing documentation for an associated constant
Тип: Предупреждение компиляции

КВарн№494
Варнинг в файле: C:\RFS-0.4.0\src\rhi\query\pool.rs
Строки-локации: 31, 32, 33
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№495
Варнинг в файле: C:\RFS-0.4.0\src\rhi\query\pool.rs
Строки-локации: 42
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№496
Варнинг в файле: C:\RFS-0.4.0\src\rhi\query\pool.rs
Строки-локации: 46
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№497
Варнинг в файле: C:\RFS-0.4.0\src\rhi\query\results.rs
Строки-локации: 11, 12, 13, 14
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

КВарн№498
Варнинг в файле: C:\RFS-0.4.0\src\rhi\query\results.rs
Строки-локации: 20, 21, 22, 23, 24
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№499
Варнинг в файле: C:\RFS-0.4.0\src\rhi\debug\markers.rs
Строки-локации: 9, 10, 22, 33, 34
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№500
Варнинг в файле: C:\RFS-0.4.0\src\rhi\debug\markers.rs
Строки-локации: 14, 26, 38
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№501
Варнинг в файле: C:\RFS-0.4.0\src\rhi\debug\validation.rs
Строки-локации: 11, 12, 13, 14, 15, 16, 17, 18, 19
В чём заключается варнинг: missing documentation for a variant
Тип: Предупреждение компиляции

КВарн№502
Варнинг в файле: C:\RFS-0.4.0\src\rhi\debug\validation.rs
Строки-локации: 25, 26, 27
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№503
Варнинг в файле: C:\RFS-0.4.0\src\rhi\debug\validation.rs
Строки-локации: 31
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№504
Варнинг в файле: C:\RFS-0.4.0\src\rhi\debug\stats.rs
Строки-локации: 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 25, 26, 27, 28, 29, 30, 31, 37, 38, 39
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№505
Варнинг в файле: C:\RFS-0.4.0\src\rhi\debug\capture.rs
Строки-локации: 11, 12, 26, 27, 28
В чём заключается варнинг: missing documentation for a struct field
Тип: Предупреждение компиляции

КВарн№506
Варнинг в файле: C:\RFS-0.4.0\src\rhi\debug\capture.rs
Строки-локации: 16, 32
В чём заключается варнинг: missing documentation for an associated function
Тип: Предупреждение компиляции

КВарн№507
Варнинг в файле: C:\RFS-0.4.0\src\rhi\debug\capture.rs
Строки-локации: 40, 46
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№508
Варнинг в файле: C:\RFS-0.4.0\src\rhi\utils\conversion.rs
Строки-локации: 10, 11, 12
В чём заключается варнинг: missing documentation for a method
Тип: Предупреждение компиляции

КВарн№509
Варнинг в файле: C:\RFS-0.4.0\src\rhi\utils\alignment.rs
Строки-локации: 22, 23, 24
В чём заключается варнинг: missing documentation for a constant
Тип: Предупреждение компиляции

КВарн№510
Варнинг в файле: C:\RFS-0.4.0\без локации
Строки-локации: —
В чём заключается варнинг: `rhi` (lib) generated 1330 warnings (75 duplicates) (run `cargo fix --lib -p rhi` to apply 40 suggestions)
Тип: Предупреждение компиляции

КВарн№511
Варнинг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Строки-локации: 43
В чём заключается варнинг: unused import: `config::*`
Тип: Предупреждение компиляции

КВарн№512
Варнинг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Строки-локации: 44
В чём заключается варнинг: unused import: `error::*`
Тип: Предупреждение компиляции

КВарн№513
Варнинг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Строки-локации: 45
В чём заключается варнинг: unused import: `types::*`
Тип: Предупреждение компиляции

КВарн№514
Варнинг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Строки-локации: 46
В чём заключается варнинг: unused import: `core::*`
Тип: Предупреждение компиляции

КВарн№515
Варнинг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Строки-локации: 50
В чём заключается варнинг: unused import: `resource::*`
Тип: Предупреждение компиляции

КВарн№516
Варнинг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Строки-локации: 51
В чём заключается варнинг: unused import: `pipeline::*`
Тип: Предупреждение компиляции

КВарн№517
Варнинг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Строки-локации: 52
В чём заключается варнинг: unused import: `shader::*`
Тип: Предупреждение компиляции

КВарн№518
Варнинг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Строки-локации: 53
В чём заключается варнинг: unused import: `command::*`
Тип: Предупреждение компиляции

КВарн№519
Варнинг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Строки-локации: 54
В чём заключается варнинг: unused import: `descriptor::*`
Тип: Предупреждение компиляции

КВарн№520
Варнинг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Строки-локации: 55
В чём заключается варнинг: unused import: `sync::*`
Тип: Предупреждение компиляции

КВарн№521
Варнинг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Строки-локации: 56
В чём заключается варнинг: unused import: `swapchain::*`
Тип: Предупреждение компиляции

КВарн№522
Варнинг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Строки-локации: 57
В чём заключается варнинг: unused import: `query::*`
Тип: Предупреждение компиляции

КВарн№523
Варнинг в файле: C:\RFS-0.4.0\src\rhi\lib.rs
Строки-локации: 58
В чём заключается варнинг: unused import: `memory::*`
Тип: Предупреждение компиляции

КВарн№524
Варнинг в файле: C:\RFS-0.4.0\src\render\core\settings.rs
Строки-локации: 8
В чём заключается варнинг: unused imports: `Vec3` and `Vec4`
Тип: Предупреждение компиляции

КВарн№525
Варнинг в файле: C:\RFS-0.4.0\src\render\core\resource_manager.rs
Строки-локации: 8
В чём заключается варнинг: unused imports: `PathBuf` and `Path`
Тип: Предупреждение компиляции

КВарн№526
Варнинг в файле: C:\RFS-0.4.0\src\render\core\resource_manager.rs
Строки-локации: 9
В чём заключается варнинг: unused import: `RwLock`
Тип: Предупреждение компиляции

КВарн№527
Варнинг в файле: C:\RFS-0.4.0\src\render\core\resource_manager.rs
Строки-локации: 10
В чём заключается варнинг: unused import: `Vec3`
Тип: Предупреждение компиляции

КВарн№528
Варнинг в файле: C:\RFS-0.4.0\src\render\graph\graph.rs
Строки-локации: 8
В чём заключается варнинг: unused import: `HashSet`
Тип: Предупреждение компиляции

КВарн№529
Варнинг в файле: C:\RFS-0.4.0\src\render\graph\graph.rs
Строки-локации: 14
В чём заключается варнинг: unused import: `PassDependency`
Тип: Предупреждение компиляции

КВарн№530
Варнинг в файле: C:\RFS-0.4.0\src\render\passes\gbuffer.rs
Строки-локации: 12
В чём заключается варнинг: unused import: `std::sync::Arc`
Тип: Предупреждение компиляции

КВарн№531
Варнинг в файле: C:\RFS-0.4.0\src\render\materials\pbr.rs
Строки-локации: 15
В чём заключается варнинг: unused imports: `MaterialBuilder` and `MaterialParameters`
Тип: Предупреждение компиляции

КВарн№532
Варнинг в файле: C:\RFS-0.4.0\src\render\materials\library.rs
Строки-локации: 10
В чём заключается варнинг: unused import: `crate::render::textures::Texture`
Тип: Предупреждение компиляции

КВарн№533
Варнинг в файле: C:\RFS-0.4.0\src\render\materials\library.rs
Строки-локации: 11
В чём заключается варнинг: unused import: `MaterialType`
Тип: Предупреждение компиляции

КВарн№534
Варнинг в файле: C:\RFS-0.4.0\src\render\meshes\loader.rs
Строки-локации: 9
В чём заключается варнинг: unused imports: `IndexType`, `PrimitiveType`, and `Vertex`
Тип: Предупреждение компиляции

КВарн№535
Варнинг в файле: C:\RFS-0.4.0\src\render\textures\loader.rs
Строки-локации: 9
В чём заключается варнинг: unused import: `TextureType`
Тип: Предупреждение компиляции

КВарн№536
Варнинг в файле: C:\RFS-0.4.0\src\render\textures\library.rs
Строки-локации: 10
В чём заключается варнинг: unused import: `TextureType`
Тип: Предупреждение компиляции

КВарн№537
Варнинг в файле: C:\RFS-0.4.0\src\render\scene\scene.rs
Строки-локации: 10
В чём заключается варнинг: unused imports: `MaterialComponent`, `MeshComponent`, and `Transform`
Тип: Предупреждение компиляции

КВарн№538
Варнинг в файле: C:\RFS-0.4.0\src\render\scene\scene.rs
Строки-локации: 11
В чём заключается варнинг: unused import: `CullingResult`
Тип: Предупреждение компиляции

КВарн№539
Варнинг в файле: C:\RFS-0.4.0\src\render\scene\entity.rs
Строки-локации: 7
В чём заключается варнинг: unused imports: `Mat4`, `Quat`, `Vec3`, and `Vec4`
Тип: Предупреждение компиляции

КВарн№540
Варнинг в файле: C:\RFS-0.4.0\src\render\scene\entity.rs
Строки-локации: 8
В чём заключается варнинг: unused import: `crate::render::meshes::Mesh`
Тип: Предупреждение компиляции

КВарн№541
Варнинг в файле: C:\RFS-0.4.0\src\render\scene\entity.rs
Строки-локации: 9
В чём заключается варнинг: unused import: `crate::render\materials::Material`
Тип: Предупреждение компиляции

КВарн№542
Варнинг в файле: C:\RFS-0.4.0\src\render\scene\components.rs
Строки-локации: 5
В чём заключается варнинг: unused import: `Vec4`
Тип: Предупреждение компиляции

КВарн№543
Варнинг в файле: C:\RFS-0.4.0\src\render\scene\components.rs
Строки-локации: 8
В чём заключается варнинг: unused imports: `BlendMode as MaterialBlendMode` and `CullMode as MaterialCullMode`
Тип: Предупреждение компиляции

КВарн№544
Варнинг в файле: C:\RFS-0.4.0\src\render\scene\components.rs
Строки-локации: 9
В чём заключается варнинг: unused import: `LightType`
Тип: Предупреждение компиляции

КВарн№545
Варнинг в файле: C:\RFS-0.4.0\src\render\camera\camera.rs
Строки-локации: 5
В чём заключается варнинг: unused import: `Vec2`
Тип: Предупреждение компиляции

КВарн№546
Варнинг в файле: C:\RFS-0.4.0\src\render\camera\controller.rs
Строки-локации: 7
В чём заключается варнинг: unused imports: `Mat4` and `Vec2`
Тип: Предупреждение компиляции

КВарн№547
Варнинг в файле: C:\RFS-0.4.0\src\render\lighting\light.rs
Строки-локации: 5
В чём заключается варнинг: unused import: `Vec4`
Тип: Предупреждение компиляции

КВарн№548
Варнинг в файле: C:\RFS-0.4.0\src\render\lighting\probe.rs
Строки-локации: 8
В чём заключается варнинг: unused import: `Mat4`
Тип: Предупреждение компиляции

КВарн№549
Варнинг в файле: C:\RFS-0.4.0\src\render\postprocess\bloom.rs
Строки-локации: 11
В чём заключается варнинг: unused import: `PostProcessConfig`
Тип: Предупреждение компиляции

КВарн№550
Варнинг в файле: C:\RFS-0.4.0\src\render\postprocess\hdr.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `ToneMapping`
Тип: Предупреждение компиляции

КВарн№551
Варнинг в файле: C:\RFS-0.4.0\src\render\particles\system.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `std::collections::HashMap`
Тип: Предупреждение компиляции

КВарн№552
Варнинг в файле: C:\RFS-0.4.0\src\render\particles\system.rs
Строки-локации: 10
В чём заключается варнинг: unused import: `Mat4`
Тип: Предупреждение компиляции

КВарн№553
Варнинг в файле: C:\RFS-0.4.0\src\render\particles\system.rs
Строки-локации: 14
В чём заключается варнинг: unused import: `super::particle::Particle`
Тип: Предупреждение компиляции

КВарн№554
Варнинг в файле: C:\RFS-0.4.0\src\render\particles\emitter.rs
Строки-локации: 252
В чём заключается варнинг: unused import: `glam::Vec3Swizzles`
Тип: Предупреждение компиляции

КВарн№555
Варнинг в файле: C:\RFS-0.4.0\src\render\particles\effects\smoke.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::rhi::types::*`
Тип: Предупреждение компиляции

КВарн№556
Варнинг в файле: C:\RFS-0.4.0\src\render\particles\effects\fire.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::rhi::types::*`
Тип: Предупреждение компиляции

КВарн№557
Варнинг в файле: C:\RFS-0.4.0\src\render\particles\effects\splash.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::rhi::types::*`
Тип: Предупреждение компиляции

КВарн№558
Варнинг в файле: C:\RFS-0.4.0\src\render\particles\effects\explosion.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::rhi::types::*`
Тип: Предупреждение компиляции

КВарн№559
Варнинг в файле: C:\RFS-0.4.0\src\render\particles\effects\sparks.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::rhi::types::*`
Тип: Предупреждение компиляции

КВарн№560
Варнинг в файле: C:\RFS-0.4.0\src\render\particles\effects\dust.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::rhi::types::*`
Тип: Предупреждение компиляции

КВарн№561
Варнинг в файле: C:\RFS-0.4.0\src\render\particles\effects\blood.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::rhi::types::*`
Тип: Предупреждение компиляции

КВарн№562
Варнинг в файле: C:\RFS-0.4.0\src\render\particles\effects\magic.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `crate::rhi::types::*`
Тип: Предупреждение компиляции

КВарн№563
Варнинг в файле: C:\RFS-0.4.0\src\render\particles\effect_manager.rs
Строки-локации: 8
В чём заключается варнинг: unused imports: `Vec3` and `Vec4`
Тип: Предупреждение компиляции

КВарн№564
Варнинг в файле: C:\RFS-0.4.0\src\render\particles\effect_manager.rs
Строки-локации: 10
В чём заключается варнинг: unused import: `ParticleEmitter`
Тип: Предупреждение компиляции

КВарн№565
Варнинг в файле: C:\RFS-0.4.0\src\render\water\interaction.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `Vec2`
Тип: Предупреждение компиляции

КВарн№566
Варнинг в файле: C:\RFS-0.4.0\src\render\ui\hud.rs
Строки-локации: 5
В чём заключается варнинг: unused imports: `Mat4` and `Vec3`
Тип: Предупреждение компиляции

КВарн№567
Варнинг в файле: C:\RFS-0.4.0\src\render\ui\minimap.rs
Строки-локации: 5
В чём заключается варнинг: unused import: `Mat4`
Тип: Предупреждение компиляции

КВарн№568
Варнинг в файле: C:\RFS-0.4.0\src\render\effects\manager.rs
Строки-локации: 8
В чём заключается варнинг: unused imports: `Mat4` and `Vec4`
Тип: Предупреждение компиляции

КВарн№569
Варнинг в файле: C:\RFS-0.4.0\src\render\effects\manager.rs
Строки-локации: 10
В чём заключается варнинг: unused imports: `BloomConfig` and `MotionBlurConfig`
Тип: Предупреждение компиляции

КВарн№570
Варнинг в файле: C:\RFS-0.4.0\src\render\effects\underwater.rs
Строки-локации: 7
В чём заключается варнинг: unused import: `Mat4`
Тип: Предупреждение компиляции

КВарн№571
Варнинг в файле: C:\RFS-0.4.0\src\render\effects\underwater.rs
Строки-локации: 8
В чём заключается варнинг: unused import: `std::time::Duration`
Тип: Предупреждение компиляции

КВарн№572
Варнинг в файле: C:\RFS-0.4.0\src\render\effects\screen_space.rs
Строки-локации: 7
В чём заключается варнинг: unused imports: `Mat4` and `Vec4`
Тип: Предупреждение компиляции

КВарн№573
Варнинг в файле: C:\RFS-0.4.0\src\render\effects\screen_space.rs
Строки-локации: 123
В чём заключается варнинг: unused import: `std::f32::consts::PI`
Тип: Предупреждение компиляции

КВарн№574
Варнинг в файле: C:\RFS-0.4.0\без локации
Строки-локации: —
В чём заключается варнинг: `rfs-client` (lib) generated 63 warnings
Тип: Предупреждение компиляции